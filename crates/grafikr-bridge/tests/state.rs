use grafikr_bridge::handles::Handle;
use grafikr_bridge::state::{Scalar, StateStore};

#[test]
fn state_equality_finiteness_and_binding_lifetime_are_enforced() {
    let mut states = StateStore::new();
    let id = states.create(Scalar::Bool(false)).unwrap();
    assert!(!states.set(id, Scalar::Bool(false)).unwrap());
    assert_eq!(
        states.set(id, Scalar::F64(f64::NAN)).unwrap_err().code,
        "invalid_value"
    );
    states.bind(id, Handle(77)).unwrap();
    assert_eq!(states.destroy(id).unwrap_err().code, "resource_in_use");
    states.unbind(id, Handle(77));
    states.destroy(id).unwrap();
}

#[test]
fn each_binding_holds_the_state_until_its_own_unbind() {
    let mut states = StateStore::new();
    let state = states.create(Scalar::Bool(false)).unwrap();
    let node = Handle(7);
    states.bind(state, node).unwrap();
    states.bind(state, node).unwrap();
    states.unbind(state, node);
    assert_eq!(states.destroy(state).unwrap_err().code, "resource_in_use");
}
