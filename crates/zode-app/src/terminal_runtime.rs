use std::{
    collections::VecDeque,
    path::Path,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex, MutexGuard,
    },
    thread::{self, JoinHandle},
};

use futures_util::StreamExt;
use zode_node_protocol::TerminalId;

use crate::services::{TerminalError, TerminalService};

const OUTPUT_ITEM_LIMIT: usize = 128;
const OUTPUT_BYTE_LIMIT: usize = 1024 * 1024;

struct OutputBuffer {
    items: VecDeque<Result<Vec<u8>, TerminalError>>,
    bytes: usize,
}

impl OutputBuffer {
    fn new() -> Self {
        Self {
            items: VecDeque::new(),
            bytes: 0,
        }
    }

    fn push(&mut self, item: Result<Vec<u8>, TerminalError>) {
        let item = match item {
            Ok(bytes) if bytes.len() > OUTPUT_BYTE_LIMIT => {
                Ok(bytes[bytes.len() - OUTPUT_BYTE_LIMIT..].to_vec())
            }
            item => item,
        };
        let incoming = item.as_ref().map_or(0, Vec::len);
        while self.items.len() >= OUTPUT_ITEM_LIMIT
            || self.bytes.saturating_add(incoming) > OUTPUT_BYTE_LIMIT
        {
            let Some(removed) = self.items.pop_front() else {
                break;
            };
            self.bytes = self
                .bytes
                .saturating_sub(removed.as_ref().map_or(0, Vec::len));
        }
        self.bytes = self.bytes.saturating_add(incoming);
        self.items.push_back(item);
    }

    fn drain(&mut self) -> Vec<Result<Vec<u8>, TerminalError>> {
        self.bytes = 0;
        self.items.drain(..).collect()
    }
}

struct RuntimeSession {
    id: TerminalId,
    finished: Arc<AtomicBool>,
    output_thread: JoinHandle<()>,
}

/// Owns terminal service effects and bridges streamed output onto UI wakes.
pub struct TerminalRuntime {
    service: Arc<dyn TerminalService>,
    output: Arc<Mutex<OutputBuffer>>,
    wake: Arc<dyn Fn() + Send + Sync>,
    session: Option<RuntimeSession>,
}

impl TerminalRuntime {
    pub fn new<F>(service: Arc<dyn TerminalService>, wake: F) -> Self
    where
        F: Fn() + Send + Sync + 'static,
    {
        Self {
            service,
            output: Arc::new(Mutex::new(OutputBuffer::new())),
            wake: Arc::new(wake),
            session: None,
        }
    }

    pub fn active_id(&self) -> Option<TerminalId> {
        self.session.as_ref().map(|session| session.id)
    }

    pub fn open(&mut self, cwd: &Path, cols: u16, rows: u16) -> Result<TerminalId, TerminalError> {
        if self.reap_finished()?.is_some() {
            output_buffer(&self.output).drain();
        }
        if self.session.is_some() {
            return Err(TerminalError::Busy);
        }
        let id = self.service.spawn(cwd)?;
        if let Err(error) = self.service.resize(id, cols, rows) {
            let _ = self.service.close(id);
            return Err(error);
        }
        let mut stream = match self.service.subscribe(id) {
            Ok(stream) => stream,
            Err(error) => {
                let _ = self.service.close(id);
                return Err(error);
            }
        };
        let output = Arc::clone(&self.output);
        let wake = Arc::clone(&self.wake);
        let finished = Arc::new(AtomicBool::new(false));
        let thread_finished = Arc::clone(&finished);
        let output_thread = match thread::Builder::new()
            .name("zode-terminal-output".into())
            .spawn(move || {
                let runtime = tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build();
                match runtime {
                    Ok(runtime) => runtime.block_on(async {
                        while let Some(item) = stream.next().await {
                            output_buffer(&output).push(item);
                            wake();
                        }
                    }),
                    Err(error) => {
                        output_buffer(&output).push(Err(TerminalError::Io(error)));
                        wake();
                    }
                }
                thread_finished.store(true, Ordering::Release);
                wake();
            }) {
            Ok(thread) => thread,
            Err(error) => {
                let _ = self.service.close(id);
                return Err(TerminalError::Io(error));
            }
        };
        self.session = Some(RuntimeSession {
            id,
            finished,
            output_thread,
        });
        Ok(id)
    }

    pub fn drain_output(&self) -> Vec<Result<Vec<u8>, TerminalError>> {
        output_buffer(&self.output).drain()
    }

    pub fn write(&self, id: TerminalId, bytes: Vec<u8>) -> Result<(), TerminalError> {
        self.ensure_active(id)?;
        self.service.write(id, bytes)
    }

    pub fn resize(&self, id: TerminalId, cols: u16, rows: u16) -> Result<(), TerminalError> {
        self.ensure_active(id)?;
        self.service.resize(id, cols, rows)
    }

    pub fn close(&mut self, id: TerminalId) -> Result<(), TerminalError> {
        self.ensure_active(id)?;
        let session = self.session.take().ok_or(TerminalError::NotFound)?;
        let close_result = self.service.close(id);
        let join_result = session
            .output_thread
            .join()
            .map_err(|_| TerminalError::Platform("terminal output thread panicked".into()));
        close_result?;
        join_result
    }

    pub fn reap_finished(&mut self) -> Result<Option<TerminalId>, TerminalError> {
        let Some(session) = self.session.as_ref() else {
            return Ok(None);
        };
        if !session.finished.load(Ordering::Acquire) {
            return Ok(None);
        }
        let id = session.id;
        self.close(id)?;
        Ok(Some(id))
    }

    fn ensure_active(&self, id: TerminalId) -> Result<(), TerminalError> {
        if self.active_id() == Some(id) {
            Ok(())
        } else {
            Err(TerminalError::NotFound)
        }
    }
}

impl Drop for TerminalRuntime {
    fn drop(&mut self) {
        if let Some(id) = self.active_id() {
            let _ = self.close(id);
        }
    }
}

fn output_buffer(buffer: &Mutex<OutputBuffer>) -> MutexGuard<'_, OutputBuffer> {
    buffer
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}
