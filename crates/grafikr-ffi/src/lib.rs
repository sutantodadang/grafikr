use std::collections::HashMap;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, OnceLock};
use std::thread::ThreadId;
use std::time::Duration;

use grafikr_bridge::driver::BridgeDriver;
use grafikr_bridge::protocol::BridgeError;
use grafikr_bridge::session::{Lifecycle, Session, SessionControl, SessionSeed};
use grafikr_core::View;
use grafikr_shell::App;

pub const OK: i32 = 0;
pub const INVALID_ARGUMENT: i32 = 1;
pub const INVALID_SESSION: i32 = 2;
pub const WRONG_THREAD: i32 = 3;
pub const WRONG_LIFECYCLE: i32 = 4;
pub const QUEUE_FULL: i32 = 5;
pub const BUFFER_TOO_SMALL: i32 = 6;
pub const TIMEOUT: i32 = 7;
pub const STOPPED: i32 = 8;
pub const INTERNAL_FAILURE: i32 = 9;
pub const READER_BUSY: i32 = 10;

struct FfiSession {
    control: SessionControl,
    seed: Option<SessionSeed>,
    creator: ThreadId,
    ui_thread: Option<ThreadId>,
    running: bool,
}

impl FfiSession {
    fn finish_run(&mut self) {
        self.running = false;
        self.ui_thread = None;
    }
}

static NEXT_ID: AtomicU64 = AtomicU64::new(1);
static REGISTRY: OnceLock<Mutex<HashMap<u64, FfiSession>>> = OnceLock::new();
static PROCESS_RUNNING: OnceLock<Mutex<bool>> = OnceLock::new();

fn registry() -> &'static Mutex<HashMap<u64, FfiSession>> {
    REGISTRY.get_or_init(|| Mutex::new(HashMap::new()))
}

fn status(error: &BridgeError) -> i32 {
    match error.code {
        "queue_full" | "output_overflow" => QUEUE_FULL,
        "reader_busy" => READER_BUSY,
        "stopped" => STOPPED,
        "wrong_lifecycle" => WRONG_LIFECYCLE,
        _ => INTERNAL_FAILURE,
    }
}

fn guarded(f: impl FnOnce() -> i32) -> i32 {
    catch_unwind(AssertUnwindSafe(f)).unwrap_or(INTERNAL_FAILURE)
}

fn with_session(id: u64, f: impl FnOnce(&mut FfiSession) -> i32) -> i32 {
    let mut sessions = match registry().lock() {
        Ok(sessions) => sessions,
        Err(_) => return INTERNAL_FAILURE,
    };
    match sessions.get_mut(&id) {
        Some(session) => f(session),
        None => INVALID_SESSION,
    }
}

fn complete_run(session: u64, outcome: i32) -> i32 {
    with_session(session, |entry| {
        entry.finish_run();
        outcome
    })
}

struct EmptyView;

impl View for EmptyView {
    fn mount(&self, cx: &mut grafikr_core::MountCx) -> grafikr_core::NodeId {
        cx.add_node(grafikr_core::Style::default())
    }
}

/// Returns the ABI version as `(major << 16) | minor`.
#[unsafe(no_mangle)]
pub extern "C" fn grafikr_abi_version() -> u32 {
    0x0001_0000
}

/// Creates a session and writes its opaque token to `session`.
///
/// # Safety
/// `session` must point to writable storage for one `uint64_t`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn grafikr_session_create(session: *mut u64) -> i32 {
    guarded(|| {
        if session.is_null() {
            return INVALID_ARGUMENT;
        }
        let Ok(id) = NEXT_ID.fetch_update(Ordering::Relaxed, Ordering::Relaxed, |next| {
            next.checked_add(1)
        }) else {
            return INTERNAL_FAILURE;
        };
        let (control, seed) = Session::channel();
        let entry = FfiSession {
            control,
            seed: Some(seed),
            creator: std::thread::current().id(),
            ui_thread: None,
            running: false,
        };
        if registry()
            .lock()
            .map(|mut sessions| sessions.insert(id, entry))
            .is_err()
        {
            return INTERNAL_FAILURE;
        }
        // SAFETY: caller promises a writable u64 pointer, checked for null above.
        unsafe { session.write(id) };
        OK
    })
}

/// Copies and queues one complete JSON request frame.
///
/// # Safety
/// For nonzero `length`, `data` must point to `length` readable bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn grafikr_submit(session: u64, data: *const u8, length: usize) -> i32 {
    guarded(|| {
        if data.is_null() || length == 0 || length > grafikr_bridge::protocol::MAX_FRAME_BYTES {
            return INVALID_ARGUMENT;
        }
        // SAFETY: caller promises a readable buffer of `length` bytes.
        let frame = unsafe { std::slice::from_raw_parts(data, length) }.to_vec();
        with_session(session, |entry| match entry.control.submit(&frame) {
            Ok(()) => OK,
            Err(error) => status(&error),
        })
    })
}

/// Runs a session on the thread that created it. This call blocks until stop.
#[unsafe(no_mangle)]
pub extern "C" fn grafikr_run(session: u64) -> i32 {
    guarded(|| {
        let (control, seed) = {
            let mut sessions = match registry().lock() {
                Ok(sessions) => sessions,
                Err(_) => return INTERNAL_FAILURE,
            };
            let Some(entry) = sessions.get_mut(&session) else {
                return INVALID_SESSION;
            };
            if entry.creator != std::thread::current().id() {
                return WRONG_THREAD;
            }
            if entry.running || entry.seed.is_none() {
                return WRONG_LIFECYCLE;
            }
            if entry.control.lifecycle() == Lifecycle::Stopping {
                entry.seed.take();
                entry.control.finish();
                return OK;
            }
            if entry.control.lifecycle() != Lifecycle::Created {
                return WRONG_LIFECYCLE;
            }
            let process = PROCESS_RUNNING.get_or_init(|| Mutex::new(false));
            let Ok(mut active) = process.lock() else {
                return INTERNAL_FAILURE;
            };
            if *active {
                return WRONG_LIFECYCLE;
            }
            *active = true;
            entry.running = true;
            entry.ui_thread = Some(std::thread::current().id());
            (
                entry.control.clone(),
                entry.seed.take().expect("seed validated"),
            )
        };

        let result = catch_unwind(AssertUnwindSafe(|| {
            App::new(|| EmptyView).run_controlled(BridgeDriver::from_channel(control.clone(), seed))
        }));
        control.finish();
        if let Some(process) = PROCESS_RUNNING.get()
            && let Ok(mut active) = process.lock()
        {
            *active = false;
        }
        let outcome = if matches!(result, Ok(Ok(()))) {
            OK
        } else {
            INTERNAL_FAILURE
        };
        complete_run(session, outcome)
    })
}

/// Waits for an event. A short buffer leaves the event queued.
///
/// # Safety
/// `required` must be writable. If capacity is nonzero, `data` must be writable
/// for `capacity` bytes. `data == NULL` is valid only when capacity is zero.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn grafikr_next_event(
    session: u64,
    data: *mut u8,
    capacity: usize,
    required: *mut usize,
    timeout_ms: u32,
) -> i32 {
    guarded(|| {
        if required.is_null() || (capacity > 0 && data.is_null()) {
            return INVALID_ARGUMENT;
        }
        let (control, is_ui_thread) = {
            let sessions = match registry().lock() {
                Ok(sessions) => sessions,
                Err(_) => return INTERNAL_FAILURE,
            };
            let Some(entry) = sessions.get(&session) else {
                return INVALID_SESSION;
            };
            (
                entry.control.clone(),
                entry.running && entry.ui_thread == Some(std::thread::current().id()),
            )
        };
        if is_ui_thread && timeout_ms > 0 {
            return WRONG_THREAD;
        }
        let lease = match control.next_lease(Duration::from_millis(u64::from(timeout_ms))) {
            Ok(Some(lease)) => lease,
            Ok(None) => {
                return if control.lifecycle() == Lifecycle::Stopped {
                    STOPPED
                } else {
                    TIMEOUT
                };
            }
            Err(error) => return status(&error),
        };
        let bytes = lease.bytes();
        // SAFETY: caller promises writable `required` storage.
        unsafe { required.write(bytes.len()) };
        if capacity < bytes.len() {
            return BUFFER_TOO_SMALL;
        }
        if !bytes.is_empty() {
            // SAFETY: capacity was checked and caller promises writable storage.
            unsafe { std::ptr::copy_nonoverlapping(bytes.as_ptr(), data, bytes.len()) };
        }
        match lease.consume() {
            Ok(()) => OK,
            Err(error) => status(&error),
        }
    })
}

#[unsafe(no_mangle)]
pub extern "C" fn grafikr_request_stop(session: u64) -> i32 {
    guarded(|| {
        with_session(session, |entry| {
            entry.control.request_stop();
            OK
        })
    })
}

#[unsafe(no_mangle)]
pub extern "C" fn grafikr_session_destroy(session: u64) -> i32 {
    guarded(|| {
        let mut sessions = match registry().lock() {
            Ok(sessions) => sessions,
            Err(_) => return INTERNAL_FAILURE,
        };
        let Some(entry) = sessions.get(&session) else {
            return INVALID_SESSION;
        };
        if entry.running {
            return WRONG_LIFECYCLE;
        }
        entry.control.request_stop();
        entry.control.finish();
        sessions.remove(&session);
        OK
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn event_buffer_probe_does_not_consume_frame() {
        let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
        let (control, seed) = Session::channel();
        drop(seed);
        let frame = b"{\"kind\":\"event\"}\n";
        control.publish(frame).unwrap();
        registry().lock().unwrap().insert(
            id,
            FfiSession {
                control: control.clone(),
                seed: None,
                creator: std::thread::current().id(),
                ui_thread: None,
                running: false,
            },
        );

        let held = control.next_lease(Duration::ZERO).unwrap().unwrap();
        let mut required = 0;
        // SAFETY: required is writable; the active lease simulates another reader.
        assert_eq!(
            unsafe { grafikr_next_event(id, std::ptr::null_mut(), 0, &mut required, 0) },
            READER_BUSY
        );
        drop(held);
        // SAFETY: required points to initialized writable storage; null data is a size probe.
        assert_eq!(
            unsafe { grafikr_next_event(id, std::ptr::null_mut(), 0, &mut required, 0) },
            BUFFER_TOO_SMALL
        );
        assert_eq!(required, frame.len());
        let mut short = [0; 2];
        // SAFETY: both buffers have the sizes advertised to the ABI.
        assert_eq!(
            unsafe { grafikr_next_event(id, short.as_mut_ptr(), short.len(), &mut required, 0) },
            BUFFER_TOO_SMALL
        );
        let mut exact = vec![0; frame.len()];
        // SAFETY: exact has required writable bytes.
        assert_eq!(
            unsafe { grafikr_next_event(id, exact.as_mut_ptr(), exact.len(), &mut required, 0) },
            OK
        );
        assert_eq!(exact, frame);
        assert_eq!(grafikr_session_destroy(id), OK);
    }

    #[test]
    fn null_outputs_and_second_destroy_are_rejected() {
        // SAFETY: null is intentionally used to exercise validation.
        assert_eq!(
            unsafe { grafikr_session_create(std::ptr::null_mut()) },
            INVALID_ARGUMENT
        );
        let mut id = 0;
        // SAFETY: id is writable storage.
        assert_eq!(unsafe { grafikr_session_create(&mut id) }, OK);
        assert_eq!(grafikr_session_destroy(id), OK);
        assert_eq!(grafikr_session_destroy(id), INVALID_SESSION);
    }

    #[test]
    fn run_completion_clears_running_state_and_allows_destroy() {
        let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
        let (control, seed) = Session::channel();
        drop(seed);
        registry().lock().unwrap().insert(
            id,
            FfiSession {
                control,
                seed: None,
                creator: std::thread::current().id(),
                ui_thread: Some(std::thread::current().id()),
                running: true,
            },
        );
        assert_eq!(complete_run(id, INTERNAL_FAILURE), INTERNAL_FAILURE);
        assert_eq!(grafikr_session_destroy(id), OK);
    }
}
