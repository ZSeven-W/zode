use crate::fs::{read_file_base64, write_file_base64};

#[tokio::test]
async fn write_then_read_base64_file() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("hello.txt");
    write_file_base64(&path, "aGVsbG8=").await.unwrap();
    assert_eq!(read_file_base64(&path).unwrap(), "aGVsbG8=");
}
