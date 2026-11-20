use std::collections::HashSet;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use crate::protocol::{BridgeError, Request, decode};
use crate::queue::{BoundedQueue, QueueLease};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Lifecycle {
    Created,
    Running,
    Stopping,
    Stopped,
    Failed,
}

struct Shared {
    commands: Arc<BoundedQueue>,
    output: Arc<BoundedQueue>,
    lifecycle: Mutex<Lifecycle>,
    waker: Mutex<Option<Arc<dyn Fn() + Send + Sync>>>,
    active_requests: Mutex<HashSet<String>>,
}

#[derive(Clone)]
pub struct SessionControl(Arc<Shared>);

pub struct SessionSeed(Arc<Shared>);

pub struct Session {
    shared: Arc<Shared>,
    next_sequence: u64,
}

impl Session {
    pub fn channel() -> (SessionControl, SessionSeed) {
        let shared = Arc::new(Shared {
            commands: Arc::new(BoundedQueue::new()),
            output: Arc::new(BoundedQueue::new()),
            lifecycle: Mutex::new(Lifecycle::Created),
            waker: Mutex::new(None),
            active_requests: Mutex::new(HashSet::new()),
        });
        (SessionControl(shared.clone()), SessionSeed(shared))
    }
    pub fn new(seed: SessionSeed) -> Self {
        Self {
            shared: seed.0,
            next_sequence: 1,
        }
    }
    pub fn start(&mut self) -> Result<(), BridgeError> {
        let mut state = self
            .shared
            .lifecycle
            .lock()
            .map_err(|_| BridgeError::new("internal_failure", "lifecycle lock poisoned"))?;
        if *state != Lifecycle::Created {
            return Err(BridgeError::new(
                "wrong_lifecycle",
                "session can run only once",
            ));
        }
        *state = Lifecycle::Running;
        Ok(())
    }
    pub fn drain(&mut self, limit: usize) -> Result<Vec<Request>, BridgeError> {
        let mut requests = Vec::new();
        let capacity = limit.min(64);
        for _ in 0..capacity {
            let Some(frame) = self.shared.commands.pop(Duration::ZERO)? else {
                break;
            };
            requests.push(decode(&frame)?);
        }
        if requests.len() == capacity
            && let Ok(waker) = self.shared.waker.lock()
            && let Some(waker) = &*waker
        {
            waker();
        }
        Ok(requests)
    }
    pub fn next_sequence(&mut self) -> u64 {
        let value = self.next_sequence;
        self.next_sequence = self.next_sequence.saturating_add(1);
        value
    }
    pub fn finish(&self) {
        if let Ok(mut lifecycle) = self.shared.lifecycle.lock() {
            *lifecycle = Lifecycle::Stopped;
        }
        self.shared.commands.stop();
        self.shared.output.stop();
        if let Ok(mut active) = self.shared.active_requests.lock() {
            active.clear();
        }
    }
}

impl SessionControl {
    pub fn submit(&self, frame: &[u8]) -> Result<(), BridgeError> {
        let request = decode(frame)?;
        {
            let mut active = self.0.active_requests.lock().map_err(|_| {
                BridgeError::new("internal_failure", "request registry lock poisoned")
            })?;
            if !active.insert(request.id.clone()) {
                return Err(
                    BridgeError::new("duplicate_request", "request id is already active")
                        .field("id"),
                );
            }
        }
        if let Err(error) = self.0.commands.push(frame) {
            if let Ok(mut active) = self.0.active_requests.lock() {
                active.remove(&request.id);
            }
            return Err(error);
        }
        if let Ok(waker) = self.0.waker.lock()
            && let Some(waker) = &*waker
        {
            waker();
        }
        Ok(())
    }
    pub fn request_stop(&self) {
        if let Ok(mut lifecycle) = self.0.lifecycle.lock()
            && matches!(*lifecycle, Lifecycle::Created | Lifecycle::Running)
        {
            *lifecycle = Lifecycle::Stopping;
        }
        self.0.commands.stop();
        if let Ok(waker) = self.0.waker.lock()
            && let Some(waker) = &*waker
        {
            waker();
        }
    }
    pub fn next(&self, timeout: Duration) -> Result<Option<Arc<[u8]>>, BridgeError> {
        self.0.output.pop(timeout)
    }
    pub fn next_lease(&self, timeout: Duration) -> Result<Option<QueueLease>, BridgeError> {
        self.0.output.lease(timeout)
    }
    pub fn publish(&self, frame: &[u8]) -> Result<(), BridgeError> {
        if let Ok(value) = serde_json::from_slice::<serde_json::Value>(frame)
            && value.get("kind").and_then(serde_json::Value::as_str) == Some("response")
            && let Some(id) = value.get("id").and_then(serde_json::Value::as_str)
            && let Ok(mut active) = self.0.active_requests.lock()
        {
            active.remove(id);
        }
        match self.0.output.push(frame) {
            Ok(()) => Ok(()),
            Err(error) if error.code == "queue_full" => {
                if let Ok(mut lifecycle) = self.0.lifecycle.lock() {
                    *lifecycle = Lifecycle::Failed;
                }
                if let Ok(mut active) = self.0.active_requests.lock() {
                    active.clear();
                }
                self.0.output.fail(b"{\"kind\":\"event\",\"sequence\":\"0\",\"event\":\"session.error\",\"source\":\"0\",\"payload\":{\"code\":\"output_overflow\"}}\n");
                Err(BridgeError::new(
                    "output_overflow",
                    "output queue quota reached; session failed",
                ))
            }
            Err(error) => Err(error),
        }
    }
    pub fn install_waker(&self, waker: Arc<dyn Fn() + Send + Sync>) -> Result<(), BridgeError> {
        *self
            .0
            .waker
            .lock()
            .map_err(|_| BridgeError::new("internal_failure", "waker lock poisoned"))? =
            Some(waker.clone());
        if !self.0.commands.is_stopped() {
            waker();
        }
        Ok(())
    }
    pub fn lifecycle(&self) -> Lifecycle {
        self.0
            .lifecycle
            .lock()
            .map(|v| *v)
            .unwrap_or(Lifecycle::Failed)
    }
    pub fn finish(&self) {
        if let Ok(mut lifecycle) = self.0.lifecycle.lock() {
            *lifecycle = Lifecycle::Stopped;
        }
        self.0.commands.stop();
        self.0.output.stop();
        if let Ok(mut active) = self.0.active_requests.lock() {
            active.clear();
        }
    }
}
