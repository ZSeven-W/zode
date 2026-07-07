use std::path::PathBuf;

fn main() -> anyhow::Result<()> {
    let out = std::env::args()
        .nth(1)
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("sdk/fixtures/jsonrpc"));
    std::fs::create_dir_all(&out)?;
    for fixture in zode_app_server_protocol::schema::fixture_messages() {
        std::fs::write(
            out.join(format!("{}.json", fixture.name)),
            serde_json::to_string_pretty(&fixture.value)?,
        )?;
    }
    Ok(())
}
