//! Compiled plugin candidate, generation 2 — the "evolved" replacement
//! for gene_v1, compiled independently and swapped in at runtime.

fn main() {
    let stdout = std::io::stdout();
    let mut stdout = stdout.lock();
    use std::io::Write;
    writeln!(stdout, "{{\"op\":\"listen\",\"event\":\"probe\"}}").unwrap();
    writeln!(
        stdout,
        "{{\"op\":\"log\",\"level\":\"info\",\"message\":\"gene v2 online (replacement)\"}}"
    )
    .unwrap();
    for line in std::io::stdin().lines() {
        let Ok(line) = line else { break };
        writeln!(
            stdout,
            "{{\"op\":\"log\",\"level\":\"info\",\"message\":\"gene v2 handled {}\"}}",
            line.trim()
        )
        .unwrap();
        writeln!(
            stdout,
            "{{\"op\":\"emit\",\"event\":\"gene/result\",\"payload\":{{\"version\":\"v2\",\"ok\":true}}}}"
        )
        .unwrap();
    }
}
