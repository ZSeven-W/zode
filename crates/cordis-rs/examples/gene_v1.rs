//! Compiled plugin candidate, generation 1. Speaks the cordis-rs
//! process-plugin JSON-lines protocol on stdout.

fn main() {
    let stdout = std::io::stdout();
    let mut stdout = stdout.lock();
    use std::io::Write;
    // Subscribe to probe events, then announce readiness.
    writeln!(stdout, "{{\"op\":\"listen\",\"event\":\"probe\"}}").unwrap();
    writeln!(
        stdout,
        "{{\"op\":\"log\",\"level\":\"info\",\"message\":\"gene v1 online\"}}"
    )
    .unwrap();
    for line in std::io::stdin().lines() {
        let Ok(line) = line else { break };
        writeln!(
            stdout,
            "{{\"op\":\"log\",\"level\":\"info\",\"message\":\"gene v1 handled {}\"}}",
            line.trim()
        )
        .unwrap();
        writeln!(
            stdout,
            "{{\"op\":\"emit\",\"event\":\"gene/result\",\"payload\":{{\"version\":\"v1\",\"ok\":true}}}}"
        )
        .unwrap();
    }
}
