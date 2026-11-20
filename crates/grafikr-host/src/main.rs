use std::io::{self, Read, Write};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::Duration;

use grafikr_bridge::driver::BridgeDriver;
use grafikr_bridge::protocol::{BridgeError, MAX_FRAME_BYTES, Outbound, WireError, encode};
use grafikr_core::{MountCx, NodeId, Style, View};
use grafikr_shell::App;

struct HostRoot;
impl View for HostRoot {
    fn mount(&self, cx: &mut MountCx) -> NodeId {
        cx.add_node(Style::default())
    }
}

fn main() {
    if let Err(error) = run() {
        eprintln!("grafikr-host: {error}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    match std::env::args().nth(1).as_deref() {
        Some("--version") => {
            println!("grafikr-host {} protocol 1.0", env!("CARGO_PKG_VERSION"));
            return Ok(());
        }
        Some(argument) => return Err(format!("unsupported argument: {argument}").into()),
        None => {}
    }
    let driver = BridgeDriver::new();
    let control = driver.control();
    let writer_control = control.clone();
    let writer = thread::Builder::new()
        .name("grafikr-host-writer".into())
        .spawn(move || -> io::Result<()> {
            let stdout = io::stdout();
            let mut stdout = stdout.lock();
            while let Some(frame) = writer_control
                .next(Duration::from_secs(86_400))
                .map_err(|e| io::Error::other(e.to_string()))?
            {
                stdout.write_all(&frame)?;
                stdout.flush()?;
            }
            Ok(())
        })?;

    let reader_control = control.clone();
    let protocol_failure = Arc::new(AtomicBool::new(false));
    let reader_failure = protocol_failure.clone();
    let _reader = thread::Builder::new()
        .name("grafikr-host-reader".into())
        .spawn(move || read_requests(reader_control, reader_failure))?;
    let result = App::new(|| HostRoot).run_controlled(driver);
    control.finish();
    writer.join().map_err(|_| "host writer thread panicked")??;
    if protocol_failure.load(Ordering::Acquire) {
        return Err("terminal protocol error".into());
    }
    result?;
    Ok(())
}

fn read_requests(control: grafikr_bridge::session::SessionControl, failed: Arc<AtomicBool>) {
    let stdin = io::stdin();
    let mut input = stdin.lock();
    let mut chunk = [0u8; 8192];
    let mut frame = Vec::with_capacity(4096);
    loop {
        let count = match input.read(&mut chunk) {
            Ok(n) => n,
            Err(error) => {
                eprintln!("grafikr-host: stdin read failed: {error}");
                failed.store(true, Ordering::Release);
                control.request_stop();
                return;
            }
        };
        if count == 0 {
            if !frame.is_empty() {
                reject(
                    &control,
                    &frame,
                    BridgeError::new("invalid_frame", "EOF before LF frame terminator"),
                );
                failed.store(true, Ordering::Release);
            }
            control.request_stop();
            return;
        }
        for byte in &chunk[..count] {
            frame.push(*byte);
            if frame.len() > MAX_FRAME_BYTES {
                reject(
                    &control,
                    &frame,
                    BridgeError::new("frame_too_large", "request exceeds 8 MiB"),
                );
                failed.store(true, Ordering::Release);
                control.request_stop();
                return;
            }
            if *byte == b'\n' {
                if serde_json::from_slice::<serde_json::Value>(&frame).is_err() {
                    reject(
                        &control,
                        &frame,
                        BridgeError::new("invalid_json", "request frame is not valid JSON"),
                    );
                    failed.store(true, Ordering::Release);
                    control.request_stop();
                    return;
                }
                if let Err(error) = control.submit(&frame) {
                    let terminal =
                        matches!(error.code, "queue_full" | "output_overflow" | "stopped");
                    reject(&control, &frame, error);
                    if terminal {
                        failed.store(true, Ordering::Release);
                        control.request_stop();
                        return;
                    }
                }
                frame.clear();
            }
        }
    }
}

fn reject(control: &grafikr_bridge::session::SessionControl, frame: &[u8], error: BridgeError) {
    let id = serde_json::from_slice::<serde_json::Value>(frame)
        .ok()
        .and_then(|value| {
            value
                .get("id")
                .and_then(serde_json::Value::as_str)
                .map(str::to_owned)
        });
    let outbound = if let Some(id) = id {
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
    } else {
        Outbound::Event {
            sequence: "0".into(),
            event: "session.error".into(),
            source: "0".into(),
            payload: serde_json::json!({"code":error.code,"message":error.message}),
        }
    };
    if let Ok(frame) = encode(&outbound) {
        let _ = control.publish(&frame);
    }
}
