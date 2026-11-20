use grafikr_bridge::handles::{Object, ObjectKind, Registry};

#[test]
fn per_kind_handle_quotas_do_not_collapse_independent_object_limits() {
    let mut registry = Registry::new();
    for _ in 0..100_000 {
        registry
            .insert(ObjectKind::Binding, Object::Marker)
            .expect("the binding quota allows 100,000 handles");
    }
    assert_eq!(
        registry
            .insert(ObjectKind::Binding, Object::Marker)
            .unwrap_err()
            .code,
        "quota_exceeded"
    );
    registry
        .insert(ObjectKind::Subscription, Object::Marker)
        .expect("subscriptions have their own 100,000-handle quota");
}
