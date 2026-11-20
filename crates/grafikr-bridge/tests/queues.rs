use std::sync::Arc;
use std::time::Duration;

use grafikr_bridge::queue::{BoundedQueue, MAX_QUEUE_BYTES, MAX_QUEUE_MESSAGES};

#[test]
fn queue_enforces_count_and_bytes_and_stop_wakes_reader() {
    let queue = BoundedQueue::new();
    for _ in 0..MAX_QUEUE_MESSAGES {
        queue.push(b"x").unwrap();
    }
    assert_eq!(queue.push(b"x").unwrap_err().code, "queue_full");
    for _ in 0..MAX_QUEUE_MESSAGES {
        queue.pop(Duration::ZERO).unwrap().unwrap();
    }
    queue.push(&vec![b'x'; MAX_QUEUE_BYTES]).unwrap();
    assert_eq!(queue.push(b"x").unwrap_err().code, "queue_full");
    queue.pop(Duration::ZERO).unwrap();
    queue.stop();
    assert_eq!(queue.pop(Duration::from_millis(1)).unwrap(), None);
}

#[test]
fn queued_frames_are_shared_and_terminal_follows_normal_output() {
    let queue = Arc::new(BoundedQueue::new());
    queue.push(b"first").unwrap();
    queue.fail(b"terminal");
    assert_eq!(&*queue.pop(Duration::ZERO).unwrap().unwrap(), b"first");
    assert_eq!(&*queue.pop(Duration::ZERO).unwrap().unwrap(), b"terminal");
}

#[test]
fn buffer_lease_can_be_retried_without_consuming_a_short_read() {
    let queue = Arc::new(BoundedQueue::new());
    queue.push(b"event\n").unwrap();
    let lease = queue.lease(Duration::ZERO).unwrap().unwrap();
    assert!(lease.bytes().len() > 2);
    drop(lease);
    let retry = queue.lease(Duration::ZERO).unwrap().unwrap();
    assert_eq!(retry.bytes(), b"event\n");
    retry.consume().unwrap();
    assert_eq!(queue.pop(Duration::ZERO).unwrap(), None);
}
