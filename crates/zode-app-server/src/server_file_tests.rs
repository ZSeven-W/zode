use std::fs;

use tempfile::tempdir;

use crate::server_file::{cleanup_if_owner, generate_token, write_atomic, ServerFile};

fn file(pid: u32) -> ServerFile {
    ServerFile {
        port: 4321,
        pid,
        token: "secret".to_string(),
    }
}

#[test]
fn token_is_64_lowercase_hex_characters() {
    let token = generate_token();
    assert_eq!(token.len(), 64);
    assert!(token
        .bytes()
        .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte)));
}

#[cfg(unix)]
#[test]
fn atomic_write_creates_private_file_without_temp_artifacts() {
    use std::os::unix::fs::PermissionsExt;

    let dir = tempdir().unwrap();
    let path = write_atomic(dir.path(), &file(7)).unwrap();

    assert_eq!(path.file_name().unwrap(), "server.json");
    assert_eq!(
        fs::metadata(&path).unwrap().permissions().mode() & 0o777,
        0o600
    );
    assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 1);
}

#[test]
fn existing_server_file_is_refused() {
    let dir = tempdir().unwrap();
    let path = write_atomic(dir.path(), &file(7)).unwrap();
    let error = write_atomic(dir.path(), &file(8)).unwrap_err();

    assert!(error.to_string().contains(path.to_string_lossy().as_ref()));
    assert_eq!(
        serde_json::from_slice::<ServerFile>(&fs::read(path).unwrap())
            .unwrap()
            .pid,
        7
    );
}

#[test]
fn cleanup_only_removes_parseable_file_owned_by_pid() {
    let dir = tempdir().unwrap();
    let path = write_atomic(dir.path(), &file(7)).unwrap();

    cleanup_if_owner(dir.path(), 8);
    assert!(path.exists());
    cleanup_if_owner(dir.path(), 7);
    assert!(!path.exists());

    fs::write(&path, "not json").unwrap();
    cleanup_if_owner(dir.path(), 7);
    assert!(path.exists());
}
