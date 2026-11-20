use grafikr_shell::{AppDriver, ShellCx, ShellWindowId, WakeHandle};
use serde_json::json;
use std::time::Instant;

use crate::backend::{ShellBackend, WindowKey};
use crate::dispatch::Dispatcher;
use crate::protocol::{BridgeError, Outbound, WireError, encode};
use crate::session::{Lifecycle, Session, SessionControl, SessionSeed};

pub struct BridgeDriver {
    control: SessionControl,
    session: Session,
    dispatcher: Dispatcher,
    handshaken: bool,
    last_transition_tick: Instant,
}

impl BridgeDriver {
    pub fn new() -> Self {
        let (control, seed) = Session::channel();
        Self::from_channel(control, seed)
    }
    pub fn from_channel(control: SessionControl, seed: SessionSeed) -> Self {
        let mut dispatcher = Dispatcher::new();
        dispatcher.set_publisher(control.clone());
        Self {
            control,
            session: Session::new(seed),
            dispatcher,
            handshaken: false,
            last_transition_tick: Instant::now(),
        }
    }
    pub fn control(&self) -> SessionControl {
        self.control.clone()
    }
}

impl Default for BridgeDriver {
    fn default() -> Self {
        Self::new()
    }
}

impl AppDriver for BridgeDriver {
    fn ready(&mut self, wake: WakeHandle) {
        let _ = self.control.install_waker(wake.clone());
        if self.control.lifecycle() == Lifecycle::Created && self.session.start().is_err() {
            self.control.request_stop();
        }
        if self.control.lifecycle() == Lifecycle::Stopping {
            wake();
        }
    }

    fn commands(&mut self, cx: &mut ShellCx<'_>) {
        if matches!(
            self.control.lifecycle(),
            Lifecycle::Stopped | Lifecycle::Failed
        ) {
            cx.stop();
            return;
        }
        let requests = match self.session.drain(64) {
            Ok(requests) => requests,
            Err(_) => {
                self.control.request_stop();
                cx.stop();
                return;
            }
        };
        let batch_full = requests.len() == 64;
        let stop = {
            let mut backend = ShellBackend::new(cx);
            let now = Instant::now();
            let dt = now.duration_since(self.last_transition_tick).as_secs_f32();
            self.last_transition_tick = now;
            self.dispatcher.tick_transitions(dt, &mut backend);
            self.dispatcher.drain_intents(&mut backend);
            let mut stop = false;
            for request in requests {
                if !self.handshaken && request.op != crate::protocol::Operation::Hello {
                    let response = protocol_error(
                        request.id,
                        BridgeError::new("hello_required", "hello must be the first request"),
                    );
                    if let Ok(frame) = encode(&response) {
                        let _ = self.control.publish(&frame);
                    }
                    continue;
                }
                let hello = request.op == crate::protocol::Operation::Hello;
                let response = self.dispatcher.execute(request, &mut backend);
                let version_mismatch = matches!(
                    &response,
                    Outbound::Response {
                        ok: false,
                        error: Some(error),
                        ..
                    } if error.code == "version_mismatch"
                );
                if hello && matches!(&response, Outbound::Response { ok: true, .. }) {
                    self.handshaken = true;
                }
                if let Ok(frame) = encode(&response) {
                    let _ = self.control.publish(&frame);
                }
                if self.dispatcher.is_stopped() || version_mismatch {
                    self.control.request_stop();
                    stop = true;
                    break;
                }
            }
            stop
        };
        let eof_drained = self.control.lifecycle() == Lifecycle::Stopping && !batch_full;
        if stop || eof_drained {
            cx.stop();
        }
    }

    fn closed(&mut self, id: ShellWindowId) {
        if let Some(window) = self.dispatcher.window_closed(WindowKey(id)) {
            self.dispatcher
                .emit_event("window.closed", window, json!({}));
        }
    }

    fn stopping(&mut self) {
        self.session.finish();
    }

    fn is_animating(&self) -> bool {
        self.dispatcher.is_animating()
    }
}

fn protocol_error(id: String, error: BridgeError) -> Outbound {
    Outbound::Response {
        id,
        ok: false,
        result: None,
        error: Some(WireError {
            code: error.code.to_owned(),
            message: error.message,
            field: error.field,
            handle: error.handle,
        }),
    }
}
