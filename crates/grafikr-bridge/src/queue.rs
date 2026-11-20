use std::collections::VecDeque;
use std::sync::{Condvar, Mutex};
use std::time::{Duration, Instant};

use crate::protocol::BridgeError;

pub const MAX_QUEUE_MESSAGES: usize = 1_024;
pub const MAX_QUEUE_BYTES: usize = 16 * 1024 * 1024;

struct QueueState {
    records: VecDeque<std::sync::Arc<[u8]>>,
    bytes: usize,
    stopped: bool,
    terminal: Option<std::sync::Arc<[u8]>>,
    reader_busy: bool,
}

pub struct BoundedQueue {
    state: Mutex<QueueState>,
    changed: Condvar,
}

pub struct QueueLease {
    queue: std::sync::Arc<BoundedQueue>,
    frame: std::sync::Arc<[u8]>,
    terminal: bool,
    consumed: bool,
}

impl QueueLease {
    pub fn bytes(&self) -> &[u8] {
        &self.frame
    }
    pub fn consume(mut self) -> Result<(), BridgeError> {
        let mut state = self
            .queue
            .state
            .lock()
            .map_err(|_| BridgeError::new("internal_failure", "queue lock poisoned"))?;
        if self.terminal {
            let terminal = state.terminal.take().ok_or_else(|| {
                BridgeError::new("internal_failure", "terminal frame changed during lease")
            })?;
            if !std::sync::Arc::ptr_eq(&terminal, &self.frame) {
                state.terminal = Some(terminal);
                return Err(BridgeError::new(
                    "internal_failure",
                    "terminal frame changed during lease",
                ));
            }
        } else {
            let front = state.records.front().ok_or_else(|| {
                BridgeError::new("internal_failure", "queue frame changed during lease")
            })?;
            if !std::sync::Arc::ptr_eq(front, &self.frame) {
                return Err(BridgeError::new(
                    "internal_failure",
                    "queue frame changed during lease",
                ));
            }
            let frame = state.records.pop_front().expect("front validated");
            state.bytes -= frame.len();
        }
        state.reader_busy = false;
        self.consumed = true;
        self.queue.changed.notify_all();
        Ok(())
    }
}

impl Drop for QueueLease {
    fn drop(&mut self) {
        if self.consumed {
            return;
        }
        if let Ok(mut state) = self.queue.state.lock() {
            state.reader_busy = false;
            self.queue.changed.notify_all();
        }
    }
}

impl Default for BoundedQueue {
    fn default() -> Self {
        Self::new()
    }
}

impl BoundedQueue {
    pub fn new() -> Self {
        Self {
            state: Mutex::new(QueueState {
                records: VecDeque::new(),
                bytes: 0,
                stopped: false,
                terminal: None,
                reader_busy: false,
            }),
            changed: Condvar::new(),
        }
    }
    pub fn push(&self, bytes: &[u8]) -> Result<(), BridgeError> {
        let mut state = self
            .state
            .lock()
            .map_err(|_| BridgeError::new("internal_failure", "queue lock poisoned"))?;
        if state.stopped {
            return Err(BridgeError::new("stopped", "session is stopping"));
        }
        if state.records.len() >= MAX_QUEUE_MESSAGES
            || state.bytes.saturating_add(bytes.len()) > MAX_QUEUE_BYTES
        {
            return Err(BridgeError::new(
                "queue_full",
                "queue message or byte quota reached",
            ));
        }
        state.bytes += bytes.len();
        state.records.push_back(std::sync::Arc::from(bytes));
        self.changed.notify_all();
        Ok(())
    }
    pub fn pop(&self, timeout: Duration) -> Result<Option<std::sync::Arc<[u8]>>, BridgeError> {
        let mut state = self
            .state
            .lock()
            .map_err(|_| BridgeError::new("internal_failure", "queue lock poisoned"))?;
        if state.reader_busy {
            return Err(BridgeError::new(
                "reader_busy",
                "only one queue reader is allowed",
            ));
        }
        state.reader_busy = true;
        let deadline = Instant::now() + timeout;
        loop {
            if let Some(frame) = state.records.pop_front() {
                state.bytes -= frame.len();
                state.reader_busy = false;
                return Ok(Some(frame));
            }
            if let Some(terminal) = state.terminal.take() {
                state.reader_busy = false;
                return Ok(Some(terminal));
            }
            if state.stopped {
                state.reader_busy = false;
                return Ok(None);
            }
            let now = Instant::now();
            if now >= deadline {
                state.reader_busy = false;
                return Ok(None);
            }
            let (next, timed) = self
                .changed
                .wait_timeout(state, deadline - now)
                .map_err(|_| BridgeError::new("internal_failure", "queue lock poisoned"))?;
            state = next;
            if timed.timed_out() && state.records.is_empty() {
                state.reader_busy = false;
                return Ok(None);
            }
        }
    }
    pub fn lease(
        self: &std::sync::Arc<Self>,
        timeout: Duration,
    ) -> Result<Option<QueueLease>, BridgeError> {
        let mut state = self
            .state
            .lock()
            .map_err(|_| BridgeError::new("internal_failure", "queue lock poisoned"))?;
        if state.reader_busy {
            return Err(BridgeError::new(
                "reader_busy",
                "only one queue reader is allowed",
            ));
        }
        state.reader_busy = true;
        let deadline = Instant::now() + timeout;
        loop {
            if let Some(frame) = state.records.front().cloned() {
                return Ok(Some(QueueLease {
                    queue: self.clone(),
                    frame,
                    terminal: false,
                    consumed: false,
                }));
            }
            if let Some(frame) = state.terminal.clone() {
                return Ok(Some(QueueLease {
                    queue: self.clone(),
                    frame,
                    terminal: true,
                    consumed: false,
                }));
            }
            if state.stopped {
                state.reader_busy = false;
                return Ok(None);
            }
            let now = Instant::now();
            if now >= deadline {
                state.reader_busy = false;
                return Ok(None);
            }
            let (next, timed) = self
                .changed
                .wait_timeout(state, deadline - now)
                .map_err(|_| BridgeError::new("internal_failure", "queue lock poisoned"))?;
            state = next;
            if timed.timed_out() && state.records.is_empty() && state.terminal.is_none() {
                state.reader_busy = false;
                return Ok(None);
            }
        }
    }
    pub fn stop(&self) {
        if let Ok(mut state) = self.state.lock() {
            state.stopped = true;
            self.changed.notify_all();
        }
    }
    pub fn fail(&self, frame: &[u8]) {
        if let Ok(mut state) = self.state.lock() {
            state.stopped = true;
            state.terminal = Some(std::sync::Arc::from(frame));
            self.changed.notify_all();
        }
    }
    pub fn is_stopped(&self) -> bool {
        self.state.lock().map(|s| s.stopped).unwrap_or(true)
    }
}
