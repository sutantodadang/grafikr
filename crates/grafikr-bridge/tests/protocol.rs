use grafikr_bridge::protocol::{MAX_FRAME_BYTES, Operation, Outbound, decode, encode};
use serde_json::json;

#[test]
fn decodes_strict_hello_and_rejects_unknown_or_duplicate_fields() {
    let request = decode(br#"{"id":"1","op":"hello","args":{"protocol_major":1,"protocol_minor":0,"sdk_version":"test"}}"#).unwrap();
    assert_eq!(request.id, "1");
    assert_eq!(request.op, Operation::Hello);
    assert_eq!(
        decode(br#"{"id":"1","op":"shutdown","args":{"extra":1}}"#)
            .unwrap_err()
            .code,
        "unknown_field"
    );
    assert_eq!(
        decode(br#"{"id":"1","id":"2","op":"shutdown","args":{}}"#)
            .unwrap_err()
            .code,
        "invalid_json"
    );
}

#[test]
fn frame_limit_counts_the_canonical_lf() {
    let prefix = br#"{"id":"1","op":"hello","args":{"protocol_major":1,"protocol_minor":0,"sdk_version":""}}"#;
    let mut frame = prefix[..prefix.len() - 3].to_vec();
    frame.extend(std::iter::repeat_n(
        b'x',
        MAX_FRAME_BYTES - prefix.len() - 1,
    ));
    frame.extend_from_slice(
        br#""}}
"#,
    );
    assert_eq!(frame.len(), MAX_FRAME_BYTES);
    assert!(decode(&frame).is_ok());
    frame.insert(frame.len() - 1, b'x');
    assert_eq!(decode(&frame).unwrap_err().code, "frame_too_large");
}

#[test]
fn decimal_handles_and_i64_values_remain_strings() {
    let request = decode(
        br#"{"id":"r","op":"node.attach","args":{"node":"18446744073709551615","parent":"1"}}"#,
    )
    .unwrap();
    assert_eq!(request.args["node"], "18446744073709551615");
    assert_eq!(
        decode(br#"{"id":"r","op":"node.attach","args":{"node":"01","parent":"1"}}"#)
            .unwrap_err()
            .code,
        "invalid_value"
    );
    let line = encode(&Outbound::Response {
        id: "r".to_owned(),
        ok: true,
        result: Some(json!({"value": i64::MIN.to_string()})),
        error: None,
    })
    .unwrap();
    assert!(line.ends_with(b"\n"));
    assert!(
        std::str::from_utf8(&line)
            .unwrap()
            .contains(&i64::MIN.to_string())
    );
}

#[test]
fn transition_wire_arguments_enforce_integer_duration_and_supported_easing() {
    assert!(decode(
        br#"{"id":"t","op":"transition.start","args":{"state":"1","to":{"type":"f64","value":1.0},"duration_ms":100,"easing":"linear"}}"#
    )
    .is_ok());
    for (request, field) in [
        (br#"{"id":"t","op":"transition.start","args":{"state":"1","to":{"type":"f64","value":1.0},"duration_ms":1.5,"easing":"linear"}}"#.as_slice(), "duration_ms"),
        (br#"{"id":"t","op":"transition.start","args":{"state":"1","to":{"type":"f64","value":1.0},"duration_ms":4294967296,"easing":"linear"}}"#.as_slice(), "duration_ms"),
        (br#"{"id":"t","op":"transition.start","args":{"state":"1","to":{"type":"f64","value":1.0},"duration_ms":100,"easing":"spring"}}"#.as_slice(), "easing"),
    ] {
        assert_eq!(decode(request).unwrap_err().field.as_deref(), Some(field));
    }
}

#[test]
fn checked_in_request_corpus_matches_runtime_decoder() {
    for line in include_str!("../../../bindings/protocol/v1/fixtures/valid.jsonl").lines() {
        assert!(
            decode(line.as_bytes()).is_ok(),
            "valid fixture rejected: {line}"
        );
    }
    for line in include_str!("../../../bindings/protocol/v1/fixtures/invalid.jsonl").lines() {
        assert!(
            decode(line.as_bytes()).is_err(),
            "invalid fixture accepted: {line}"
        );
    }
}
