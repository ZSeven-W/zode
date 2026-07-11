use std::path::Path;

use base64::Engine;

pub fn read_file_base64(path: &Path) -> std::io::Result<String> {
    let bytes = std::fs::read(path)?;
    Ok(base64::engine::general_purpose::STANDARD.encode(bytes))
}

pub async fn write_file_base64(path: &Path, data_base64: &str) -> std::io::Result<()> {
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(data_base64)
        .map_err(std::io::Error::other)?;
    if let Some(parent) = path.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }
    tokio::fs::write(path, bytes).await
}
