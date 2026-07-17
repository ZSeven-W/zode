use std::time::Duration;

use futures_util::StreamExt;
use zode_app::services::{LocalTerminalService, TerminalService};

#[test]
fn resize_reaches_the_native_pty_master() {
    let cwd = std::env::current_dir().unwrap();
    let service = LocalTerminalService::new();
    let terminal = service.spawn(&cwd).unwrap();

    service.resize(terminal, 120, 40).unwrap();

    assert_eq!(service.size(terminal).unwrap(), (120, 40));
    service.close(terminal).unwrap();
}

#[tokio::test]
async fn shell_output_streams_and_close_joins_the_session() {
    let cwd = std::env::current_dir().unwrap();
    let service = LocalTerminalService::new();
    let terminal = service.spawn(&cwd).unwrap();
    let mut output = service.subscribe(terminal).unwrap();
    service
        .write(terminal, b"echo zode-terminal-ready\r\n".to_vec())
        .unwrap();

    let received = tokio::time::timeout(Duration::from_secs(5), async {
        let mut bytes = Vec::new();
        while let Some(chunk) = output.next().await {
            bytes.extend(chunk.unwrap());
            if String::from_utf8_lossy(&bytes).contains("zode-terminal-ready") {
                return bytes;
            }
        }
        bytes
    })
    .await
    .expect("terminal output timed out");

    assert!(String::from_utf8_lossy(&received).contains("zode-terminal-ready"));
    service.close(terminal).unwrap();
    assert!(service.size(terminal).is_err());
}
