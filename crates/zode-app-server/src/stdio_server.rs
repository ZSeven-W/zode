use crate::router::Router;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use zode_app_server_protocol::JsonRpcMessage;

pub async fn run_stdio(zode_home: String) -> std::io::Result<()> {
    let stdin = tokio::io::stdin();
    let mut stdout = tokio::io::stdout();
    let mut lines = BufReader::new(stdin).lines();
    let mut router = Router::new(zode_home);
    while let Some(line) = lines.next_line().await? {
        let message = match zode_app_server_transport::stdio::decode_line(&line) {
            Ok(message) => message,
            Err(_) => continue,
        };
        if let JsonRpcMessage::Request(request) = message {
            let response = match router.handle_request(request) {
                Ok(response) => JsonRpcMessage::Response(response),
                Err(error) => JsonRpcMessage::Error(error),
            };
            let encoded = zode_app_server_transport::stdio::encode_message(&response)
                .map_err(|err| std::io::Error::new(std::io::ErrorKind::InvalidData, err))?;
            stdout.write_all(encoded.as_bytes()).await?;
            stdout.flush().await?;
        }
    }
    Ok(())
}
