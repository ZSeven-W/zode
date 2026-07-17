use std::{
    collections::HashMap,
    io::{Read, Write},
    path::Path,
    pin::Pin,
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Receiver, SyncSender, TrySendError},
        Arc, Mutex, MutexGuard,
    },
    thread::{self, JoinHandle},
    time::Duration,
};

use futures_core::Stream;
use portable_pty::{Child, ChildKiller, CommandBuilder, MasterPty, PtySize, PtySystem};
use zode_node_protocol::TerminalId;

const CONTROL_CAPACITY: usize = 16;
const OUTPUT_CAPACITY: usize = 64;
const READ_BUFFER_SIZE: usize = 8 * 1024;
const WRITE_CHUNK_SIZE: usize = 8 * 1024;
const IO_POLL_INTERVAL: Duration = Duration::from_millis(5);
#[cfg(unix)]
const SESSION_SWEEP_LIMIT: usize = 8;

pub type TerminalOutputStream = Pin<Box<dyn Stream<Item = Result<Vec<u8>, TerminalError>> + Send>>;

#[derive(Debug, thiserror::Error)]
pub enum TerminalError {
    #[error("terminal not found")]
    NotFound,
    #[error("terminal io: {0}")]
    Io(#[from] std::io::Error),
    #[error("terminal busy")]
    Busy,
    #[error("terminal platform: {0}")]
    Platform(String),
}

pub trait TerminalService: Send + Sync {
    fn spawn(&self, cwd: &Path) -> Result<TerminalId, TerminalError>;
    fn subscribe(&self, id: TerminalId) -> Result<TerminalOutputStream, TerminalError>;
    fn write(&self, id: TerminalId, bytes: Vec<u8>) -> Result<(), TerminalError>;
    fn resize(&self, id: TerminalId, cols: u16, rows: u16) -> Result<(), TerminalError>;
    fn close(&self, id: TerminalId) -> Result<(), TerminalError>;
}

enum Control {
    Resize(PtySize, SyncSender<Result<(), TerminalError>>),
    Size(SyncSender<Result<(u16, u16), TerminalError>>),
}

struct WriteRequest {
    bytes: Vec<u8>,
}

struct Session {
    control: SyncSender<Control>,
    input: SyncSender<WriteRequest>,
    output: Option<tokio::sync::mpsc::Receiver<Result<Vec<u8>, TerminalError>>>,
    shutdown: Arc<AtomicBool>,
    killer: Box<dyn ChildKiller + Send + Sync>,
    #[cfg(unix)]
    session_id: Option<i32>,
    control_thread: JoinHandle<Result<(), TerminalError>>,
    writer_thread: JoinHandle<Result<(), TerminalError>>,
    reader_thread: JoinHandle<Result<(), TerminalError>>,
}

pub struct LocalTerminalService {
    sessions: Mutex<HashMap<TerminalId, Session>>,
}

impl LocalTerminalService {
    pub fn new() -> Self {
        Self {
            sessions: Mutex::new(HashMap::new()),
        }
    }

    /// Reads the dimensions reported by the native PTY master. This is kept as
    /// an inherent diagnostic API rather than part of the portable service
    /// contract.
    pub fn size(&self, id: TerminalId) -> Result<(u16, u16), TerminalError> {
        let (reply_tx, reply_rx) = mpsc::sync_channel(1);
        self.try_control(id, Control::Size(reply_tx))?;
        receive_reply(reply_rx)
    }

    fn sessions(&self) -> MutexGuard<'_, HashMap<TerminalId, Session>> {
        self.sessions
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    fn try_control(&self, id: TerminalId, control: Control) -> Result<(), TerminalError> {
        let sessions = self.sessions();
        let session = sessions.get(&id).ok_or(TerminalError::NotFound)?;
        session
            .control
            .try_send(control)
            .map_err(|error| match error {
                TrySendError::Full(_) => TerminalError::Busy,
                TrySendError::Disconnected(_) => TerminalError::NotFound,
            })
    }

    fn close_session(&self, id: TerminalId) -> Result<(), TerminalError> {
        let mut session = self.sessions().remove(&id).ok_or(TerminalError::NotFound)?;
        session.shutdown.store(true, Ordering::Release);
        let terminate_result = terminate_process_tree(
            &mut *session.killer,
            #[cfg(unix)]
            session.session_id,
        );

        let Session {
            control,
            input,
            output,
            shutdown: _,
            killer: _,
            #[cfg(unix)]
                session_id: _,
            control_thread,
            writer_thread,
            reader_thread,
        } = session;
        drop(output);
        drop(control);
        drop(input);

        // The control thread owns the master. Joining it first drops the Unix
        // master fd or closes the ConPTY handle, which unblocks cloned reader
        // and writer handles before their threads are joined.
        let control_result = join_thread(control_thread, "terminal control");
        let writer_result = join_thread(writer_thread, "terminal writer");
        let reader_result = join_thread(reader_thread, "terminal reader");
        terminate_result
            .and(control_result)
            .and(writer_result)
            .and(reader_result)
    }
}

impl Default for LocalTerminalService {
    fn default() -> Self {
        Self::new()
    }
}

impl TerminalService for LocalTerminalService {
    fn spawn(&self, cwd: &Path) -> Result<TerminalId, TerminalError> {
        let pty_system = portable_pty::NativePtySystem::default();
        let pair = pty_system
            .openpty(PtySize::default())
            .map_err(platform_error)?;
        let mut command = CommandBuilder::new(user_shell());
        command.cwd(cwd.as_os_str());
        let mut child = pair.slave.spawn_command(command).map_err(platform_error)?;
        #[cfg(unix)]
        // portable-pty calls setsid(2) before exec, so the child PID is also
        // the session ID. Background jobs may create additional process
        // groups, but they remain members of this PTY session.
        let session_id = child.process_id().and_then(|pid| i32::try_from(pid).ok());
        if let Err(error) = configure_nonblocking(&*pair.master) {
            let _ = stop_child(&mut *child);
            return Err(error);
        }
        let mut killer = child.clone_killer();
        let reader = pair.master.try_clone_reader().map_err(platform_error)?;
        let writer = pair.master.take_writer().map_err(platform_error)?;
        drop(pair.slave);

        let (control_tx, control_rx) = mpsc::sync_channel(CONTROL_CAPACITY);
        let (input_tx, input_rx) = mpsc::sync_channel(CONTROL_CAPACITY);
        let (output_tx, output_rx) = tokio::sync::mpsc::channel(OUTPUT_CAPACITY);
        let shutdown = Arc::new(AtomicBool::new(false));
        let process_exited = Arc::new(AtomicBool::new(false));
        let control_shutdown = Arc::clone(&shutdown);
        let control_exited = Arc::clone(&process_exited);
        let control_thread = match thread::Builder::new()
            .name("zode-terminal-control".into())
            .spawn(move || {
                control_loop(
                    control_rx,
                    pair.master,
                    child,
                    control_shutdown,
                    control_exited,
                    #[cfg(unix)]
                    session_id,
                )
            }) {
            Ok(handle) => handle,
            Err(error) => {
                let _ = terminate_process_tree(
                    &mut *killer,
                    #[cfg(unix)]
                    session_id,
                );
                return Err(TerminalError::Io(error));
            }
        };
        let writer_shutdown = Arc::clone(&shutdown);
        let writer_exited = Arc::clone(&process_exited);
        let writer_output = output_tx.clone();
        let writer_thread = match thread::Builder::new()
            .name("zode-terminal-writer".into())
            .spawn(move || {
                writer_loop(
                    input_rx,
                    writer,
                    writer_output,
                    writer_shutdown,
                    writer_exited,
                )
            }) {
            Ok(handle) => handle,
            Err(error) => {
                shutdown.store(true, Ordering::Release);
                let _ = terminate_process_tree(
                    &mut *killer,
                    #[cfg(unix)]
                    session_id,
                );
                drop(control_tx);
                drop(input_tx);
                let _ = control_thread.join();
                return Err(TerminalError::Io(error));
            }
        };
        let reader_shutdown = Arc::clone(&shutdown);
        let reader_thread = match thread::Builder::new()
            .name("zode-terminal-reader".into())
            .spawn(move || reader_loop(reader, output_tx, reader_shutdown))
        {
            Ok(handle) => handle,
            Err(error) => {
                shutdown.store(true, Ordering::Release);
                let _ = terminate_process_tree(
                    &mut *killer,
                    #[cfg(unix)]
                    session_id,
                );
                drop(control_tx);
                drop(input_tx);
                let _ = control_thread.join();
                let _ = writer_thread.join();
                return Err(TerminalError::Io(error));
            }
        };

        let id = TerminalId::new();
        self.sessions().insert(
            id,
            Session {
                control: control_tx,
                input: input_tx,
                output: Some(output_rx),
                shutdown,
                killer,
                #[cfg(unix)]
                session_id,
                control_thread,
                writer_thread,
                reader_thread,
            },
        );
        Ok(id)
    }

    fn subscribe(&self, id: TerminalId) -> Result<TerminalOutputStream, TerminalError> {
        let receiver = self
            .sessions()
            .get_mut(&id)
            .ok_or(TerminalError::NotFound)?
            .output
            .take()
            .ok_or(TerminalError::Busy)?;
        let stream = futures_util::stream::unfold(receiver, |mut receiver| async move {
            receiver.recv().await.map(|item| (item, receiver))
        });
        Ok(Box::pin(stream))
    }

    fn write(&self, id: TerminalId, bytes: Vec<u8>) -> Result<(), TerminalError> {
        let sessions = self.sessions();
        let session = sessions.get(&id).ok_or(TerminalError::NotFound)?;
        session
            .input
            .try_send(WriteRequest { bytes })
            .map_err(|error| match error {
                TrySendError::Full(_) => TerminalError::Busy,
                TrySendError::Disconnected(_) => TerminalError::NotFound,
            })
    }

    fn resize(&self, id: TerminalId, cols: u16, rows: u16) -> Result<(), TerminalError> {
        let (reply_tx, reply_rx) = mpsc::sync_channel(1);
        self.try_control(
            id,
            Control::Resize(
                PtySize {
                    rows,
                    cols,
                    pixel_width: 0,
                    pixel_height: 0,
                },
                reply_tx,
            ),
        )?;
        receive_reply(reply_rx)
    }

    fn close(&self, id: TerminalId) -> Result<(), TerminalError> {
        self.close_session(id)
    }
}

impl Drop for LocalTerminalService {
    fn drop(&mut self) {
        let ids = self.sessions().keys().copied().collect::<Vec<_>>();
        for id in ids {
            let _ = self.close_session(id);
        }
    }
}

fn user_shell() -> std::ffi::OsString {
    #[cfg(windows)]
    let shell = std::env::var_os("COMSPEC").unwrap_or_else(|| "cmd.exe".into());
    #[cfg(not(windows))]
    let shell = std::env::var_os("SHELL").unwrap_or_else(|| "/bin/sh".into());
    shell
}

fn control_loop(
    receiver: Receiver<Control>,
    master: Box<dyn MasterPty + Send>,
    mut child: Box<dyn Child + Send + Sync>,
    shutdown: Arc<AtomicBool>,
    process_exited: Arc<AtomicBool>,
    #[cfg(unix)] session_id: Option<i32>,
) -> Result<(), TerminalError> {
    let result = (|| {
        #[cfg(unix)]
        let mut natural_exit = false;
        while !shutdown.load(Ordering::Acquire) {
            match receiver.recv_timeout(Duration::from_millis(25)) {
                Ok(Control::Resize(size, reply)) => {
                    let result = master.resize(size).map_err(platform_error);
                    let _ = reply.send(result);
                }
                Ok(Control::Size(reply)) => {
                    let result = master
                        .get_size()
                        .map(|size| (size.cols, size.rows))
                        .map_err(platform_error);
                    let _ = reply.send(result);
                }
                Err(mpsc::RecvTimeoutError::Timeout) => {}
                Err(mpsc::RecvTimeoutError::Disconnected) => break,
            }

            if child.try_wait().map_err(TerminalError::Io)?.is_some() {
                #[cfg(unix)]
                {
                    natural_exit = true;
                }
                break;
            }
        }
        // Release the control-owned master before waiting so pending PTY
        // output cannot keep macOS child reaping blocked on this handle.
        drop(master);
        #[cfg(unix)]
        if natural_exit {
            if let Some(session_id) = session_id {
                kill_process_session(session_id)?;
            }
        }
        stop_child(&mut *child).map_err(TerminalError::Io)
    })();
    process_exited.store(true, Ordering::Release);
    result
}

fn writer_loop(
    receiver: Receiver<WriteRequest>,
    mut writer: Box<dyn Write + Send>,
    output: tokio::sync::mpsc::Sender<Result<Vec<u8>, TerminalError>>,
    shutdown: Arc<AtomicBool>,
    process_exited: Arc<AtomicBool>,
) -> Result<(), TerminalError> {
    while !terminal_stopped(&shutdown, &process_exited) {
        match receiver.recv_timeout(Duration::from_millis(25)) {
            Ok(request) => {
                if let Err(error) =
                    write_all_cancellable(&mut *writer, &request.bytes, &shutdown, &process_exited)
                {
                    if terminal_stopped(&shutdown, &process_exited) {
                        break;
                    }
                    let returned = std::io::Error::new(error.kind(), error.to_string());
                    let _ = send_output(&output, Err(TerminalError::Io(error)), shutdown.as_ref());
                    return Err(TerminalError::Io(returned));
                }
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
        }
    }
    Ok(())
}

fn stop_child(child: &mut dyn Child) -> std::io::Result<()> {
    if child.try_wait()?.is_none() {
        if let Err(error) = child.kill() {
            #[cfg(unix)]
            if error.raw_os_error() != Some(nix::errno::Errno::ESRCH as i32) {
                return Err(error);
            }
            #[cfg(not(unix))]
            return Err(error);
        }
        let _ = child.wait()?;
    }
    Ok(())
}

fn reader_loop(
    mut reader: Box<dyn Read + Send>,
    output: tokio::sync::mpsc::Sender<Result<Vec<u8>, TerminalError>>,
    shutdown: Arc<AtomicBool>,
) -> Result<(), TerminalError> {
    let mut buffer = vec![0_u8; READ_BUFFER_SIZE];
    while !shutdown.load(Ordering::Acquire) {
        match reader.read(&mut buffer) {
            Ok(0) => break,
            Ok(read) => {
                if !send_output(&output, Ok(buffer[..read].to_vec()), shutdown.as_ref()) {
                    break;
                }
            }
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => {}
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                thread::sleep(IO_POLL_INTERVAL);
            }
            #[cfg(unix)]
            Err(error) if error.raw_os_error() == Some(nix::errno::Errno::EIO as i32) => break,
            Err(error) => {
                if shutdown.load(Ordering::Acquire) {
                    break;
                }
                let returned = std::io::Error::new(error.kind(), error.to_string());
                let _ = output.try_send(Err(TerminalError::Io(error)));
                return Err(TerminalError::Io(returned));
            }
        }
    }
    Ok(())
}

fn write_all_cancellable(
    writer: &mut dyn Write,
    bytes: &[u8],
    shutdown: &AtomicBool,
    process_exited: &AtomicBool,
) -> std::io::Result<()> {
    let mut written = 0;
    while written < bytes.len() {
        if terminal_stopped(shutdown, process_exited) {
            return Err(std::io::Error::new(
                std::io::ErrorKind::Interrupted,
                "terminal is closing",
            ));
        }
        let end = bytes.len().min(written + WRITE_CHUNK_SIZE);
        match writer.write(&bytes[written..end]) {
            Ok(0) => return Err(std::io::ErrorKind::WriteZero.into()),
            Ok(count) => written += count,
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => {}
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                thread::sleep(IO_POLL_INTERVAL);
            }
            Err(error) => return Err(error),
        }
    }
    Ok(())
}

fn terminal_stopped(shutdown: &AtomicBool, process_exited: &AtomicBool) -> bool {
    shutdown.load(Ordering::Acquire) || process_exited.load(Ordering::Acquire)
}

fn send_output(
    output: &tokio::sync::mpsc::Sender<Result<Vec<u8>, TerminalError>>,
    mut item: Result<Vec<u8>, TerminalError>,
    shutdown: &AtomicBool,
) -> bool {
    loop {
        if shutdown.load(Ordering::Acquire) {
            return false;
        }
        match output.try_send(item) {
            Ok(()) => return true,
            Err(tokio::sync::mpsc::error::TrySendError::Full(returned)) => {
                item = returned;
                thread::sleep(IO_POLL_INTERVAL);
            }
            Err(tokio::sync::mpsc::error::TrySendError::Closed(_)) => return false,
        }
    }
}

#[cfg(unix)]
fn configure_nonblocking(master: &dyn MasterPty) -> Result<(), TerminalError> {
    use nix::fcntl::{fcntl, FcntlArg, OFlag};

    let fd = master
        .as_raw_fd()
        .ok_or_else(|| TerminalError::Platform("native PTY has no file descriptor".into()))?;
    let current = fcntl(fd, FcntlArg::F_GETFL).map_err(platform_error)?;
    let flags = OFlag::from_bits_truncate(current) | OFlag::O_NONBLOCK;
    fcntl(fd, FcntlArg::F_SETFL(flags)).map_err(platform_error)?;
    Ok(())
}

#[cfg(not(unix))]
fn configure_nonblocking(_master: &dyn MasterPty) -> Result<(), TerminalError> {
    // ConPTY cancellation comes from dropping the master on the control thread;
    // portable-pty then closes the pseudoconsole and its pipe endpoints.
    Ok(())
}

fn terminate_process_tree(
    killer: &mut dyn ChildKiller,
    #[cfg(unix)] session_id: Option<i32>,
) -> Result<(), TerminalError> {
    #[cfg(unix)]
    if let Some(session_id) = session_id {
        let session_result = kill_process_session(session_id);
        let _ = killer.kill();
        return session_result;
    }
    #[cfg(windows)]
    {
        // portable-pty 0.9's cloned ConPTY killer can report an error after a
        // successful TerminateProcess call. The control thread owns the Child,
        // waits it out, and closes the pseudoconsole, so that lifecycle result
        // is authoritative.
        let _ = killer.kill();
        return Ok(());
    }
    #[cfg(not(windows))]
    killer.kill().map_err(TerminalError::Io)
}

#[cfg(unix)]
fn kill_process_session(session_id: i32) -> Result<(), TerminalError> {
    use nix::{
        errno::Errno,
        sys::signal::{kill, Signal},
        unistd::{getsid, Pid},
    };

    let session = Pid::from_raw(session_id);
    let mut first_error = None;
    for _ in 0..SESSION_SWEEP_LIMIT {
        let scan = scan_live_session_members(session)?;
        if first_error.is_none() {
            first_error = scan.error;
        }
        let mut members = scan.members;
        if members.is_empty() && first_error.is_none() {
            return Ok(());
        }
        // Stop workers before the shell/session leader, then rescan to catch
        // any child created after this process snapshot.
        members.sort_by_key(|pid| pid.as_raw() == session_id);
        for pid in members {
            match getsid(Some(pid)) {
                Ok(current) if current == session => {
                    if let Err(error) = kill(pid, Signal::SIGKILL) {
                        if error != Errno::ESRCH && first_error.is_none() {
                            first_error = Some(format!(
                                "failed to terminate process {} in PTY session {session_id}: {error}",
                                pid.as_raw()
                            ));
                        }
                    }
                }
                Ok(_) | Err(Errno::ESRCH) => {}
                Err(error) => {
                    if first_error.is_none() {
                        first_error = Some(format!(
                            "failed to revalidate process {} in PTY session {session_id}: {error}",
                            pid.as_raw()
                        ));
                    }
                }
            }
        }
        thread::sleep(IO_POLL_INTERVAL);
    }

    let final_scan = scan_live_session_members(session)?;
    if final_scan.members.is_empty() && final_scan.error.is_none() {
        return Ok(());
    }
    if first_error.is_none() {
        first_error = final_scan.error;
    }
    Err(TerminalError::Platform(format!(
        "PTY session {session_id} cleanup incomplete after {SESSION_SWEEP_LIMIT} sweeps; survivors: {}; first error: {}",
        final_scan
            .members
            .iter()
            .map(|pid| pid.as_raw().to_string())
            .collect::<Vec<_>>()
            .join(", "),
        first_error.as_deref().unwrap_or("none")
    )))
}

#[cfg(unix)]
struct SessionScan {
    members: Vec<nix::unistd::Pid>,
    error: Option<String>,
}

#[cfg(unix)]
fn scan_live_session_members(session: nix::unistd::Pid) -> Result<SessionScan, TerminalError> {
    use nix::{
        errno::Errno,
        unistd::{getsid, Pid},
    };

    let output = std::process::Command::new("/bin/ps")
        .args(["-x", "-o", "pid=", "-o", "state="])
        .output()?;
    if !output.status.success() {
        return Err(TerminalError::Platform(format!(
            "failed to inspect PTY session {}: {}",
            session.as_raw(),
            String::from_utf8_lossy(&output.stderr).trim()
        )));
    }

    let mut members = Vec::new();
    let mut first_error = None;
    for pid in String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter_map(parse_live_process)
        .map(Pid::from_raw)
    {
        match getsid(Some(pid)) {
            Ok(sid) if sid == session => members.push(pid),
            Ok(_) | Err(Errno::ESRCH) => {}
            Err(error) if first_error.is_none() => {
                first_error = Some(format!(
                    "failed to inspect process {} while scanning PTY session {}: {error}",
                    pid.as_raw(),
                    session.as_raw()
                ));
            }
            Err(_) => {}
        }
    }
    Ok(SessionScan {
        members,
        error: first_error,
    })
}

#[cfg(unix)]
fn parse_live_process(row: &str) -> Option<i32> {
    let mut fields = row.split_whitespace();
    let pid = fields.next()?.parse().ok()?;
    let state = fields.next()?;
    (!state.starts_with('Z')).then_some(pid)
}

fn receive_reply<T>(receiver: Receiver<Result<T, TerminalError>>) -> Result<T, TerminalError> {
    receiver.recv().map_err(|_| TerminalError::NotFound)?
}

fn join_thread(
    thread: JoinHandle<Result<(), TerminalError>>,
    name: &str,
) -> Result<(), TerminalError> {
    thread
        .join()
        .map_err(|_| TerminalError::Platform(format!("{name} thread panicked")))?
}

fn platform_error(error: impl std::fmt::Display) -> TerminalError {
    TerminalError::Platform(error.to_string())
}
