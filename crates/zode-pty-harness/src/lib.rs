//! Real-PTY regression harness for the Zode TUI.
//!
//! The harness keeps raw bytes for diagnostics and feeds the same bytes into a
//! VT100 virtual screen, so assertions describe what a user sees rather than
//! relying only on escape-code substrings.

use std::collections::BTreeMap;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError};
use std::time::{Duration, Instant};

use anyhow::{bail, Context, Result};
use portable_pty::{native_pty_system, Child, CommandBuilder, MasterPty, PtySize};
use serde::{Deserialize, Serialize};

const RAW_TAIL_BYTES: usize = 32 * 1024;

#[derive(Debug, Clone)]
pub struct HarnessConfig {
    pub command: Vec<String>,
    pub cwd: Option<PathBuf>,
    pub env: BTreeMap<String, String>,
    pub rows: u16,
    pub cols: u16,
}

impl HarnessConfig {
    pub fn new(command: impl Into<String>) -> Self {
        Self {
            command: vec![command.into()],
            cwd: None,
            env: BTreeMap::new(),
            rows: 40,
            cols: 120,
        }
    }
}

pub struct PtyHarness {
    master: Box<dyn MasterPty + Send>,
    child: Box<dyn Child + Send + Sync>,
    writer: Box<dyn Write + Send>,
    chunks: Receiver<Vec<u8>>,
    parser: vt100::Parser,
    raw: Vec<u8>,
    rows: u16,
    cols: u16,
}

impl PtyHarness {
    pub fn spawn(config: &HarnessConfig) -> Result<Self> {
        let program = config
            .command
            .first()
            .context("PTY command cannot be empty")?;
        let pair = native_pty_system()
            .openpty(PtySize {
                rows: config.rows,
                cols: config.cols,
                pixel_width: 0,
                pixel_height: 0,
            })
            .context("open PTY")?;
        let mut command = CommandBuilder::new(program);
        command.args(&config.command[1..]);
        if let Some(cwd) = &config.cwd {
            command.cwd(cwd);
        }
        command.env("TERM", "xterm-256color");
        command.env("COLORTERM", "truecolor");
        for (key, value) in &config.env {
            command.env(key, value);
        }
        let child = pair
            .slave
            .spawn_command(command)
            .context("spawn PTY child")?;
        let mut reader = pair.master.try_clone_reader().context("clone PTY reader")?;
        let writer = pair.master.take_writer().context("take PTY writer")?;
        let (tx, chunks) = mpsc::channel();
        std::thread::Builder::new()
            .name("zode-pty-reader".into())
            .spawn(move || {
                let mut buffer = [0u8; 64 * 1024];
                loop {
                    match reader.read(&mut buffer) {
                        Ok(0) => break,
                        Ok(size) if tx.send(buffer[..size].to_vec()).is_err() => break,
                        Ok(_) => {}
                        Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
                        Err(_) => break,
                    }
                }
            })
            .context("spawn PTY reader thread")?;
        Ok(Self {
            master: pair.master,
            child,
            writer,
            chunks,
            parser: vt100::Parser::new(config.rows, config.cols, 10_000),
            raw: Vec::new(),
            rows: config.rows,
            cols: config.cols,
        })
    }

    pub fn send_bytes(&mut self, bytes: &[u8]) -> Result<()> {
        self.writer.write_all(bytes).context("write PTY input")?;
        self.writer.flush().context("flush PTY input")
    }

    pub fn send_keys(&mut self, notation: &str) -> Result<()> {
        self.send_bytes(&parse_keys(notation)?)
    }

    pub fn resize(&mut self, rows: u16, cols: u16) -> Result<()> {
        self.master
            .resize(PtySize {
                rows,
                cols,
                pixel_width: 0,
                pixel_height: 0,
            })
            .context("resize PTY")?;
        self.parser.set_size(rows, cols);
        self.rows = rows;
        self.cols = cols;
        Ok(())
    }

    pub fn update(&mut self, timeout: Duration) {
        let deadline = Instant::now() + timeout;
        loop {
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                break;
            }
            match self
                .chunks
                .recv_timeout(remaining.min(Duration::from_millis(50)))
            {
                Ok(chunk) => self.feed(&chunk),
                Err(RecvTimeoutError::Timeout) => break,
                Err(RecvTimeoutError::Disconnected) => break,
            }
        }
    }

    pub fn wait_for_text(&mut self, text: &str, timeout: Duration) -> Result<()> {
        self.wait_until(timeout, |screen| screen.contains(text))
            .with_context(|| format!("wait for text {text:?}"))
    }

    pub fn wait_for_text_absent(&mut self, text: &str, timeout: Duration) -> Result<()> {
        self.wait_until(timeout, |screen| !screen.contains(text))
            .with_context(|| format!("wait for text {text:?} to disappear"))
    }

    pub fn screen(&self) -> String {
        self.parser.screen().contents()
    }

    pub fn raw_output(&self) -> &[u8] {
        &self.raw
    }

    pub fn raw_tail(&self) -> String {
        let start = self.raw.len().saturating_sub(RAW_TAIL_BYTES);
        String::from_utf8_lossy(&self.raw[start..]).into_owned()
    }

    pub fn snapshot(&self) -> ScreenSnapshot {
        ScreenSnapshot {
            rows: self.rows,
            cols: self.cols,
            screen: self.screen(),
            cursor_row: self.parser.screen().cursor_position().0,
            cursor_col: self.parser.screen().cursor_position().1,
            raw_bytes: self.raw.len(),
        }
    }

    pub fn is_alive(&mut self) -> bool {
        self.child.try_wait().ok().flatten().is_none()
    }

    pub fn wait(&mut self) -> Result<u32> {
        Ok(self.child.wait().context("wait for PTY child")?.exit_code())
    }

    pub fn kill(&mut self) -> Result<()> {
        self.child.kill().context("kill PTY child")
    }

    fn feed(&mut self, chunk: &[u8]) {
        self.raw.extend_from_slice(chunk);
        self.parser.process(chunk);
    }

    fn wait_until(&mut self, timeout: Duration, predicate: impl Fn(&str) -> bool) -> Result<()> {
        let deadline = Instant::now() + timeout;
        loop {
            if predicate(&self.screen()) {
                return Ok(());
            }
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                bail!(
                    "PTY wait timed out\n--- screen ---\n{}\n--- raw tail ---\n{}",
                    self.screen(),
                    self.raw_tail()
                );
            }
            match self
                .chunks
                .recv_timeout(remaining.min(Duration::from_millis(100)))
            {
                Ok(chunk) => self.feed(&chunk),
                Err(RecvTimeoutError::Timeout) => {}
                Err(RecvTimeoutError::Disconnected) if predicate(&self.screen()) => return Ok(()),
                Err(RecvTimeoutError::Disconnected) => bail!(
                    "PTY child output closed before condition matched\n--- screen ---\n{}",
                    self.screen()
                ),
            }
        }
    }
}

impl Drop for PtyHarness {
    fn drop(&mut self) {
        if self.is_alive() {
            let _ = self.child.kill();
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScreenSnapshot {
    pub rows: u16,
    pub cols: u16,
    pub screen: String,
    pub cursor_row: u16,
    pub cursor_col: u16,
    pub raw_bytes: usize,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Scenario {
    pub command: Vec<String>,
    #[serde(default)]
    pub cwd: Option<PathBuf>,
    #[serde(default)]
    pub env: BTreeMap<String, String>,
    #[serde(default = "default_rows")]
    pub rows: u16,
    #[serde(default = "default_cols")]
    pub cols: u16,
    pub steps: Vec<ScenarioStep>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case")]
pub enum ScenarioStep {
    WaitForText {
        text: String,
        #[serde(default = "default_timeout_ms")]
        timeout_ms: u64,
    },
    WaitForTextAbsent {
        text: String,
        #[serde(default = "default_timeout_ms")]
        timeout_ms: u64,
    },
    SendKeys {
        keys: String,
    },
    SendBytes {
        text: String,
    },
    Resize {
        rows: u16,
        cols: u16,
    },
    Snapshot {
        path: PathBuf,
    },
}

impl Scenario {
    pub fn run(&self) -> Result<ScreenSnapshot> {
        let mut harness = PtyHarness::spawn(&HarnessConfig {
            command: self.command.clone(),
            cwd: self.cwd.clone(),
            env: self.env.clone(),
            rows: self.rows,
            cols: self.cols,
        })?;
        for (index, step) in self.steps.iter().enumerate() {
            let result = match step {
                ScenarioStep::WaitForText { text, timeout_ms } => {
                    harness.wait_for_text(text, Duration::from_millis(*timeout_ms))
                }
                ScenarioStep::WaitForTextAbsent { text, timeout_ms } => {
                    harness.wait_for_text_absent(text, Duration::from_millis(*timeout_ms))
                }
                ScenarioStep::SendKeys { keys } => harness.send_keys(keys),
                ScenarioStep::SendBytes { text } => harness.send_bytes(text.as_bytes()),
                ScenarioStep::Resize { rows, cols } => harness.resize(*rows, *cols),
                ScenarioStep::Snapshot { path } => write_snapshot(path, &harness.snapshot()),
            };
            result.with_context(|| format!("scenario step {} failed: {step:?}", index + 1))?;
        }
        harness.update(Duration::from_millis(50));
        Ok(harness.snapshot())
    }
}

pub fn load_scenario(path: &Path) -> Result<Scenario> {
    serde_json::from_slice(&std::fs::read(path).context("read PTY scenario")?)
        .context("parse PTY scenario JSON")
}

pub fn write_snapshot(path: &Path, snapshot: &ScreenSnapshot) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).context("create snapshot directory")?;
    }
    std::fs::write(path, serde_json::to_vec_pretty(snapshot)?).context("write PTY snapshot")
}

pub fn parse_keys(notation: &str) -> Result<Vec<u8>> {
    let mut output = Vec::new();
    let mut rest = notation;
    while let Some(start) = rest.find('<') {
        output.extend_from_slice(&rest.as_bytes()[..start]);
        let Some(end) = rest[start..].find('>') else {
            output.extend_from_slice(&rest.as_bytes()[start..]);
            return Ok(output);
        };
        let token = &rest[start + 1..start + end];
        let bytes: &[u8] = match token.to_ascii_lowercase().as_str() {
            "cr" | "enter" => b"\r",
            "lf" => b"\n",
            "tab" => b"\t",
            "esc" => b"\x1b",
            "space" => b" ",
            "up" => b"\x1b[A",
            "down" => b"\x1b[B",
            "right" => b"\x1b[C",
            "left" => b"\x1b[D",
            "bs" | "backspace" => b"\x7f",
            "c-c" => b"\x03",
            "c-d" => b"\x04",
            "c-l" => b"\x0c",
            _ => bail!("unsupported key token <{token}>"),
        };
        output.extend_from_slice(bytes);
        rest = &rest[start + end + 1..];
    }
    output.extend_from_slice(rest.as_bytes());
    Ok(output)
}

fn default_rows() -> u16 {
    40
}

fn default_cols() -> u16 {
    120
}

fn default_timeout_ms() -> u64 {
    10_000
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_key_notation() {
        assert_eq!(parse_keys("hi<CR><C-c>").unwrap(), b"hi\r\x03");
        assert!(parse_keys("<unknown>").is_err());
    }

    #[test]
    fn readme_scenario_shape_parses() {
        let scenario: Scenario = serde_json::from_str(
            r#"{
                "command": ["target/debug/zode", "--no-sandbox"],
                "rows": 40,
                "cols": 120,
                "steps": [
                    {"action":"wait_for_text","text":"zode","timeout_ms":5000},
                    {"action":"send_keys","keys":"hello<Enter>"},
                    {"action":"resize","rows":50,"cols":140},
                    {"action":"snapshot","path":"target/pty/after-input.json"}
                ]
            }"#,
        )
        .unwrap();
        assert_eq!(scenario.steps.len(), 4);
    }

    #[cfg(unix)]
    #[test]
    fn drives_a_real_pty_and_virtual_screen() {
        let mut config = HarnessConfig::new("sh");
        config.command.extend([
            "-c".into(),
            "printf 'ready\\n'; read line; printf 'got:%s\\n' \"$line\"".into(),
        ]);
        let mut harness = PtyHarness::spawn(&config).unwrap();
        harness
            .wait_for_text("ready", Duration::from_secs(2))
            .unwrap();
        harness.send_keys("hello<CR>").unwrap();
        harness
            .wait_for_text("got:hello", Duration::from_secs(2))
            .unwrap();
        assert!(harness.snapshot().raw_bytes > 0);
    }
}
