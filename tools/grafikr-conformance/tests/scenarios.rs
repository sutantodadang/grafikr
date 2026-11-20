use std::io::{BufRead, BufReader, Write};
use std::process::{Command, Stdio};

use serde_json::{Value, json};

fn request(id: &str, op: &str, args: Value) -> Vec<u8> {
    let mut frame = serde_json::to_vec(&json!({"id":id,"op":op,"args":args})).unwrap();
    frame.push(b'\n');
    frame
}

#[test]
fn fixture_host_uses_the_versioned_dispatcher_protocol() {
    let mut child = Command::new(env!("CARGO_BIN_EXE_fixture-host"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let input = child.stdin.as_mut().unwrap();
    input
        .write_all(&request(
            "1",
            "hello",
            json!({"protocol_major":1,"protocol_minor":0,"sdk_version":"fixture-test"}),
        ))
        .unwrap();
    input
        .write_all(&request(
            "2",
            "state.create",
            json!({"value":{"type":"i64","value":"7"}}),
        ))
        .unwrap();
    input
        .write_all(&request("3", "shutdown", json!({})))
        .unwrap();
    drop(child.stdin.take());
    let output = child.wait_with_output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let lines: Vec<Value> = std::str::from_utf8(&output.stdout)
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    assert_eq!(lines.len(), 3);
    assert_eq!(lines[0]["kind"], "response");
    assert_eq!(lines[0]["result"]["protocol"]["major"], 1);
    assert!(lines[1]["result"]["state"].as_str().is_some());
    assert_eq!(lines[2]["ok"], true);
}

#[test]
fn fixture_host_requires_hello_before_dispatching_requests() {
    let mut child = Command::new(env!("CARGO_BIN_EXE_fixture-host"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let input = child.stdin.as_mut().unwrap();
    input
        .write_all(&request(
            "1",
            "state.create",
            json!({"value":{"type":"bool","value":true}}),
        ))
        .unwrap();
    input
        .write_all(&request(
            "2",
            "hello",
            json!({"protocol_major":1,"protocol_minor":0,"sdk_version":"fixture-test"}),
        ))
        .unwrap();
    input
        .write_all(&request("3", "shutdown", json!({})))
        .unwrap();
    drop(child.stdin.take());
    let output = child.wait_with_output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let lines: Vec<Value> = std::str::from_utf8(&output.stdout)
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    assert_eq!(lines[0]["error"]["code"], "hello_required");
    assert_eq!(lines[1]["result"]["protocol"]["major"], 1);
    assert_eq!(lines[2]["ok"], true);
}

#[test]
fn fixture_host_reports_incomplete_eof_as_a_terminal_error() {
    let mut child = Command::new(env!("CARGO_BIN_EXE_fixture-host"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child.stdin.as_mut().unwrap().write_all(b"{").unwrap();
    drop(child.stdin.take());
    let output = child.wait_with_output().unwrap();
    assert!(!output.status.success());
    let message: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(message["kind"], "event");
    assert_eq!(message["event"], "session.error");
    assert_eq!(message["payload"]["code"], "invalid_frame");
}

#[test]
fn fixture_host_paints_visible_collection_ranges_for_sdk_contracts() {
    let mut child = Command::new(env!("CARGO_BIN_EXE_fixture-host"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut input = child.stdin.take().unwrap();
    let mut output = BufReader::new(child.stdout.take().unwrap());
    let mut line = String::new();

    input
        .write_all(&request(
            "1",
            "hello",
            json!({"protocol_major":1,"protocol_minor":0,"sdk_version":"fixture-test"}),
        ))
        .unwrap();
    input.flush().unwrap();
    line.clear();
    output.read_line(&mut line).unwrap();
    assert_eq!(serde_json::from_str::<Value>(&line).unwrap()["ok"], true);

    input
        .write_all(&request(
            "2",
            "window.create",
            json!({"title":"list","width":320,"height":160}),
        ))
        .unwrap();
    input.flush().unwrap();
    line.clear();
    output.read_line(&mut line).unwrap();
    let window = serde_json::from_str::<Value>(&line).unwrap()["result"].clone();

    input.write_all(&request("3", "node.create", json!({"window":window["window"],"widget":"VirtualList","properties":{"row_count":50000,"row_height":24,"height":120}}))).unwrap();
    input.flush().unwrap();
    line.clear();
    output.read_line(&mut line).unwrap();
    let node = serde_json::from_str::<Value>(&line).unwrap()["result"]["node"].clone();

    input
        .write_all(&request(
            "4",
            "node.attach",
            json!({"node":node,"parent":window["root"]}),
        ))
        .unwrap();
    input.flush().unwrap();
    line.clear();
    output.read_line(&mut line).unwrap();

    input
        .write_all(&request(
            "5",
            "event.subscribe",
            json!({"node":node,"event":"data.request"}),
        ))
        .unwrap();
    input.flush().unwrap();
    line.clear();
    output.read_line(&mut line).unwrap();
    assert_eq!(serde_json::from_str::<Value>(&line).unwrap()["ok"], true);

    input
        .write_all(&request(
            "6",
            "collection.create",
            json!({"node":node,"row_count":50000,"row_height":24,"columns":["name"]}),
        ))
        .unwrap();
    input.flush().unwrap();
    line.clear();
    output.read_line(&mut line).unwrap();
    assert_eq!(serde_json::from_str::<Value>(&line).unwrap()["ok"], true);
    line.clear();
    output.read_line(&mut line).unwrap();
    let event = serde_json::from_str::<Value>(&line).unwrap();
    assert_eq!(event["kind"], "event");
    assert_eq!(event["event"], "data.request");
    let payload = &event["payload"];
    let count = payload["count"].as_u64().unwrap();
    let start = payload["start"].as_u64().unwrap();
    let rows: Vec<Vec<Value>> = (0..count)
        .map(|index| vec![json!(format!("row-{}", start + index))])
        .collect();
    input.write_all(&request("7", "data.provide", json!({"node":node,"revision":payload["revision"],"request":payload["request"],"start":start,"rows":rows}))).unwrap();
    input.flush().unwrap();
    line.clear();
    output.read_line(&mut line).unwrap();
    assert_eq!(serde_json::from_str::<Value>(&line).unwrap()["ok"], true);

    input
        .write_all(&request("8", "shutdown", json!({})))
        .unwrap();
    input.flush().unwrap();
    line.clear();
    output.read_line(&mut line).unwrap();
    drop(input);
    drop(output);
    let output = child.wait_with_output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}
