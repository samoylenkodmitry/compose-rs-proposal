use super::*;

use compose_core::{location_key, with_current_composer, Composition, MemoryApplier, State};
use std::cell::RefCell;
use std::rc::Rc;

#[test]
fn animate_float_as_state_interpolates_over_time() {
    let mut composition = Composition::new(MemoryApplier::new());
    let runtime = composition.runtime_handle();
    let root_key = location_key(file!(), line!(), column!());
    let group_key = location_key(file!(), line!(), column!());
    let state_slot = Rc::new(RefCell::new(None::<State<f32>>));
    let target = Rc::new(RefCell::new(0.0f32));

    {
        let state_slot = Rc::clone(&state_slot);
        let target = Rc::clone(&target);
        composition
            .render(root_key, move || {
                let state_slot = Rc::clone(&state_slot);
                let target = Rc::clone(&target);
                with_current_composer(|composer| {
                    composer.with_group(group_key, |_| {
                        let state = animateFloatAsState(*target.borrow(), "alpha");
                        state_slot.borrow_mut().replace(state);
                    });
                });
            })
            .expect("render succeeds");
    }

    let mut samples = Vec::new();
    let initial = state_slot.borrow().as_ref().expect("state available").get();
    samples.push(initial);
    assert_eq!(samples.as_slice(), &[0.0]);
    assert!(!composition.should_render());

    *target.borrow_mut() = 1.0;

    {
        let state_slot = Rc::clone(&state_slot);
        let target = Rc::clone(&target);
        composition
            .render(root_key, move || {
                let state_slot = Rc::clone(&state_slot);
                let target = Rc::clone(&target);
                with_current_composer(|composer| {
                    composer.with_group(group_key, |_| {
                        let state = animateFloatAsState(*target.borrow(), "alpha");
                        state_slot.borrow_mut().replace(state);
                    });
                });
            })
            .expect("render succeeds");
    }

    let immediate = state_slot.borrow().as_ref().expect("state available").get();
    samples.push(immediate);
    assert_eq!(samples[1], 0.0);
    assert!(composition.should_render());

    let mut frame_time = 0u64;
    let mut saw_midpoint = false;
    for _ in 0..32 {
        if !composition.should_render() {
            break;
        }
        frame_time += 16_666_667; // ~60 FPS
        runtime.drain_frame_callbacks(frame_time);
        composition
            .process_invalid_scopes()
            .expect("process invalid scopes succeeds");
        if let Some(state) = state_slot.borrow().as_ref() {
            let value = state.get();
            if value > 0.0 && value < 1.0 {
                saw_midpoint = true;
            }
            samples.push(value);
        }
    }

    let last = *samples.last().expect("at least one value recorded");
    assert!(saw_midpoint, "animation should report intermediate values");
    assert!(
        (last - 1.0).abs() < f32::EPSILON,
        "animation should end at target"
    );
    assert!(!composition.should_render());
}

#[test]
fn easing_linear_is_identity() {
    assert_eq!(Easing::LinearEasing.transform(0.0), 0.0);
    assert_eq!(Easing::LinearEasing.transform(0.5), 0.5);
    assert_eq!(Easing::LinearEasing.transform(1.0), 1.0);
}

#[test]
fn easing_bounds_are_correct() {
    let easings = [
        Easing::LinearEasing,
        Easing::EaseIn,
        Easing::EaseOut,
        Easing::EaseInOut,
        Easing::FastOutSlowInEasing,
    ];

    for easing in easings {
        let start = easing.transform(0.0);
        let end = easing.transform(1.0);
        assert!(
            (start - 0.0).abs() < 0.01,
            "Start should be ~0 for {:?}",
            easing
        );
        assert!(
            (end - 1.0).abs() < 0.01,
            "End should be ~1 for {:?}",
            easing
        );
    }
}

#[test]
fn animation_spec_default_has_reasonable_values() {
    let spec = AnimationSpec::default();
    assert_eq!(spec.duration_millis, 300);
    assert_eq!(spec.easing, Easing::FastOutSlowInEasing);
    assert_eq!(spec.delay_millis, 0);
}

#[test]
fn spring_spec_default_is_critically_damped() {
    let spec = SpringSpec::default();
    assert_eq!(spec.damping_ratio, 1.0);
}

#[test]
fn spring_spec_bouncy_has_low_damping() {
    let spec = SpringSpec::bouncy();
    assert_eq!(spec.damping_ratio, 0.5);
    assert!(
        spec.damping_ratio < 1.0,
        "Bouncy spring should be under-damped"
    );
}

#[test]
fn spring_spec_stiff_has_high_stiffness() {
    let spec = SpringSpec::stiff();
    assert_eq!(spec.stiffness, 3000.0);
    assert!(spec.stiffness > SpringSpec::default().stiffness);
}


