use std::collections::{HashMap, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use chromiumoxide::browser::Browser;
use chromiumoxide::cdp::browser_protocol::browser::{
    DownloadProgressState, EventDownloadProgress, EventDownloadWillBegin,
    SetDownloadBehaviorBehavior, SetDownloadBehaviorParams,
};
use futures::StreamExt;

use super::backend::{BrowserError, DownloadEntry, DownloadStatus};

const DOWNLOAD_BUFFER_CAP: usize = 500;

#[derive(Debug)]
pub(crate) struct DownloadCache {
    cap: usize,
    entries: HashMap<String, DownloadEntry>,
    order: VecDeque<String>,
}

impl DownloadCache {
    pub(crate) fn new(cap: usize) -> Self {
        Self {
            cap,
            entries: HashMap::new(),
            order: VecDeque::new(),
        }
    }

    fn ensure(&mut self, guid: &str) {
        if self.entries.contains_key(guid) {
            return;
        }
        self.order.push_back(guid.to_string());
        self.entries.insert(
            guid.to_string(),
            DownloadEntry {
                status: DownloadStatus::InProgress,
                path: None,
                url: String::new(),
                received_bytes: 0,
                total_bytes: 0,
                error: None,
                attribution: None,
            },
        );
        self.evict();
    }

    fn begin(&mut self, guid: &str, url: &str) {
        self.ensure(guid);
        if let Some(entry) = self.entries.get_mut(guid) {
            entry.url = url.to_string();
        }
    }

    fn progress(
        &mut self,
        guid: &str,
        status: DownloadStatus,
        received_bytes: u64,
        total_bytes: u64,
        path: Option<PathBuf>,
    ) {
        self.ensure(guid);
        if let Some(entry) = self.entries.get_mut(guid) {
            entry.status = status;
            entry.received_bytes = received_bytes;
            entry.total_bytes = total_bytes;
            if status == DownloadStatus::Complete {
                entry.path = path;
            }
        }
        self.evict();
    }

    fn evict(&mut self) {
        while self.entries.len() > self.cap {
            let Some(index) = self.order.iter().position(|guid| {
                self.entries
                    .get(guid)
                    .is_some_and(|entry| entry.status != DownloadStatus::InProgress)
            }) else {
                break;
            };
            if let Some(guid) = self.order.remove(index) {
                self.entries.remove(&guid);
            }
        }
    }

    pub(crate) fn list(&self, limit: usize) -> Vec<DownloadEntry> {
        self.order
            .iter()
            .rev()
            .filter_map(|guid| self.entries.get(guid))
            .take(limit)
            .cloned()
            .collect()
    }
}

pub(crate) async fn configure(
    browser: &Browser,
    download_dir: &Path,
) -> Result<Arc<Mutex<DownloadCache>>, BrowserError> {
    let cache = Arc::new(Mutex::new(DownloadCache::new(DOWNLOAD_BUFFER_CAP)));
    attach_listeners(browser, cache.clone()).await?;
    browser
        .execute(
            SetDownloadBehaviorParams::builder()
                .behavior(SetDownloadBehaviorBehavior::Allow)
                .download_path(download_dir.to_string_lossy().into_owned())
                .events_enabled(true)
                .build()
                .map_err(BrowserError::Launch)?,
        )
        .await
        .map_err(|e| BrowserError::Launch(format!("download behavior: {e}")))?;
    Ok(cache)
}

async fn attach_listeners(
    browser: &Browser,
    cache: Arc<Mutex<DownloadCache>>,
) -> Result<(), BrowserError> {
    let mut begins = browser
        .event_listener::<EventDownloadWillBegin>()
        .await
        .map_err(|e| BrowserError::Launch(format!("download begin listener: {e}")))?;
    let begin_cache = cache.clone();
    tokio::spawn(async move {
        while let Some(event) = begins.next().await {
            begin_cache.lock().unwrap().begin(&event.guid, &event.url);
        }
    });

    let mut progress = browser
        .event_listener::<EventDownloadProgress>()
        .await
        .map_err(|e| BrowserError::Launch(format!("download progress listener: {e}")))?;
    tokio::spawn(async move {
        while let Some(event) = progress.next().await {
            let status = match event.state {
                DownloadProgressState::InProgress => DownloadStatus::InProgress,
                DownloadProgressState::Completed => DownloadStatus::Complete,
                DownloadProgressState::Canceled => DownloadStatus::Canceled,
            };
            cache.lock().unwrap().progress(
                &event.guid,
                status,
                event.received_bytes.max(0.0) as u64,
                event.total_bytes.max(0.0) as u64,
                event.file_path.as_ref().map(PathBuf::from),
            );
        }
    });
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn merges_and_returns_newest_first() {
        let mut cache = DownloadCache::new(10);
        cache.begin("a", "https://x/a");
        cache.begin("b", "https://x/b");
        cache.progress(
            "a",
            DownloadStatus::Complete,
            4,
            4,
            Some(PathBuf::from("/tmp/a")),
        );
        let entries = cache.list(10);
        assert_eq!(entries[0].url, "https://x/b");
        assert_eq!(entries[1].status, DownloadStatus::Complete);
        assert_eq!(entries[1].path.as_deref(), Some(Path::new("/tmp/a")));
    }

    #[test]
    fn evicts_oldest_terminal_but_keeps_in_progress() {
        let mut cache = DownloadCache::new(2);
        cache.begin("active", "https://x/active");
        cache.begin("done", "https://x/done");
        cache.progress("done", DownloadStatus::Complete, 1, 1, None);
        cache.begin("new", "https://x/new");
        let entries = cache.list(10);
        assert_eq!(entries.len(), 2);
        assert!(entries.iter().any(|entry| entry.url.ends_with("active")));
        assert!(entries.iter().any(|entry| entry.url.ends_with("new")));
    }
}
