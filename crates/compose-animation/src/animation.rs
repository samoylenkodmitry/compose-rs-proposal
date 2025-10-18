//! Animation system for Compose-RS
//!
//! Provides time-based animations with easing curves and spring physics.
//!
//! Note: This module uses camelCase for method names (animateTo, snapTo) to maintain
//! 1:1 API parity with Jetpack Compose.

#![allow(non_snake_case)]

use std::cell::RefCell;
use std::rc::Rc;

use compose_core::{
    with_current_composer, FrameCallbackRegistration, MutableState, Owned, RuntimeHandle, State,
};

/// Trait for types that can be linearly interpolated.
pub trait Lerp {
    fn lerp(&self, target: &Self, fraction: f32) -> Self;
}

impl Lerp for f32 {
    fn lerp(&self, target: &Self, fraction: f32) -> Self {
        self + (target - self) * fraction
    }
}

impl Lerp for f64 {
    fn lerp(&self, target: &Self, fraction: f32) -> Self {
        self + (target - self) * fraction as f64
    }
}

/// Trait for values that can participate in spring animations.
pub trait SpringScalar: Lerp + Clone {
    /// Convert the value to `f32` for physics calculations.
    fn to_f32(&self) -> f32;

    /// Compute the current progress between the start and target values.
    fn spring_progress(start: &Self, target: &Self, current: &Self) -> f32 {
        let start_val = start.to_f32();
        let target_val = target.to_f32();
        let current_val = current.to_f32();

        if (target_val - start_val).abs() < f32::EPSILON {
            1.0
        } else {
            (current_val - start_val) / (target_val - start_val)
        }
    }

    /// Determine whether the current value is close enough to the target to
    /// consider the spring finished.
    fn is_near_target(current: &Self, target: &Self, threshold: f32) -> bool {
        (current.to_f32() - target.to_f32()).abs() < threshold
    }
}

impl SpringScalar for f32 {
    fn to_f32(&self) -> f32 {
        *self
    }
}

impl SpringScalar for f64 {
    fn to_f32(&self) -> f32 {
        *self as f32
    }
}

/// Easing functions for animations matching Jetpack Compose.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Easing {
    /// Linear interpolation (no easing).
    /// Jetpack Compose: LinearEasing
    LinearEasing,
    /// Ease in using cubic curve.
    /// Jetpack Compose: EaseIn (not a standard constant, but supported)
    EaseIn,
    /// Ease out using cubic curve.
    /// Jetpack Compose: EaseOut (not a standard constant, but supported)
    EaseOut,
    /// Ease in and out using cubic curve.
    /// Jetpack Compose: EaseInOut (not a standard constant, but supported)
    EaseInOut,
    /// Fast out, slow in (material design standard).
    /// Jetpack Compose: FastOutSlowInEasing
    FastOutSlowInEasing,
    /// Linear out, slow in (material design).
    /// Jetpack Compose: LinearOutSlowInEasing
    LinearOutSlowInEasing,
    /// Fast out, linear in (material design).
    /// Jetpack Compose: FastOutLinearEasing
    FastOutLinearEasing,
}

impl Easing {
    /// Apply the easing function to a linear fraction [0, 1].
    pub fn transform(&self, fraction: f32) -> f32 {
        match self {
            Easing::LinearEasing => fraction,
            Easing::EaseIn => cubic_bezier(0.42, 0.0, 1.0, 1.0, fraction),
            Easing::EaseOut => cubic_bezier(0.0, 0.0, 0.58, 1.0, fraction),
            Easing::EaseInOut => cubic_bezier(0.42, 0.0, 0.58, 1.0, fraction),
            Easing::FastOutSlowInEasing => cubic_bezier(0.4, 0.0, 0.2, 1.0, fraction),
            Easing::LinearOutSlowInEasing => cubic_bezier(0.0, 0.0, 0.2, 1.0, fraction),
            Easing::FastOutLinearEasing => cubic_bezier(0.4, 0.0, 1.0, 1.0, fraction),
        }
    }
}

/// Cubic bezier curve approximation for easing.
fn cubic_bezier(x1: f32, y1: f32, x2: f32, y2: f32, fraction: f32) -> f32 {
    if fraction <= 0.0 {
        return 0.0;
    }
    if fraction >= 1.0 {
        return 1.0;
    }

    let cx = 3.0 * x1;
    let bx = 3.0 * (x2 - x1) - cx;
    let ax = 1.0 - cx - bx;

    let cy = 3.0 * y1;
    let by = 3.0 * (y2 - y1) - cy;
    let ay = 1.0 - cy - by;

    fn sample_curve(a: f32, b: f32, c: f32, t: f32) -> f32 {
        ((a * t + b) * t + c) * t
    }

    fn sample_derivative(a: f32, b: f32, c: f32, t: f32) -> f32 {
        (3.0 * a * t + 2.0 * b) * t + c
    }

    // Use Newton-Raphson iterations to solve for the parametric value `t`
    // corresponding to the provided x fraction. Clamp to [0, 1] to keep the
    // solution within bounds.
    let mut t = fraction;
    let mut newton_success = false;
    for _ in 0..8 {
        let x = sample_curve(ax, bx, cx, t) - fraction;
        if x.abs() < 1e-6 {
            newton_success = true;
            break;
        }
        let dx = sample_derivative(ax, bx, cx, t);
        if dx.abs() < 1e-6 {
            break;
        }
        t = (t - x / dx).clamp(0.0, 1.0);
    }

    if !newton_success {
        // Fall back to a binary subdivision if Newton-Raphson did not converge.
        let mut t0 = 0.0;
        let mut t1 = 1.0;
        t = fraction;
        for _ in 0..16 {
            let x = sample_curve(ax, bx, cx, t);
            let delta = x - fraction;
            if delta.abs() < 1e-6 {
                break;
            }
            if delta > 0.0 {
                t1 = t;
            } else {
                t0 = t;
            }
            t = 0.5 * (t0 + t1);
        }
    }

    sample_curve(ay, by, cy, t)
}

/// Animation specification combining duration and easing.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AnimationSpec {
    /// Duration in milliseconds.
    pub duration_millis: u64,
    /// Easing function to apply.
    pub easing: Easing,
    /// Delay before starting animation in milliseconds.
    pub delay_millis: u64,
}

impl AnimationSpec {
    /// Create a tween animation with duration and easing.
    pub fn tween(duration_millis: u64, easing: Easing) -> Self {
        Self {
            duration_millis,
            easing,
            delay_millis: 0,
        }
    }

    /// Create a linear tween animation.
    pub fn linear(duration_millis: u64) -> Self {
        Self::tween(duration_millis, Easing::LinearEasing)
    }

    /// Add a delay before the animation starts.
    pub fn with_delay(mut self, delay_millis: u64) -> Self {
        self.delay_millis = delay_millis;
        self
    }
}

impl Default for AnimationSpec {
    fn default() -> Self {
        Self::tween(300, Easing::FastOutSlowInEasing)
    }
}

/// Spring animation configuration.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SpringSpec {
    /// Damping ratio. 1.0 = critically damped, < 1.0 = under-damped (bouncy), > 1.0 = over-damped.
    pub damping_ratio: f32,
    /// Stiffness constant. Higher values = faster animation.
    pub stiffness: f32,
    /// Velocity threshold to stop animation.
    pub velocity_threshold: f32,
    /// Position threshold to stop animation.
    pub position_threshold: f32,
}

impl SpringSpec {
    /// Create a spring with default material design values.
    pub fn default_spring() -> Self {
        Self {
            damping_ratio: 1.0,
            stiffness: 1500.0,
            velocity_threshold: 0.01,
            position_threshold: 0.001,
        }
    }

    /// Create a bouncy spring.
    pub fn bouncy() -> Self {
        Self {
            damping_ratio: 0.5,
            stiffness: 1500.0,
            velocity_threshold: 0.01,
            position_threshold: 0.001,
        }
    }

    /// Create a stiff spring (fast, no bounce).
    pub fn stiff() -> Self {
        Self {
            damping_ratio: 1.0,
            stiffness: 3000.0,
            velocity_threshold: 0.01,
            position_threshold: 0.001,
        }
    }
}

impl Default for SpringSpec {
    fn default() -> Self {
        Self::default_spring()
    }
}

/// Animation type specification.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum AnimationType {
    /// Time-based tween animation.
    Tween(AnimationSpec),
    /// Physics-based spring animation.
    Spring(SpringSpec),
}

impl Default for AnimationType {
    fn default() -> Self {
        AnimationType::Tween(AnimationSpec::default())
    }
}

/// Generic animatable value holder.
pub struct Animatable<T: SpringScalar + 'static> {
    inner: Rc<RefCell<AnimatableInner<T>>>,
}

struct AnimatableInner<T: SpringScalar + 'static> {
    state: MutableState<T>,
    runtime: RuntimeHandle,
    current: T,
    /// Velocity for spring animations (currently unused, reserved for future spring physics)
    #[allow(dead_code)]
    velocity: f32,
    start: T,
    target: T,
    animation_type: AnimationType,
    start_time_nanos: Option<u64>,
    registration: Option<FrameCallbackRegistration>,
}

impl<T: SpringScalar + 'static> Animatable<T> {
    /// Create a new animatable with the given initial value.
    pub fn new(initial: T, runtime: RuntimeHandle) -> Self {
        let inner = AnimatableInner {
            state: MutableState::with_runtime(initial.clone(), runtime.clone()),
            runtime,
            current: initial.clone(),
            velocity: 0.0,
            start: initial.clone(),
            target: initial,
            animation_type: AnimationType::default(),
            start_time_nanos: None,
            registration: None,
        };
        Self {
            inner: Rc::new(RefCell::new(inner)),
        }
    }

    /// Animate to the target value using the specified animation.
    pub fn animateTo(&mut self, target: T, animation: AnimationType) {
        let should_schedule = {
            let mut inner = self.inner.borrow_mut();

            // Cancel existing animation
            if let Some(registration) = inner.registration.take() {
                registration.cancel();
            }

            inner.start = inner.current.clone();
            inner.target = target;
            inner.animation_type = animation;
            inner.start_time_nanos = None;

            true // Always schedule for now
        };

        if should_schedule {
            Self::schedule_frame(&self.inner);
        }
    }

    /// Return the current animation target.
    pub fn target(&self) -> T {
        self.inner.borrow().target.clone()
    }

    /// Return the animation spec currently driving this animatable.
    pub fn animation_type(&self) -> AnimationType {
        self.inner.borrow().animation_type
    }

    /// Get the current state.
    pub fn state(&self) -> State<T> {
        self.inner.borrow().state.as_state()
    }

    /// Snap immediately to the target value without animating.
    pub fn snapTo(&mut self, target: T) {
        let mut inner = self.inner.borrow_mut();
        if let Some(registration) = inner.registration.take() {
            registration.cancel();
        }
        inner.current = target.clone();
        inner.start = target.clone();
        inner.target = target.clone();
        inner.start_time_nanos = None;
        inner.state.set_value(target);
    }

    fn schedule_frame(this: &Rc<RefCell<AnimatableInner<T>>>) {
        let runtime = {
            let inner = this.borrow();
            if inner.registration.is_some() {
                return;
            }
            inner.runtime.clone()
        };
        let weak = Rc::downgrade(this);
        let registration = runtime.frame_clock().with_frame_nanos(move |time| {
            if let Some(strong) = weak.upgrade() {
                Self::on_frame(&strong, time);
            }
        });
        this.borrow_mut().registration = Some(registration);
    }

    fn on_frame(this: &Rc<RefCell<AnimatableInner<T>>>, frame_time_nanos: u64) {
        let mut schedule_next = false;
        {
            let mut inner = this.borrow_mut();
            inner.registration = None;

            match inner.animation_type {
                AnimationType::Tween(spec) => {
                    let start_time = inner.start_time_nanos.get_or_insert(frame_time_nanos);
                    let elapsed_nanos = frame_time_nanos.saturating_sub(*start_time);
                    let delay_nanos = spec.delay_millis * 1_000_000;

                    if elapsed_nanos < delay_nanos {
                        schedule_next = true;
                    } else {
                        let animation_elapsed = elapsed_nanos - delay_nanos;
                        let duration_nanos = spec.duration_millis * 1_000_000;
                        let duration_nanos = duration_nanos.max(1);
                        let linear_progress =
                            (animation_elapsed as f32 / duration_nanos as f32).clamp(0.0, 1.0);
                        let progress = spec.easing.transform(linear_progress);

                        let new_value = inner.start.lerp(&inner.target, progress);
                        inner.current = new_value.clone();
                        inner.state.set_value(new_value);

                        if linear_progress >= 1.0 {
                            inner.current = inner.target.clone();
                            inner.start = inner.target.clone();
                            inner.start_time_nanos = None;
                            inner.state.set_value(inner.target.clone());
                        } else {
                            schedule_next = true;
                        }
                    }
                }
                AnimationType::Spring(spec) => {
                    // Implement spring physics using damped harmonic oscillator
                    let start_time = inner.start_time_nanos.get_or_insert(frame_time_nanos);
                    let elapsed_nanos = frame_time_nanos.saturating_sub(*start_time);
                    let dt = elapsed_nanos as f32 / 1_000_000_000.0; // Convert to seconds

                    // SpringScalar ensures we have scalar values that support the
                    // physics calculations below (currently f32 and f64).
                    if dt == 0.0 {
                        schedule_next = true;
                    } else {
                        // Spring physics calculations
                        // Using semi-implicit Euler integration for stability
                        let stiffness = spec.stiffness;
                        let damping = 2.0 * spec.damping_ratio * stiffness.sqrt();

                        // Simulate spring from last frame to current frame
                        let mut prev_time = 0.0f32;
                        let timestep: f32 = 0.016; // ~60fps timestep for stability

                        while prev_time < dt {
                            let step = timestep.min(dt - prev_time);

                            // Spring force: F = -k * displacement - damping * velocity
                            // For interpolation between start and target:
                            // We treat position as progress from 0 to 1
                            let current_progress = <T as SpringScalar>::spring_progress(
                                &inner.start,
                                &inner.target,
                                &inner.current,
                            );

                            let displacement = current_progress - 1.0; // Target is at 1.0
                            let spring_force = -stiffness * displacement - damping * inner.velocity;

                            // Update velocity and position
                            inner.velocity += spring_force * step;
                            let new_progress = current_progress + inner.velocity * step;

                            // Update current value
                            inner.current = inner
                                .start
                                .lerp(&inner.target, new_progress.clamp(0.0, 2.0));

                            prev_time += step;
                        }

                        inner.state.set_value(inner.current.clone());

                        // Check if we've settled (velocity and displacement both small)
                        let at_rest = inner.velocity.abs() < spec.velocity_threshold;
                        let near_target = <T as SpringScalar>::is_near_target(
                            &inner.current,
                            &inner.target,
                            spec.position_threshold,
                        );

                        if at_rest && near_target {
                            inner.current = inner.target.clone();
                            inner.start = inner.target.clone();
                            inner.start_time_nanos = None;
                            inner.velocity = 0.0;
                            inner.state.set_value(inner.target.clone());
                        } else {
                            schedule_next = true;
                        }
                    }
                }
            }
        }

        if schedule_next {
            Self::schedule_frame(this);
        }
    }
}

#[allow(non_snake_case)]
pub fn animateFloatAsState(target: f32, label: &str) -> State<f32> {
    animateFloatAsStateWithSpec(target, AnimationType::default(), label)
}

#[allow(non_snake_case)]
pub fn animateFloatAsStateWithSpec(
    target: f32,
    animation: AnimationType,
    label: &str,
) -> State<f32> {
    let _ = label;
    with_current_composer(|composer| {
        let runtime = composer.runtime_handle();
        let anim: Owned<Animatable<f32>> = composer.remember(|| Animatable::new(target, runtime));
        anim.update(|animatable| {
            let is_new_target = (animatable.target() - target).abs() > f32::EPSILON;
            let is_new_animation = animatable.animation_type() != animation;
            if is_new_target || is_new_animation {
                animatable.animateTo(target, animation);
            }
        });
        anim.with(|animatable| animatable.state())
    })
}

impl<T: SpringScalar + 'static> Clone for Animatable<T> {
    fn clone(&self) -> Self {
        Self {
            inner: self.inner.clone(),
        }
    }
}

#[cfg(test)]
#[path = "tests/animation_tests.rs"]
mod tests;
