use std::hash::{Hash, Hasher};

use super::WidgetId;

/// Stable FNV-1a IDs occupy a namespace byte plus a deterministic 56-bit
/// payload. This never depends on `RandomState` or process entropy.
pub(crate) fn stable_widget_id<T: Hash + ?Sized>(namespace: u8, key: &T) -> WidgetId {
    let mut hasher = StableFnvHasher::new();
    namespace.hash(&mut hasher);
    key.hash(&mut hasher);
    let payload = hasher.finish() & 0x00ff_ffff_ffff_ffff;
    WidgetId((u64::from(namespace) << 56) | payload)
}

struct StableFnvHasher(u64);

impl StableFnvHasher {
    const fn new() -> Self {
        Self(0xcbf2_9ce4_8422_2325)
    }
}

impl Hasher for StableFnvHasher {
    fn finish(&self) -> u64 {
        self.0
    }

    fn write(&mut self, bytes: &[u8]) {
        for byte in bytes {
            self.0 ^= u64::from(*byte);
            self.0 = self.0.wrapping_mul(0x0000_0100_0000_01b3);
        }
    }
}
