use crate::fs::{read_file_base64, write_file_base64};
use crate::router::Router;
use zode_app_server_protocol::{JsonRpcRequest, RequestId};

#[tokio::test]
async fn write_then_read_base64_file() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("hello.txt");
    write_file_base64(&path, "aGVsbG8=").await.unwrap();
    assert_eq!(read_file_base64(&path).unwrap(), "aGVsbG8=");
}

fn init(router: &mut Router) {
    router
        .handle_request(JsonRpcRequest::new(
            RequestId::Number(0),
            "initialize".to_string(),
            Some(serde_json::json!({
                "clientInfo": {"name": "test", "version": "0.0.0"}
            })),
        ))
        .unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn router_write_read_stat_list_copy_remove() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("nested").join("hello.txt");
    let copy = dir.path().join("copy.txt");
    let mut router = Router::for_tests("/tmp/zode");
    init(&mut router);

    router
        .handle_request(JsonRpcRequest::new(
            RequestId::Number(1),
            "fs/writeFile".to_string(),
            Some(serde_json::json!({
                "path": file.display().to_string(),
                "dataBase64": "aGVsbG8="
            })),
        ))
        .unwrap();
    let read = router
        .handle_request(JsonRpcRequest::new(
            RequestId::Number(2),
            "fs/readFile".to_string(),
            Some(serde_json::json!({"path": file.display().to_string()})),
        ))
        .unwrap();
    assert_eq!(read.result["dataBase64"], "aGVsbG8=");

    let meta = router
        .handle_request(JsonRpcRequest::new(
            RequestId::Number(3),
            "fs/getMetadata".to_string(),
            Some(serde_json::json!({"path": file.display().to_string()})),
        ))
        .unwrap();
    assert_eq!(meta.result["isFile"], true);

    let list = router
        .handle_request(JsonRpcRequest::new(
            RequestId::Number(4),
            "fs/readDirectory".to_string(),
            Some(serde_json::json!({"path": file.parent().unwrap().display().to_string()})),
        ))
        .unwrap();
    assert_eq!(list.result["entries"][0]["fileName"], "hello.txt");

    router
        .handle_request(JsonRpcRequest::new(
            RequestId::Number(5),
            "fs/copy".to_string(),
            Some(serde_json::json!({
                "sourcePath": file.display().to_string(),
                "destinationPath": copy.display().to_string()
            })),
        ))
        .unwrap();
    assert_eq!(read_file_base64(&copy).unwrap(), "aGVsbG8=");

    router
        .handle_request(JsonRpcRequest::new(
            RequestId::Number(6),
            "fs/remove".to_string(),
            Some(serde_json::json!({"path": copy.display().to_string()})),
        ))
        .unwrap();
    assert!(!copy.exists());
}
