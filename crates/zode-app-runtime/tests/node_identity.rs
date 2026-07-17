use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Barrier};

use zode_app_runtime::NodeIdentityStore;
use zode_node_protocol::NodeId;

static NEXT_TEST_DIR: AtomicU64 = AtomicU64::new(1);

struct TestDir(PathBuf);

impl TestDir {
    fn new(label: &str) -> Self {
        let unique = NEXT_TEST_DIR.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "zode-node-identity-{label}-{}-{unique}",
            std::process::id()
        ));
        fs::create_dir_all(&path).unwrap();
        Self(path)
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TestDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn node_identity_is_stable_across_runtime_restart() {
    let dir = TestDir::new("restart");

    let first = NodeIdentityStore::new(dir.path()).load_or_create().unwrap();
    let second = NodeIdentityStore::new(dir.path()).load_or_create().unwrap();

    assert_eq!(first, second);
}

#[test]
fn concurrent_first_start_creates_one_shared_identity() {
    const WORKERS: usize = 8;
    let dir = TestDir::new("concurrent");
    let barrier = Arc::new(Barrier::new(WORKERS));

    let handles: Vec<_> = (0..WORKERS)
        .map(|_| {
            let config_dir = dir.path().to_path_buf();
            let barrier = Arc::clone(&barrier);
            std::thread::spawn(move || {
                barrier.wait();
                NodeIdentityStore::new(config_dir).load_or_create().unwrap()
            })
        })
        .collect();

    let identities: Vec<_> = handles
        .into_iter()
        .map(|handle| handle.join().unwrap())
        .collect();
    assert!(identities.iter().all(|id| *id == identities[0]));
}

#[test]
fn corrupt_identity_returns_error_without_replacing_original_bytes() {
    let dir = TestDir::new("corrupt");
    let path = dir.path().join("node.json");
    let original = br#"{"version":1,"nodeId":"not-a-uuid"}"#;
    fs::write(&path, original).unwrap();

    let result = NodeIdentityStore::new(dir.path()).load_or_create();

    assert!(result.is_err());
    assert_eq!(fs::read(path).unwrap(), original);
}

#[test]
fn unknown_identity_version_returns_error_without_replacing_original_bytes() {
    let dir = TestDir::new("unknown-version");
    let path = dir.path().join("node.json");
    let original = serde_json::to_vec(&serde_json::json!({
        "version": 2,
        "nodeId": NodeId::new(),
    }))
    .unwrap();
    fs::write(&path, &original).unwrap();

    let result = NodeIdentityStore::new(dir.path()).load_or_create();

    assert!(result.is_err());
    assert_eq!(fs::read(path).unwrap(), original);
}
