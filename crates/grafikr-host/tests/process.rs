use std::io::{BufRead, BufReader, Write};
use std::process::{Command, Stdio};
use std::sync::{Mutex, MutexGuard};

static PROCESS_TEST_LOCK: Mutex<()> = Mutex::new(());

fn process_test_guard() -> MutexGuard<'static, ()> {
    PROCESS_TEST_LOCK
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

#[test]
fn split_utf8_request_gets_protocol_response_and_eof_stops_host() {
    let _guard = process_test_guard();
    let mut child = Command::new(env!("CARGO_BIN_EXE_grafikr-host"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("host executable starts");
    let mut input = child.stdin.take().unwrap();
    let request = "{\"id\":\"hello-1\",\"op\":\"hello\",\"args\":{\"protocol_major\":1,\"protocol_minor\":0,\"sdk_version\":\"π\"}}\n";
    let split = request.find('π').unwrap() + 1;
    input.write_all(&request.as_bytes()[..split]).unwrap();
    input.flush().unwrap();
    input.write_all(&request.as_bytes()[split..]).unwrap();
    input.flush().unwrap();
    drop(input);
    let output = child.wait_with_output().unwrap();
    assert!(
        output.status.success(),
        "host failed after EOF: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        !output.stdout.is_empty(),
        "host emitted no response after EOF: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let response: serde_json::Value =
        serde_json::from_slice(output.stdout.split(|byte| *byte == b'\n').next().unwrap()).unwrap();
    assert_eq!(response["kind"], "response");
    assert_eq!(response["id"], "hello-1");
    assert_eq!(response["ok"], true);
}

#[test]
fn version_is_the_only_normal_stdout_line() {
    let _guard = process_test_guard();
    let output = Command::new(env!("CARGO_BIN_EXE_grafikr-host"))
        .arg("--version")
        .output()
        .unwrap();
    assert!(output.status.success());
    let text = String::from_utf8(output.stdout).unwrap();
    assert_eq!(text.lines().count(), 1);
    assert!(text.contains("protocol 1.0"));
    assert!(output.stderr.is_empty());
}

#[test]
fn eof_mid_frame_reports_terminal_protocol_error() {
    let _guard = process_test_guard();
    let mut child = Command::new(env!("CARGO_BIN_EXE_grafikr-host"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .as_mut()
        .unwrap()
        .write_all(b"{\"id\":\"partial\"")
        .unwrap();
    drop(child.stdin.take());
    let output = child.wait_with_output().unwrap();
    assert!(!output.status.success());
    let line = String::from_utf8(output.stdout).unwrap();
    let terminal: serde_json::Value = serde_json::from_str(line.lines().next().unwrap()).unwrap();
    assert_eq!(terminal["event"], "session.error");
    assert_eq!(terminal["payload"]["code"], "invalid_frame");
}

#[test]
fn malformed_json_is_rejected_as_a_terminal_protocol_error() {
    let _guard = process_test_guard();
    let mut child = Command::new(env!("CARGO_BIN_EXE_grafikr-host"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut input = child.stdin.take().unwrap();
    input.write_all(b"not-json\n").unwrap();
    input.flush().unwrap();
    drop(input);
    let output = child.wait_with_output().unwrap();
    assert!(!output.status.success());
    let line = String::from_utf8(output.stdout).unwrap();
    let terminal: serde_json::Value = serde_json::from_str(line.lines().next().unwrap()).unwrap();
    assert_eq!(terminal["event"], "session.error");
    assert_eq!(terminal["payload"]["code"], "invalid_json");
}

#[test]
fn oversized_frame_is_rejected_and_stops_the_host() {
    let _guard = process_test_guard();
    let mut child = Command::new(env!("CARGO_BIN_EXE_grafikr-host"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut input = child.stdin.take().unwrap();
    if let Err(error) = input.write_all(&vec![b' '; 8 * 1024 * 1024 + 1]) {
        assert_eq!(error.kind(), std::io::ErrorKind::BrokenPipe);
    }
    drop(input);
    let output = child.wait_with_output().unwrap();
    assert!(!output.status.success());
    let line = String::from_utf8(output.stdout).unwrap();
    let terminal: serde_json::Value = serde_json::from_str(line.lines().next().unwrap()).unwrap();
    assert_eq!(terminal["event"], "session.error");
    assert_eq!(terminal["payload"]["code"], "frame_too_large");
}

#[test]
fn non_hello_request_is_rejected_without_preventing_a_valid_handshake() {
    let _guard = process_test_guard();
    let mut child = Command::new(env!("CARGO_BIN_EXE_grafikr-host"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut input = child.stdin.take().unwrap();
    let mut output = BufReader::new(child.stdout.take().unwrap());
    input
        .write_all(b"{\"id\":\"early\",\"op\":\"window.create\",\"args\":{\"title\":\"bad\",\"width\":100,\"height\":100}}\n")
        .unwrap();
    input
        .write_all(b"{\"id\":\"hello\",\"op\":\"hello\",\"args\":{\"protocol_major\":1,\"protocol_minor\":0,\"sdk_version\":\"test\"}}\n")
        .unwrap();
    input.flush().unwrap();
    let mut rejected_line = String::new();
    let mut hello_line = String::new();
    output.read_line(&mut rejected_line).unwrap();
    output.read_line(&mut hello_line).unwrap();
    let rejected: serde_json::Value = serde_json::from_str(&rejected_line).unwrap();
    let hello: serde_json::Value = serde_json::from_str(&hello_line).unwrap();
    assert_eq!(rejected["error"]["code"], "hello_required");
    assert_eq!(hello["id"], "hello");
    assert_eq!(hello["ok"], true);
    drop(input);
    assert!(child.wait().unwrap().success());
}

#[test]
fn wrong_major_hello_stops_before_following_window_creation() {
    let _guard = process_test_guard();
    let mut child = Command::new(env!("CARGO_BIN_EXE_grafikr-host"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut input = child.stdin.take().unwrap();
    input
        .write_all(b"{\"id\":\"bad-hello\",\"op\":\"hello\",\"args\":{\"protocol_major\":2,\"protocol_minor\":0,\"sdk_version\":\"test\"}}\n")
        .unwrap();
    input
        .write_all(b"{\"id\":\"window\",\"op\":\"window.create\",\"args\":{\"title\":\"must not open\",\"width\":100,\"height\":100}}\n")
        .unwrap();
    input.flush().unwrap();
    drop(input);
    let output = child.wait_with_output().unwrap();
    assert!(
        !output.stdout.is_empty(),
        "host emitted no response; status={:?}, stderr={}",
        output.status,
        String::from_utf8_lossy(&output.stderr)
    );
    let text = String::from_utf8(output.stdout).unwrap();
    let mut lines = text.lines();
    let response: serde_json::Value = serde_json::from_str(lines.next().unwrap()).unwrap();
    assert_eq!(response["id"], "bad-hello");
    assert_eq!(response["error"]["code"], "version_mismatch");
    assert!(lines.next().is_none());
    assert!(output.status.success());
}

#[test]
fn eof_drains_every_accepted_request_across_driver_batches() {
    let _guard = process_test_guard();
    let mut child = Command::new(env!("CARGO_BIN_EXE_grafikr-host"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut input = child.stdin.take().unwrap();
    let mut frames = vec![serde_json::json!({
        "id":"hello",
        "op":"hello",
        "args":{"protocol_major":1,"protocol_minor":0,"sdk_version":"test"}
    })];
    frames.extend((0..70).map(|index| {
        serde_json::json!({"id":format!("state-{index}"),"op":"state.create","args":{"value":{"type":"i64","value":index.to_string()}}})
    }));
    for frame in frames {
        let mut bytes = serde_json::to_vec(&frame).unwrap();
        bytes.push(b'\n');
        input.write_all(&bytes).unwrap();
    }
    drop(input);
    let output = child.wait_with_output().unwrap();
    assert!(
        output.status.success(),
        "host failed while draining EOF: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let lines: Vec<_> = String::from_utf8(output.stdout)
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str::<serde_json::Value>(line).unwrap())
        .collect();
    let received: std::collections::HashSet<_> = lines
        .iter()
        .filter_map(|response| response["id"].as_str())
        .collect();
    let missing: Vec<_> = std::iter::once("hello".to_owned())
        .chain((0..70).map(|index| format!("state-{index}")))
        .filter(|id| !received.contains(id.as_str()))
        .collect();
    assert_eq!(lines.len(), 71, "missing responses: {missing:?}");
    assert!(lines.iter().all(|response| response["ok"] == true));
}
