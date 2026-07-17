#![cfg_attr(target_os = "windows", windows_subsystem = "windows")]
#![forbid(unsafe_code)]

use std::{env, fs, path::PathBuf};

fn main() {
    if let Err(error) = run(env::args().skip(1)) {
        eprintln!("zode-app: {error}");
        std::process::exit(1);
    }
}

fn run(mut args: impl Iterator<Item = String>) -> Result<(), Box<dyn std::error::Error>> {
    match args.next().as_deref() {
        Some("--render-snapshot") => {
            let path = PathBuf::from(args.next().ok_or("--render-snapshot requires a path")?);
            if args.next().is_some() {
                return Err("unexpected arguments after snapshot path".into());
            }
            let png =
                zode_app::render::render_offscreen(&zode_app_model::demo_state(), 1221, 992, 1.0)?;
            if let Some(parent) = path
                .parent()
                .filter(|parent| !parent.as_os_str().is_empty())
            {
                fs::create_dir_all(parent)?;
            }
            fs::write(&path, png)?;
            println!("rendered {}", path.display());
            Ok(())
        }
        Some("--demo") => zode_app::app::run_demo(),
        Some(argument) => Err(format!("unknown argument: {argument}").into()),
        None => zode_app::app::run_demo(),
    }
}
