use std::path::PathBuf;

fn main() {
    let Some(path) = std::env::args_os().nth(1).map(PathBuf::from) else {
        eprintln!("usage: zode-pty-scenario <scenario.json>");
        std::process::exit(2);
    };
    match zode_pty_harness::load_scenario(&path).and_then(|scenario| scenario.run()) {
        Ok(snapshot) => println!("{}", serde_json::to_string_pretty(&snapshot).unwrap()),
        Err(error) => {
            eprintln!("zode-pty-scenario: {error:#}");
            std::process::exit(1);
        }
    }
}
