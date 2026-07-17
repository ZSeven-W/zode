use std::{
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

use zode_app::{
    services::{TerminalError, TerminalOutputStream, TerminalService},
    terminal_runtime::TerminalRuntime,
};
use zode_node_protocol::TerminalId;

#[derive(Default)]
struct FakeState {
    terminal: Option<TerminalId>,
    cwd: Option<PathBuf>,
    writes: Vec<Vec<u8>>,
    resizes: Vec<(u16, u16)>,
    closes: usize,
}

#[derive(Default)]
struct FakeTerminalService {
    state: Mutex<FakeState>,
}

impl TerminalService for FakeTerminalService {
    fn spawn(&self, cwd: &Path) -> Result<TerminalId, TerminalError> {
        let id = TerminalId::new();
        let mut state = self.state.lock().unwrap();
        state.terminal = Some(id);
        state.cwd = Some(cwd.to_path_buf());
        Ok(id)
    }

    fn subscribe(&self, id: TerminalId) -> Result<TerminalOutputStream, TerminalError> {
        if self.state.lock().unwrap().terminal != Some(id) {
            return Err(TerminalError::NotFound);
        }
        Ok(Box::pin(futures_util::stream::iter(vec![Ok(
            b"runtime-ready".to_vec(),
        )])))
    }

    fn write(&self, id: TerminalId, bytes: Vec<u8>) -> Result<(), TerminalError> {
        let mut state = self.state.lock().unwrap();
        if state.terminal != Some(id) {
            return Err(TerminalError::NotFound);
        }
        state.writes.push(bytes);
        Ok(())
    }

    fn resize(&self, id: TerminalId, cols: u16, rows: u16) -> Result<(), TerminalError> {
        let mut state = self.state.lock().unwrap();
        if state.terminal != Some(id) {
            return Err(TerminalError::NotFound);
        }
        state.resizes.push((cols, rows));
        Ok(())
    }

    fn close(&self, id: TerminalId) -> Result<(), TerminalError> {
        let mut state = self.state.lock().unwrap();
        if state.terminal.take() != Some(id) {
            return Err(TerminalError::NotFound);
        }
        state.closes += 1;
        Ok(())
    }
}

#[test]
fn terminal_runtime_executes_service_effects_and_bridges_output() {
    let service = Arc::new(FakeTerminalService::default());
    let wakes = Arc::new(Mutex::new(0_usize));
    let wake_count = Arc::clone(&wakes);
    let mut runtime = TerminalRuntime::new(service.clone(), move || {
        *wake_count.lock().unwrap() += 1;
    });
    let cwd = std::env::current_dir().unwrap();

    let terminal = runtime.open(&cwd, 80, 24).unwrap();
    runtime.write(terminal, b"echo ready\r".to_vec()).unwrap();
    runtime.resize(terminal, 120, 40).unwrap();

    let deadline = Instant::now() + Duration::from_secs(2);
    let output = loop {
        let output = runtime.drain_output();
        if !output.is_empty() {
            break output;
        }
        assert!(
            Instant::now() < deadline,
            "terminal output bridge timed out"
        );
        std::thread::sleep(Duration::from_millis(10));
    };
    assert_eq!(output.len(), 1);
    assert_eq!(output[0].as_ref().unwrap(), b"runtime-ready");

    runtime.close(terminal).unwrap();
    let state = service.state.lock().unwrap();
    assert_eq!(state.cwd.as_deref(), Some(cwd.as_path()));
    assert_eq!(state.writes, vec![b"echo ready\r".to_vec()]);
    assert_eq!(state.resizes, vec![(80, 24), (120, 40)]);
    assert_eq!(state.closes, 1);
    assert!(*wakes.lock().unwrap() >= 1);
}

#[test]
fn terminal_runtime_reaps_stream_eof_before_reopening() {
    let service = Arc::new(FakeTerminalService::default());
    let mut runtime = TerminalRuntime::new(service.clone(), || {});
    let cwd = std::env::current_dir().unwrap();
    let first = runtime.open(&cwd, 80, 24).unwrap();
    let deadline = Instant::now() + Duration::from_secs(2);

    let second = loop {
        match runtime.open(&cwd, 80, 24) {
            Ok(id) => break id,
            Err(TerminalError::Busy) if Instant::now() < deadline => {
                std::thread::sleep(Duration::from_millis(10));
            }
            Err(error) => panic!("terminal did not reopen after stream EOF: {error}"),
        }
    };

    assert_ne!(first, second);
    runtime.close(second).unwrap();
    assert_eq!(service.state.lock().unwrap().closes, 2);
}

#[test]
fn terminal_output_and_eof_share_one_wake_then_rearm_after_drain() {
    let service = Arc::new(FakeTerminalService::default());
    let wakes = Arc::new(Mutex::new(0_usize));
    let wake_count = Arc::clone(&wakes);
    let mut runtime = TerminalRuntime::new(service, move || {
        *wake_count.lock().unwrap() += 1;
    });
    let cwd = std::env::current_dir().unwrap();

    for expected_wakes in 1..=2 {
        runtime.open(&cwd, 80, 24).unwrap();
        let deadline = Instant::now() + Duration::from_secs(2);
        loop {
            if runtime.reap_finished().unwrap().is_some() {
                break;
            }
            assert!(Instant::now() < deadline, "terminal stream did not finish");
            std::thread::sleep(Duration::from_millis(10));
        }

        assert_eq!(
            *wakes.lock().unwrap(),
            expected_wakes,
            "output chunks and EOF must remain coalesced until the UI drain"
        );
        let output = runtime.drain_output();
        assert_eq!(output.len(), 1);
        assert_eq!(output[0].as_ref().unwrap(), b"runtime-ready");
    }
}
