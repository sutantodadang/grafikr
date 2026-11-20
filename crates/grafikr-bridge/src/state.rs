use std::collections::HashMap;

use crate::handles::{Handle, Object, ObjectKind, Registry};
use crate::protocol::BridgeError;

#[derive(Debug, Clone, PartialEq)]
pub enum Scalar {
    Bool(bool),
    I64(i64),
    F64(f64),
    Text(String),
}

pub struct StateStore {
    registry: Registry,
    bound: HashMap<Handle, HashMap<Handle, usize>>,
}

impl Default for StateStore {
    fn default() -> Self {
        Self::new()
    }
}

impl StateStore {
    pub fn new() -> Self {
        Self {
            registry: Registry::new(),
            bound: HashMap::new(),
        }
    }
    pub fn create(&mut self, value: Scalar) -> Result<Handle, BridgeError> {
        validate(&value)?;
        self.registry
            .insert(ObjectKind::State, Object::State(value))
    }
    pub fn get(&self, id: Handle) -> Result<&Scalar, BridgeError> {
        match self.registry.get(id, ObjectKind::State)? {
            Object::State(value) => Ok(value),
            _ => unreachable!("registry kind checked"),
        }
    }
    pub fn set(&mut self, id: Handle, value: Scalar) -> Result<bool, BridgeError> {
        validate(&value)?;
        let current = match self.registry.get_mut(id, ObjectKind::State)? {
            Object::State(value) => value,
            _ => unreachable!("registry kind checked"),
        };
        if *current == value {
            return Ok(false);
        }
        *current = value;
        Ok(true)
    }
    pub fn bind(&mut self, state: Handle, node: Handle) -> Result<(), BridgeError> {
        self.get(state)?;
        let count = self
            .bound
            .get(&state)
            .and_then(|nodes| nodes.get(&node))
            .copied()
            .unwrap_or_default()
            .checked_add(1)
            .ok_or_else(|| BridgeError::new("quota_exceeded", "binding count overflow"))?;
        self.bound.entry(state).or_default().insert(node, count);
        Ok(())
    }
    pub fn unbind(&mut self, state: Handle, node: Handle) {
        if let Some(nodes) = self.bound.get_mut(&state) {
            if let Some(count) = nodes.get_mut(&node) {
                *count -= 1;
                if *count == 0 {
                    nodes.remove(&node);
                }
            }
            if nodes.is_empty() {
                self.bound.remove(&state);
            }
        }
    }
    pub fn destroy(&mut self, id: Handle) -> Result<(), BridgeError> {
        if self.bound.get(&id).is_some_and(|nodes| !nodes.is_empty()) {
            return Err(BridgeError::new(
                "resource_in_use",
                "state still has bound targets",
            ));
        }
        self.registry.remove(id, ObjectKind::State)?;
        Ok(())
    }
}

fn validate(value: &Scalar) -> Result<(), BridgeError> {
    if matches!(value, Scalar::F64(n) if !n.is_finite()) {
        return Err(BridgeError::new(
            "invalid_value",
            "state numbers must be finite",
        ));
    }
    Ok(())
}
