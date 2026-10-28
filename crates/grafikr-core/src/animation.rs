//! Time-based value transitions driven by the frame scheduler.
//!
//! Widgets tick their transitions during paint and request an animation frame
//! while one is active. No global registry or background thread is involved.

use std::cell::Cell;
use std::rc::Rc;

use crate::signal::Signal;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Easing {
    Linear,
    #[default]
    EaseOutCubic,
    EaseInOutCubic,
}

impl Easing {
    pub fn apply(self, t: f32) -> f32 {
        let t = t.clamp(0.0, 1.0);
        match self {
            Self::Linear => t,
            Self::EaseOutCubic => 1.0 - (1.0 - t).powi(3),
            Self::EaseInOutCubic => {
                if t < 0.5 {
                    4.0 * t * t * t
                } else {
                    1.0 - (-2.0 * t + 2.0).powi(3) / 2.0
                }
            }
        }
    }
}

#[derive(Debug, Default)]
struct State {
    target: Cell<f32>,
    from: Cell<f32>,
    elapsed: Cell<f32>,
    duration: Cell<f32>,
    easing: Cell<Easing>,
}

/// A retargetable animated `f32`. Reading [`Transition::value`] inside a bound
/// expression subscribes to repaints like any other signal.
#[derive(Clone)]
pub struct Transition {
    value: Signal<f32>,
    state: Rc<State>,
}

impl Transition {
    pub fn new(initial: f32) -> Self {
        let state = Rc::new(State {
            duration: Cell::new(0.18),
            ..State::default()
        });
        state.target.set(initial);
        state.from.set(initial);
        state.elapsed.set(state.duration.get());
        Self {
            value: Signal::new(initial),
            state,
        }
    }

    pub fn duration(self, seconds: f32) -> Self {
        self.state.duration.set(seconds.max(0.0));
        self
    }

    pub fn easing(self, easing: Easing) -> Self {
        self.state.easing.set(easing);
        self
    }

    pub fn value(&self) -> f32 {
        self.value.get()
    }

    pub fn signal(&self) -> Signal<f32> {
        self.value.clone()
    }

    pub fn target(&self) -> f32 {
        self.state.target.get()
    }

    /// Start moving toward `target` from the current value.
    pub fn set_target(&self, target: f32) {
        if (self.state.target.get() - target).abs() <= f32::EPSILON {
            return;
        }
        self.state.from.set(self.value());
        self.state.target.set(target);
        self.state.elapsed.set(0.0);
        if self.state.duration.get() <= 0.0 {
            self.value.set(target);
        }
    }

    /// Jump to `value` and stop animating.
    pub fn jump_to(&self, value: f32) {
        self.state.from.set(value);
        self.state.target.set(value);
        self.state.elapsed.set(self.state.duration.get());
        self.value.set(value);
    }

    /// Advance by `dt` seconds. Returns `true` while more frames are needed.
    pub fn tick(&self, dt: f32) -> bool {
        let target = self.state.target.get();
        let current = self.value();
        if (current - target).abs() <= 1e-4 {
            if current != target {
                self.value.set(target);
            }
            return false;
        }
        let duration = self.state.duration.get();
        let elapsed = (self.state.elapsed.get() + dt.max(0.0)).min(duration);
        self.state.elapsed.set(elapsed);
        let t = if duration <= 0.0 {
            1.0
        } else {
            elapsed / duration
        };
        let from = self.state.from.get();
        self.value
            .set(from + (target - from) * self.state.easing.get().apply(t));
        t < 1.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn easing_hits_endpoints() {
        assert_eq!(Easing::Linear.apply(0.0), 0.0);
        assert_eq!(Easing::EaseOutCubic.apply(1.0), 1.0);
        assert_eq!(Easing::EaseInOutCubic.apply(0.0), 0.0);
        assert_eq!(Easing::EaseInOutCubic.apply(1.0), 1.0);
    }

    #[test]
    fn transition_reaches_target_and_stops() {
        let transition = Transition::new(0.0).duration(0.2).easing(Easing::Linear);
        transition.set_target(1.0);
        assert!(transition.tick(0.1));
        assert!((transition.value() - 0.5).abs() < 0.01);
        assert!(!transition.tick(0.1));
        assert_eq!(transition.value(), 1.0);
        assert!(!transition.tick(0.1));
    }

    #[test]
    fn retarget_uses_current_value_as_origin() {
        let transition = Transition::new(0.0).duration(0.2).easing(Easing::Linear);
        transition.set_target(1.0);
        transition.tick(0.1);
        transition.set_target(0.0);
        assert!(transition.tick(0.1));
        assert!(transition.value() > 0.0 && transition.value() < 0.5);
    }

    #[test]
    fn zero_duration_snaps() {
        let transition = Transition::new(0.0).duration(0.0);
        transition.set_target(5.0);
        assert_eq!(transition.value(), 5.0);
        assert!(!transition.tick(0.016));
    }
}
