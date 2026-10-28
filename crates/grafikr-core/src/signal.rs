//! Minimal reactive state: values with change subscribers.
//!
//! Subscriptions are collected by *evaluating* a bound closure inside
//! [`observe`]. Every [`Signal::get`] during that evaluation registers the
//! current subscriber. This is the whole dependency-tracking mechanism.

use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};

type Subscriber = Rc<dyn Fn()>;

thread_local! {
    static CURRENT: RefCell<Option<Subscriber>> = const { RefCell::new(None) };
}

/// Evaluate `f` while recording every signal read into `subscriber`.
pub fn observe<R>(subscriber: &Subscriber, f: impl FnOnce() -> R) -> R {
    let previous = CURRENT.with(|current| current.replace(Some(subscriber.clone())));
    let result = f();
    CURRENT.with(|current| *current.borrow_mut() = previous);
    result
}

fn current() -> Option<Subscriber> {
    CURRENT.with(|current| current.borrow().clone())
}

struct Slot<T> {
    value: RefCell<T>,
    version: Cell<u64>,
    subscribers: RefCell<Vec<Weak<dyn Fn()>>>,
}

/// Cloneable, UI-thread-only reactive value.
pub struct Signal<T> {
    slot: Rc<Slot<T>>,
}

impl<T> Clone for Signal<T> {
    fn clone(&self) -> Self {
        Self {
            slot: self.slot.clone(),
        }
    }
}

impl<T: Clone + PartialEq + 'static> Signal<T> {
    pub fn new(value: T) -> Self {
        Self {
            slot: Rc::new(Slot {
                value: RefCell::new(value),
                version: Cell::new(0),
                subscribers: RefCell::new(Vec::new()),
            }),
        }
    }

    pub fn get(&self) -> T {
        if let Some(observer) = current() {
            self.subscribe(&observer);
        }
        self.slot.value.borrow().clone()
    }

    pub fn set(&self, value: T) {
        if *self.slot.value.borrow() != value {
            *self.slot.value.borrow_mut() = value;
            self.notify();
        }
    }

    pub fn update(&self, f: impl FnOnce(&mut T)) {
        let old = self.slot.value.borrow().clone();
        f(&mut self.slot.value.borrow_mut());
        if *self.slot.value.borrow() != old {
            self.notify();
        }
    }

    /// Version increases on every actual change. Useful for caches.
    pub fn version(&self) -> u64 {
        self.slot.version.get()
    }

    fn subscribe(&self, observer: &Subscriber) {
        let mut subscribers = self.slot.subscribers.borrow_mut();
        subscribers.retain(|weak| weak.strong_count() > 0);
        let observer = Rc::downgrade(observer);
        if !subscribers
            .iter()
            .any(|existing| Weak::ptr_eq(existing, &observer))
        {
            subscribers.push(observer);
        }
    }

    fn notify(&self) {
        self.slot.version.set(self.slot.version.get() + 1);
        let subscribers = std::mem::take(&mut *self.slot.subscribers.borrow_mut());
        let mut alive = Vec::with_capacity(subscribers.len());
        for weak in subscribers {
            if let Some(subscriber) = weak.upgrade() {
                subscriber();
                alive.push(weak);
            }
        }
        *self.slot.subscribers.borrow_mut() = alive;
    }
}

/// A closure that re-reads signals and must be re-evaluated on change.
///
/// Evaluation during mount also records the subscriptions.
#[derive(Clone)]
pub struct BoundText(Rc<dyn Fn() -> String>);

impl BoundText {
    pub fn new(f: impl Fn() -> String + 'static) -> Self {
        Self(Rc::new(f))
    }

    pub fn evaluate(&self) -> String {
        (self.0)()
    }

    pub fn raw(&self) -> Rc<dyn Fn() -> String> {
        self.0.clone()
    }

    /// Evaluate and record subscriptions on `subscriber`.
    pub fn tracked(&self, subscriber: &Rc<dyn Fn()>) -> String {
        observe(subscriber, || (self.0)())
    }
}

/// Bind a string view to the signals it reads.
pub fn bind(f: impl Fn() -> String + 'static) -> BoundText {
    BoundText::new(f)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;

    #[test]
    fn get_tracks_and_set_notifies_once() {
        let signal = Signal::new(1);
        let hits = Rc::new(Cell::new(0));
        let observer: Subscriber = {
            let hits = hits.clone();
            Rc::new(move || hits.set(hits.get() + 1))
        };
        assert_eq!(observe(&observer, || signal.get()), 1);
        signal.set(1);
        assert_eq!(hits.get(), 0, "unchanged value must not notify");
        signal.set(2);
        assert_eq!(hits.get(), 1);
        signal.set(3);
        assert_eq!(hits.get(), 2);
    }

    #[test]
    fn duplicate_reads_subscribe_once() {
        let signal = Signal::new(0);
        let hits = Rc::new(Cell::new(0));
        let observer: Subscriber = {
            let hits = hits.clone();
            Rc::new(move || hits.set(hits.get() + 1))
        };
        observe(&observer, || {
            let _ = signal.get();
            let _ = signal.get();
        });
        signal.set(5);
        assert_eq!(hits.get(), 1);
    }

    #[test]
    fn dead_subscribers_are_pruned() {
        let signal = Signal::new(0);
        {
            let observer: Subscriber = Rc::new(|| {});
            observe(&observer, || signal.get());
        }
        // Notifying with only dead subscribers must not panic and must drop them.
        signal.set(1);
        assert_eq!(signal.slot.subscribers.borrow().len(), 0);
    }
}
