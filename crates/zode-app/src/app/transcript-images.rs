use std::{
    collections::{HashMap, HashSet},
    io::Cursor,
    sync::Arc,
};

use tokio::sync::mpsc;
use winit::event_loop::EventLoopProxy;
use zode_app_model::{AppCommand, ImageItem, TranscriptItem, ZodeAppState};
use zode_app_ui::{corrected_card_height, TranscriptImageBytes, TranscriptImageSource};
use zode_node_protocol::SessionLocator;

use crate::window_state::AppWake;

use super::DesktopApp;

/// Refuses to read a source file bigger than this rather than blocking on
/// (or exhausting memory for) a pathological multi-hundred-MB file a tool
/// happened to touch and that this loader then tried to decode inline.
const MAX_SOURCE_BYTES: u64 = 20 * 1024 * 1024;

/// Upper bound on total re-encoded bytes held across every cached item.
/// Mirrors `NativeBackend::IMAGE_SOURCE_CACHE_CAP`'s LRU-eviction idea, but
/// budgets by content size rather than entry count - thumbnail byte sizes
/// vary far more than the render backend's fixed-size browser frames do.
const MAX_CACHE_TOTAL_BYTES: u64 = 64 * 1024 * 1024;

/// SVG documents are never rasterized here (a full SVG renderer was
/// rejected on `cargo-deny` advisories elsewhere in this codebase) - an SVG
/// item keeps the icon-tile placeholder/click-to-open path forever.
const SVG_MEDIA_TYPE: &str = "image/svg+xml";

/// Outcome of resolving one `ImageItem::path` to pixels, cached per item id.
#[derive(Debug, Clone, PartialEq)]
pub(super) enum TranscriptImageState {
    /// PNG-re-encoded bytes ready for `ImageSource::KeyedBytes`, plus the
    /// natural pixel size read from the original file.
    Loaded {
        bytes: Arc<Vec<u8>>,
        width: u32,
        height: u32,
    },
    /// The source file exceeded `MAX_SOURCE_BYTES`.
    TooLarge,
    /// SVG, or any other format this loader intentionally never rasterizes.
    Unsupported,
    /// Missing file, unreadable, or an image decode failure (corrupt or
    /// unrecognized bytes) - never a panic, always this state.
    DecodeFailed,
}

/// Blocking file-to-pixels resolution, isolated behind a trait so tests can
/// substitute a deterministic fake instead of touching the filesystem
/// (mirrors `ExternalApplicationService` in `app/open-with.rs`).
pub(super) trait TranscriptImageLoader: Send + Sync {
    fn load(&self, path: &str) -> TranscriptImageState;
}

struct LocalTranscriptImageLoader;

impl LocalTranscriptImageLoader {
    /// `max_bytes` is a parameter (rather than always `MAX_SOURCE_BYTES`) so
    /// tests can exercise the "too large" branch against a small fixture
    /// file instead of allocating a real 20 MiB+ one.
    fn load_capped(path: &str, max_bytes: u64) -> TranscriptImageState {
        let metadata = match std::fs::metadata(path) {
            Ok(metadata) => metadata,
            Err(_) => return TranscriptImageState::DecodeFailed,
        };
        if metadata.len() > max_bytes {
            return TranscriptImageState::TooLarge;
        }
        let bytes = match std::fs::read(path) {
            Ok(bytes) => bytes,
            Err(_) => return TranscriptImageState::DecodeFailed,
        };
        let decoded = match image::load_from_memory(&bytes) {
            Ok(decoded) => decoded,
            Err(_) => return TranscriptImageState::DecodeFailed,
        };
        let (width, height) = (decoded.width(), decoded.height());
        let mut encoded = Vec::new();
        if decoded
            .write_to(&mut Cursor::new(&mut encoded), image::ImageFormat::Png)
            .is_err()
        {
            return TranscriptImageState::DecodeFailed;
        }
        TranscriptImageState::Loaded {
            bytes: Arc::new(encoded),
            width,
            height,
        }
    }
}

impl TranscriptImageLoader for LocalTranscriptImageLoader {
    fn load(&self, path: &str) -> TranscriptImageState {
        Self::load_capped(path, MAX_SOURCE_BYTES)
    }
}

struct CacheEntry {
    state: TranscriptImageState,
    /// Insertion-order tick used for LRU eviction. `lookup` (the trait
    /// method) takes `&self`, so recency can only be tracked on write, not
    /// on read - an acceptable simplification given the trait signature is
    /// shared with the lightbox and inline-card call sites.
    inserted_tick: u64,
}

/// One completed background load, delivered exactly once via the unbounded
/// channel below.
struct LoadOutcome {
    item_id: String,
    state: TranscriptImageState,
}

/// Loads `TranscriptItem::Image` source files away from the winit thread and
/// caches the result for `ThreadTranscript`/`Lightbox` to paint, reporting
/// completion through the same coalesced redraw path as the other desktop
/// effects (`BranchCatalogEffect`, `OpenWithEffect`).
pub(super) struct TranscriptImageEffect {
    loader: Arc<dyn TranscriptImageLoader>,
    cache: HashMap<String, CacheEntry>,
    in_flight: HashSet<String>,
    result_sender: mpsc::UnboundedSender<LoadOutcome>,
    results: mpsc::UnboundedReceiver<LoadOutcome>,
    wake: Arc<dyn Fn() + Send + Sync>,
    tick: u64,
}

impl TranscriptImageEffect {
    pub(super) fn new(proxy: EventLoopProxy<AppWake>) -> Self {
        Self::with_loader_and_wake(Arc::new(LocalTranscriptImageLoader), move || {
            let _ = proxy.send_event(AppWake::Redraw);
        })
    }

    fn with_loader_and_wake(
        loader: Arc<dyn TranscriptImageLoader>,
        wake: impl Fn() + Send + Sync + 'static,
    ) -> Self {
        let (result_sender, results) = mpsc::unbounded_channel();
        Self {
            loader,
            cache: HashMap::new(),
            in_flight: HashSet::new(),
            result_sender,
            results,
            wake: Arc::new(wake),
            tick: 0,
        }
    }

    /// Starts a background load for `item` unless it is already cached
    /// (loaded, too-large, unsupported, or failed) or already in flight.
    /// SVG never spawns a load - it is marked `Unsupported` synchronously.
    pub(super) fn ensure_requested(&mut self, item: &ImageItem) {
        if self.cache.contains_key(&item.id) || self.in_flight.contains(&item.id) {
            return;
        }
        if item.media_type == SVG_MEDIA_TYPE {
            self.insert(item.id.clone(), TranscriptImageState::Unsupported);
            return;
        }
        self.in_flight.insert(item.id.clone());
        let sender = self.result_sender.clone();
        let wake = Arc::clone(&self.wake);
        let loader = Arc::clone(&self.loader);
        let item_id = item.id.clone();
        let path = item.path.clone();
        tokio::spawn(async move {
            let state = tokio::task::spawn_blocking(move || loader.load(&path))
                .await
                .unwrap_or(TranscriptImageState::DecodeFailed);
            if sender.send(LoadOutcome { item_id, state }).is_ok() {
                wake();
            }
        });
    }

    /// Drains every load that completed since the last call, caching each
    /// result. Each item id is delivered at most once across the process's
    /// lifetime for this effect - the channel message is consumed here and
    /// never re-sent, so callers dispatching a one-time height correction
    /// off this return value (see `DesktopApp::correct_image_height`) never
    /// need extra bookkeeping to avoid firing it twice.
    pub(super) fn drain(&mut self) -> Vec<(String, TranscriptImageState)> {
        let mut newly_loaded = Vec::new();
        while let Ok(outcome) = self.results.try_recv() {
            self.in_flight.remove(&outcome.item_id);
            newly_loaded.push((outcome.item_id.clone(), outcome.state.clone()));
            self.insert(outcome.item_id, outcome.state);
        }
        newly_loaded
    }

    fn insert(&mut self, item_id: String, state: TranscriptImageState) {
        self.tick += 1;
        self.cache.insert(
            item_id,
            CacheEntry {
                state,
                inserted_tick: self.tick,
            },
        );
        self.evict_if_over_budget();
    }

    fn evict_if_over_budget(&mut self) {
        while self.total_bytes() > MAX_CACHE_TOTAL_BYTES {
            let Some(victim) = self
                .cache
                .iter()
                .min_by_key(|(_, entry)| entry.inserted_tick)
                .map(|(id, _)| id.clone())
            else {
                break;
            };
            self.cache.remove(&victim);
        }
    }

    fn total_bytes(&self) -> u64 {
        self.cache
            .values()
            .filter_map(|entry| match &entry.state {
                TranscriptImageState::Loaded { bytes, .. } => Some(bytes.len() as u64),
                _ => None,
            })
            .sum()
    }
}

impl TranscriptImageSource for TranscriptImageEffect {
    fn lookup(&self, item: &ImageItem) -> Option<TranscriptImageBytes<'_>> {
        match &self.cache.get(&item.id)?.state {
            TranscriptImageState::Loaded {
                bytes,
                width,
                height,
            } => Some(TranscriptImageBytes {
                encoded: bytes.as_slice(),
                width: *width,
                height: *height,
            }),
            TranscriptImageState::TooLarge
            | TranscriptImageState::Unsupported
            | TranscriptImageState::DecodeFailed => None,
        }
    }
}

impl DesktopApp {
    /// Called once per redraw tick (see `AppWake::Redraw` in `app.rs`,
    /// mirroring `poll_browser_frame`): requests loads for any new `Image`
    /// items in the current transcript, then drains completed loads and
    /// corrects each freshly-decoded item's card height exactly once.
    /// Returns whether anything changed, so the caller knows to rebuild the
    /// frame snapshot like the other background-effect drains do.
    pub(super) fn poll_transcript_images(&mut self) -> bool {
        self.request_visible_transcript_images();
        let newly_loaded = self.transcript_images.drain();
        if newly_loaded.is_empty() {
            return false;
        }
        let card_width = self.frame_snapshot.layout.transcript.size.x;
        if card_width > 0.0 {
            for (item_id, state) in &newly_loaded {
                if let TranscriptImageState::Loaded { width, height, .. } = state {
                    self.correct_image_height(item_id, *width, *height, card_width);
                }
            }
        }
        true
    }

    fn request_visible_transcript_images(&mut self) {
        let Some(session) = self.app_state.current_session.clone() else {
            return;
        };
        let Some(transcript) = self.app_state.transcripts.get(&session) else {
            return;
        };
        for item in &transcript.items {
            if let TranscriptItem::Image(image) = item {
                self.transcript_images.ensure_requested(image);
            }
        }
    }

    /// Dispatches the existing `AppCommand::SetTranscriptItemHeight` with
    /// the exact card height `corrected_card_height` computes for the now-
    /// known natural size, so the placeholder-sized layout slot snaps to
    /// the real aspect ratio. A no-op if the item can no longer be found
    /// (e.g. a session was cleared while its load was in flight).
    fn correct_image_height(&mut self, item_id: &str, width: u32, height: u32, card_width: f32) {
        let Some((session, index, image)) = locate_image_item(&self.app_state, item_id) else {
            return;
        };
        let sized = ImageItem {
            width: Some(width),
            height: Some(height),
            ..image
        };
        let card_height = corrected_card_height(&sized, card_width);
        self.enqueue_command(AppCommand::SetTranscriptItemHeight {
            session: session.clone(),
            index,
            height: card_height,
        });
        // Backfill the item's natural size so the lightbox scales its zoom
        // steps against real dimensions instead of the fill-available
        // fallback. Addressed by id, so a stale in-flight decode can never
        // resize a different item after transcript edits.
        self.enqueue_command(AppCommand::SetTranscriptImageDimensions {
            session,
            item_id: item_id.to_owned(),
            width,
            height,
        });
    }
}

/// Finds the session and positional index of the transcript item whose
/// `ImageItem::id` matches `item_id`, plus a clone of the item itself, so
/// the caller can dispatch a command against that exact position without
/// holding a borrow of `state` across the dispatch.
fn locate_image_item(
    state: &ZodeAppState,
    item_id: &str,
) -> Option<(SessionLocator, usize, ImageItem)> {
    for (session, transcript) in state.transcripts.iter() {
        for (index, item) in transcript.items.iter().enumerate() {
            if let TranscriptItem::Image(image) = item {
                if image.id == item_id {
                    return Some((session.clone(), index, image.clone()));
                }
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use std::sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    };

    use tokio::sync::Notify;
    use zode_app_model::{demo_state, ImageItem, TranscriptItem};
    use zode_node_protocol::SessionLocator;

    use zode_app_ui::TranscriptImageSource;

    use super::{
        locate_image_item, LocalTranscriptImageLoader, TranscriptImageEffect,
        TranscriptImageLoader, TranscriptImageState,
    };

    fn image_item(id: &str, path: &str, media_type: &str) -> ImageItem {
        ImageItem {
            id: id.to_owned(),
            path: path.to_owned(),
            media_type: media_type.to_owned(),
            width: None,
            height: None,
        }
    }

    struct FixedLoader {
        state: TranscriptImageState,
        calls: Arc<AtomicUsize>,
    }

    impl TranscriptImageLoader for FixedLoader {
        fn load(&self, _path: &str) -> TranscriptImageState {
            self.calls.fetch_add(1, Ordering::SeqCst);
            self.state.clone()
        }
    }

    fn effect_with_fixed_result(
        state: TranscriptImageState,
    ) -> (TranscriptImageEffect, Arc<AtomicUsize>, Arc<Notify>) {
        let calls = Arc::new(AtomicUsize::new(0));
        let wake = Arc::new(Notify::new());
        let loader_calls = Arc::clone(&calls);
        let worker_wake = Arc::clone(&wake);
        let effect = TranscriptImageEffect::with_loader_and_wake(
            Arc::new(FixedLoader {
                state,
                calls: loader_calls,
            }),
            move || worker_wake.notify_one(),
        );
        (effect, calls, wake)
    }

    #[tokio::test]
    async fn a_request_transitions_from_in_flight_to_a_loaded_cache_entry() {
        let (mut effect, calls, wake) = effect_with_fixed_result(TranscriptImageState::Loaded {
            bytes: Arc::new(vec![1, 2, 3]),
            width: 10,
            height: 20,
        });
        let item = image_item("image:1", "/tmp/does-not-matter.png", "image/png");

        effect.ensure_requested(&item);
        assert!(effect.in_flight.contains(&item.id));
        assert!(effect.cache.is_empty());

        tokio::time::timeout(std::time::Duration::from_secs(5), wake.notified())
            .await
            .expect("the loader should wake the app once the load completes");

        let drained = effect.drain();
        assert_eq!(drained.len(), 1);
        assert_eq!(drained[0].0, item.id);
        assert!(matches!(
            drained[0].1,
            TranscriptImageState::Loaded {
                width: 10,
                height: 20,
                ..
            }
        ));
        assert!(!effect.in_flight.contains(&item.id));
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn a_completed_item_is_delivered_by_drain_exactly_once() {
        let (mut effect, _calls, wake) = effect_with_fixed_result(TranscriptImageState::Loaded {
            bytes: Arc::new(vec![9]),
            width: 1,
            height: 1,
        });
        let item = image_item("image:once", "/tmp/whatever.png", "image/png");

        effect.ensure_requested(&item);
        tokio::time::timeout(std::time::Duration::from_secs(5), wake.notified())
            .await
            .expect("load should complete");

        assert_eq!(effect.drain().len(), 1);
        // The item is now cached, so a repeat request is a no-op and a
        // second drain (nothing new arrived) returns nothing - the height
        // correction this feeds can only ever fire once for this item.
        effect.ensure_requested(&item);
        assert_eq!(effect.drain().len(), 0);
    }

    #[tokio::test]
    async fn svg_items_are_marked_unsupported_without_spawning_a_load() {
        struct PanicLoader;
        impl TranscriptImageLoader for PanicLoader {
            fn load(&self, _path: &str) -> TranscriptImageState {
                panic!("SVG items must never reach the loader");
            }
        }
        let mut effect = TranscriptImageEffect::with_loader_and_wake(Arc::new(PanicLoader), || {});
        let item = image_item("image:svg", "/tmp/icon.svg", "image/svg+xml");

        effect.ensure_requested(&item);

        assert!(!effect.in_flight.contains(&item.id));
        assert!(matches!(
            effect.cache.get(&item.id).map(|entry| &entry.state),
            Some(TranscriptImageState::Unsupported)
        ));
        assert!(effect.lookup(&item).is_none());
    }

    #[tokio::test]
    async fn cache_eviction_keeps_total_bytes_under_the_budget() {
        let mut effect = TranscriptImageEffect::with_loader_and_wake(Arc::new(PanicOnLoad), || {});
        // Each entry is deliberately large so a handful of them exceed
        // `MAX_CACHE_TOTAL_BYTES` (64 MiB) and force eviction.
        let big = vec![0u8; 20 * 1024 * 1024];
        for index in 0..5 {
            effect.insert(
                format!("image:{index}"),
                TranscriptImageState::Loaded {
                    bytes: Arc::new(big.clone()),
                    width: 100,
                    height: 100,
                },
            );
        }

        assert!(effect.total_bytes() <= super::MAX_CACHE_TOTAL_BYTES);
        // The oldest insertions were evicted first.
        assert!(!effect.cache.contains_key("image:0"));
        assert!(effect.cache.contains_key("image:4"));
    }

    struct PanicOnLoad;
    impl TranscriptImageLoader for PanicOnLoad {
        fn load(&self, _path: &str) -> TranscriptImageState {
            panic!("this test never spawns a real load");
        }
    }

    #[test]
    fn local_loader_rejects_files_over_a_configured_size_cap() {
        let dir = std::env::temp_dir();
        let path = dir.join(format!(
            "zode-image-cap-{}.png",
            uuid::Uuid::new_v4().simple()
        ));
        std::fs::write(&path, b"not actually 11 bytes of png").unwrap();

        let state = LocalTranscriptImageLoader::load_capped(path.to_str().unwrap(), 10);

        assert_eq!(state, TranscriptImageState::TooLarge);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn local_loader_reports_decode_failed_for_a_missing_file() {
        let state = LocalTranscriptImageLoader::load_capped(
            "/tmp/zode-image-loader-missing-file-fixture.png",
            super::MAX_SOURCE_BYTES,
        );
        assert_eq!(state, TranscriptImageState::DecodeFailed);
    }

    #[tokio::test]
    async fn the_real_async_loader_decodes_a_png_and_reports_natural_dimensions() {
        let dir = std::env::temp_dir();
        let path = dir.join(format!(
            "zode-image-real-{}.png",
            uuid::Uuid::new_v4().simple()
        ));
        let png = tiny_test_png(6, 4);
        std::fs::write(&path, &png).unwrap();

        let wake = Arc::new(Notify::new());
        let worker_wake = Arc::clone(&wake);
        let mut effect = TranscriptImageEffect::with_loader_and_wake(
            Arc::new(LocalTranscriptImageLoader),
            move || worker_wake.notify_one(),
        );
        let item = image_item("image:real", path.to_str().unwrap(), "image/png");

        effect.ensure_requested(&item);
        tokio::time::timeout(std::time::Duration::from_secs(5), wake.notified())
            .await
            .expect("the real async load should complete and wake the app");
        let drained = effect.drain();

        assert_eq!(drained.len(), 1);
        assert!(matches!(
            drained[0].1,
            TranscriptImageState::Loaded {
                width: 6,
                height: 4,
                ..
            }
        ));

        let _ = std::fs::remove_file(&path);
    }

    fn tiny_test_png(width: u32, height: u32) -> Vec<u8> {
        let image = image::RgbaImage::from_pixel(width, height, image::Rgba([10, 20, 30, 255]));
        let mut bytes = Vec::new();
        image::DynamicImage::ImageRgba8(image)
            .write_to(
                &mut std::io::Cursor::new(&mut bytes),
                image::ImageFormat::Png,
            )
            .unwrap();
        bytes
    }

    #[test]
    fn locate_image_item_finds_the_owning_session_and_index() {
        let mut state = demo_state();
        let session = SessionLocator::new(state.host.node_id, "transcript-images-test");
        let transcript = state.transcripts.entry(session.clone()).or_default();
        transcript.items.push(TranscriptItem::Status {
            code: "placeholder".into(),
            message: "placeholder".into(),
        });
        transcript.items.push(TranscriptItem::Image(image_item(
            "image:target",
            "/tmp/target.png",
            "image/png",
        )));

        let found = locate_image_item(&state, "image:target");

        assert_eq!(
            found.map(|(found_session, index, item)| (found_session, index, item.id)),
            Some((session, 1, "image:target".to_owned()))
        );
    }

    #[test]
    fn locate_image_item_returns_none_for_an_unknown_id() {
        let state = demo_state();
        assert!(locate_image_item(&state, "image:missing").is_none());
    }
}
