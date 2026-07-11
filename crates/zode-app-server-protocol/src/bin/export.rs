use std::path::PathBuf;

fn main() -> anyhow::Result<()> {
    let mut args = std::env::args().skip(1);
    let out = match args.next().as_deref() {
        None => PathBuf::from("sdk/fixtures/jsonrpc"),
        Some("--out") => PathBuf::from(
            args.next()
                .ok_or_else(|| anyhow::anyhow!("--out requires a directory"))?,
        ),
        Some(argument) => anyhow::bail!("unexpected argument: {argument}"),
    };
    if let Some(argument) = args.next() {
        anyhow::bail!("unexpected argument: {argument}");
    }
    std::fs::create_dir_all(&out)?;
    for fixture in zode_app_server_protocol::schema::fixture_messages() {
        std::fs::write(
            out.join(format!("{}.json", fixture.name)),
            serde_json::to_string_pretty(&fixture.value)?,
        )?;
    }
    Ok(())
}
