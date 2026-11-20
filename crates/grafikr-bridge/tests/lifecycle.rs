use std::time::Duration;

use grafikr_bridge::session::{Lifecycle, Session};

#[test]
fn stop_before_start_is_remembered_and_ids_cannot_be_reused_while_pending() {
    let (control, seed) = Session::channel();
    control
        .submit(br#"{"id":"1","op":"shutdown","args":{}}"#)
        .unwrap();
    assert_eq!(
        control
            .submit(br#"{"id":"1","op":"shutdown","args":{}}"#)
            .unwrap_err()
            .code,
        "duplicate_request"
    );
    control.request_stop();
    assert_eq!(control.lifecycle(), Lifecycle::Stopping);
    let mut session = Session::new(seed);
    assert!(session.start().is_err());
    assert_eq!(session.drain(64).unwrap().len(), 1);
    control
        .publish(br#"{"kind":"response","id":"1","ok":true,"result":{}}"#)
        .unwrap();
    assert!(
        control
            .submit(br#"{"id":"1","op":"shutdown","args":{}}"#)
            .is_err()
    );
    session.finish();
    assert_eq!(
        control.next(Duration::ZERO).unwrap(),
        Some(std::sync::Arc::from(
            &b"{\"kind\":\"response\",\"id\":\"1\",\"ok\":true,\"result\":{}}"[..]
        ))
    );
}
