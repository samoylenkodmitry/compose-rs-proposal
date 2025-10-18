use super::*;

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

#[test]
fn try_as_f32_handles_f32() {
    let value = 42.5f32;
    assert_eq!(try_as_f32(&value), Some(42.5));
}

#[test]
fn try_as_f32_handles_f64() {
    let value = 42.5f64;
    assert_eq!(try_as_f32(&value), Some(42.5));
}

#[test]
fn try_as_f32_returns_none_for_other_types() {
    let value = 42i32;
    assert_eq!(try_as_f32(&value), None);

    let value = "hello";
    assert_eq!(try_as_f32(&value), None);
}
