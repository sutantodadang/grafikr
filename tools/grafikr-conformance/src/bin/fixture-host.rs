use std::io::{self, BufRead, Write};
use std::time::Duration;

use grafikr_bridge::dispatch::Dispatcher;
use grafikr_bridge::protocol::{
    BridgeError, MAX_FRAME_BYTES, Operation, Outbound, WireError, decode, encode,
};
use grafikr_bridge::session::Session;
use grafikr_conformance::memory_backend::MemoryBackend;

fn main() {
    if let Err(error) = run() {
        eprintln!("grafikr-fixture-host: {error}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    match args.as_slice() {
        [] => {}
        [flag] if flag == "--version" => {
            println!(
                "grafikr-fixture-host {} protocol 1.0",
                env!("CARGO_PKG_VERSION")
            );
            return Ok(());
        }
        [flag, milliseconds] if flag == "--sleep-ms" => {
            std::thread::sleep(Duration::from_millis(milliseconds.parse()?));
            return Ok(());
        }
        _ => return Err("fixture host accepts only --version or --sleep-ms".into()),
    }

    let (control, seed) = Session::channel();
    drop(seed);
    let mut dispatcher = Dispatcher::new();
    dispatcher.set_publisher(control.clone());
    let mut backend = MemoryBackend::new();
    let stdin = io::stdin();
    let mut input = stdin.lock();
    let stdout = io::stdout();
    let mut output = stdout.lock();
    let mut frame = Vec::with_capacity(4096);
    let mut handshaken = false;
    let mut event_sequence = 1_u64;
    let mut last_subscription: Option<(String, String)> = None;
    let mut deferred_response = None;
    let mut state_get_count = 0_usize;
    let exit_after_state_get_count = std::env::var("GRAFIKR_FIXTURE_EXIT_AFTER_STATE_GET_COUNT")
        .ok()
        .and_then(|value| value.parse::<usize>().ok())
        .filter(|count| (1..=1024).contains(count));

    loop {
        match read_frame(&mut input, &mut frame) {
            Ok(false) => break,
            Ok(true) => {}
            Err(error) => {
                let (code, message) = if error.kind() == io::ErrorKind::InvalidData {
                    ("frame_too_large", "request exceeds 8 MiB")
                } else {
                    ("invalid_frame", "EOF before LF frame terminator")
                };
                write_message(
                    &mut output,
                    &terminal_error(&frame, BridgeError::new(code, message)),
                )?;
                output.flush()?;
                return Err(error.into());
            }
        }
        let request = match decode(&frame) {
            Ok(request) => request,
            Err(error) => {
                write_message(&mut output, &terminal_error(&frame, error))?;
                return Err("invalid protocol frame".into());
            }
        };
        if !handshaken && request.op != Operation::Hello {
            write_message(
                &mut output,
                &request_error(
                    request.id,
                    BridgeError::new("hello_required", "hello must be the first request"),
                ),
            )?;
            output.flush()?;
            continue;
        }
        let hello = request.op == Operation::Hello;
        if request.op == Operation::StateGet {
            state_get_count += 1;
        }
        if request.op == Operation::Subscribe
            && let (Some(node), Some(event)) = (
                request.args.get("node").and_then(serde_json::Value::as_str),
                request
                    .args
                    .get("event")
                    .and_then(serde_json::Value::as_str),
            )
        {
            last_subscription = Some((node.to_owned(), event.to_owned()));
        }
        let unsubscribe_event = (request.op == Operation::Unsubscribe
            && std::env::var_os("GRAFIKR_FIXTURE_UNSUBSCRIBE_EVENT").is_some())
        .then(|| last_subscription.clone())
        .flatten();
        let hold_fixture_response = (std::env::var_os("GRAFIKR_FIXTURE_HOLD_ALL_RESPONSES")
            .is_some()
            && !matches!(
                request.op,
                Operation::Hello | Operation::StateCreate | Operation::Shutdown
            ))
            || (std::env::var_os("GRAFIKR_FIXTURE_HOLD_RESPONSES").is_some()
                && request.op == Operation::StateGet
                && request
                    .args
                    .get("state")
                    .and_then(serde_json::Value::as_str)
                    == Some("0"));
        let collection_subscription = request.op == Operation::Subscribe
            && request
                .args
                .get("event")
                .and_then(serde_json::Value::as_str)
                == Some("data.request")
            && std::env::var_os("GRAFIKR_FIXTURE_COLLECTION_REQUEST").is_some();
        let subscribed_event = request
            .args
            .get("event")
            .and_then(serde_json::Value::as_str)
            .map(str::to_owned);
        let event_burst = if request.op == Operation::Subscribe {
            match subscribed_event.as_deref() {
                Some("change") => std::env::var("GRAFIKR_FIXTURE_EVENT_BURST")
                    .ok()
                    .and_then(|value| value.parse::<usize>().ok())
                    .filter(|count| (1..=1024).contains(count)),
                Some("select")
                    if std::env::var_os("GRAFIKR_FIXTURE_EVENT_COUNT_OVERFLOW").is_some() =>
                {
                    Some(1025)
                }
                Some("resize")
                    if std::env::var_os("GRAFIKR_FIXTURE_EVENT_BYTES_OVERFLOW").is_some() =>
                {
                    Some(9)
                }
                _ => None,
            }
        } else {
            None
        };
        let event_name = subscribed_event.as_deref().unwrap_or("unknown");
        let event_source = request
            .args
            .get("node")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("0")
            .to_owned();
        if request.op == Operation::Subscribe
            && event_burst.is_none()
            && std::env::var_os("GRAFIKR_FIXTURE_EARLY_SUBSCRIBE").is_some()
            && !collection_subscription
        {
            let event = request
                .args
                .get("event")
                .and_then(serde_json::Value::as_str)
                .unwrap_or("unknown");
            let source = request
                .args
                .get("node")
                .and_then(serde_json::Value::as_str)
                .unwrap_or("0");
            serde_json::to_writer(
                &mut output,
                &serde_json::json!({
                    "kind": "event",
                    "sequence": event_sequence.to_string(),
                    "event": event,
                    "source": source,
                    "payload": {}
                }),
            )?;
            output.write_all(b"\n")?;
            event_sequence += 1;
        }
        if let Some((source, event)) = unsubscribe_event {
            serde_json::to_writer(
                &mut output,
                &serde_json::json!({
                    "kind": "event",
                    "sequence": event_sequence.to_string(),
                    "event": event,
                    "source": source,
                    "payload": {}
                }),
            )?;
            output.write_all(b"\n")?;
            event_sequence += 1;
        }
        if request.op == Operation::Unsubscribe
            && let Some(milliseconds) =
                std::env::var("GRAFIKR_FIXTURE_DELAY_UNSUBSCRIBE_RESPONSE_MS")
                    .ok()
                    .and_then(|value| value.parse::<u64>().ok())
                    .filter(|value| *value <= 60_000)
        {
            std::thread::sleep(Duration::from_millis(milliseconds));
        }
        let reverse_response = request.op == Operation::StateGet
            && std::env::var_os("GRAFIKR_FIXTURE_REVERSE_RESPONSES").is_some();
        let response = dispatcher.execute(request, &mut backend);
        if let Some(count) = event_burst {
            write_message(&mut output, &response)?;
            output.flush()?;
            std::thread::sleep(Duration::from_millis(100));
            let padding_bytes = match subscribed_event.as_deref() {
                Some("resize") => 2 * 1024 * 1024,
                Some("select") => 4 * 1024,
                _ => 24 * 1024,
            };
            let padding = "x".repeat(padding_bytes);
            for index in 0..count {
                serde_json::to_writer(
                    &mut output,
                    &serde_json::json!({
                        "kind": "event",
                        "sequence": (event_sequence + index as u64).to_string(),
                        "event": event_name,
                        "source": event_source.as_str(),
                        "payload": {"index": index, "padding": padding.as_str()}
                    }),
                )?;
                output.write_all(b"\n")?;
                if index == 0 {
                    output.flush()?;
                    std::thread::sleep(Duration::from_millis(1000));
                }
            }
            event_sequence += count as u64;
            output.flush()?;
            if let Some(marker) = std::env::var_os("GRAFIKR_FIXTURE_EVENT_MARKER") {
                std::fs::write(marker, format!("{event_name}:{count}"))?;
            }
        } else if reverse_response {
            if let Some(first) = deferred_response.take() {
                write_message(&mut output, &response)?;
                write_message(&mut output, &first)?;
                if let Some(marker) = std::env::var_os("GRAFIKR_FIXTURE_RESPONSE_MARKER") {
                    std::fs::write(marker, "second-before-first")?;
                }
            } else {
                deferred_response = Some(response.clone());
            }
        } else if hello || !hold_fixture_response {
            write_message(&mut output, &response)?;
        }
        if hello && matches!(&response, Outbound::Response { ok: true, .. }) {
            handshaken = true;
        }
        for key in backend.window_keys().collect::<Vec<_>>() {
            let _ = backend.paint(key)?;
        }
        while let Some(event) = control.next(Duration::ZERO)? {
            output.write_all(&event)?;
        }
        output.flush()?;
        if exit_after_state_get_count == Some(state_get_count) {
            return Ok(());
        }
        if hello
            && let Some(milliseconds) = std::env::var("GRAFIKR_FIXTURE_PAUSE_AFTER_HELLO_MS")
                .ok()
                .and_then(|value| value.parse::<u64>().ok())
                .filter(|value| *value <= 60_000)
        {
            if let Some(marker) = std::env::var_os("GRAFIKR_FIXTURE_PAUSE_MARKER") {
                std::fs::write(marker, "paused-after-hello")?;
            }
            std::thread::sleep(Duration::from_millis(milliseconds));
        }
        if hello
            && let Some(milliseconds) = std::env::var("GRAFIKR_FIXTURE_EXIT_AFTER_HELLO_MS")
                .ok()
                .and_then(|value| value.parse::<u64>().ok())
                .filter(|value| *value <= 60_000)
        {
            std::thread::sleep(Duration::from_millis(milliseconds));
            return Ok(());
        }
        if dispatcher.is_stopped() {
            if let Some(milliseconds) = std::env::var("GRAFIKR_FIXTURE_DELAY_EXIT_MS")
                .ok()
                .and_then(|value| value.parse::<u64>().ok())
                .filter(|value| *value <= 60_000)
            {
                if let Some(marker) = std::env::var_os("GRAFIKR_FIXTURE_EXIT_MARKER") {
                    std::fs::write(marker, "delayed-exit")?;
                }
                std::thread::sleep(Duration::from_millis(milliseconds));
            }
            break;
        }
    }
    Ok(())
}

fn read_frame(input: &mut impl BufRead, frame: &mut Vec<u8>) -> io::Result<bool> {
    frame.clear();
    loop {
        let available = input.fill_buf()?;
        if available.is_empty() {
            return if frame.is_empty() {
                Ok(false)
            } else {
                Err(io::Error::new(
                    io::ErrorKind::UnexpectedEof,
                    "EOF before LF frame terminator",
                ))
            };
        }
        let count = available
            .iter()
            .position(|byte| *byte == b'\n')
            .map_or(available.len(), |position| position + 1);
        if frame.len() + count > MAX_FRAME_BYTES {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "frame exceeds 8 MiB",
            ));
        }
        let complete = available[..count].last() == Some(&b'\n');
        frame.extend_from_slice(&available[..count]);
        input.consume(count);
        if complete {
            return Ok(true);
        }
    }
}

fn write_message(output: &mut impl Write, message: &Outbound) -> io::Result<()> {
    let frame = encode(message).map_err(|error| io::Error::other(error.to_string()))?;
    output.write_all(&frame)
}

fn request_error(id: String, error: BridgeError) -> Outbound {
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

fn terminal_error(frame: &[u8], error: BridgeError) -> Outbound {
    let id = serde_json::from_slice::<serde_json::Value>(frame)
        .ok()
        .and_then(|value| value.get("id")?.as_str().map(str::to_owned));
    match id {
        Some(id) => request_error(id, error),
        None => Outbound::Event {
            sequence: "0".into(),
            event: "session.error".into(),
            source: "0".into(),
            payload: serde_json::json!({"code":error.code,"message":error.message}),
        },
    }
}
