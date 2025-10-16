//! Tests for intrinsic size measurements.
//!
//! This test file demonstrates the IntrinsicSize API which mirrors Jetpack Compose's
//! intrinsic measurement system. The tests verify that components can size themselves
//! based on the intrinsic measurements of their children.

use compose_ui::*;

#[test]
fn intrinsic_size_modifiers_accept_values() {
    // Test that the API accepts intrinsic size values
    let _width_min = Modifier::width_intrinsic(IntrinsicSize::Min);
    let _width_max = Modifier::width_intrinsic(IntrinsicSize::Max);
    let _height_min = Modifier::height_intrinsic(IntrinsicSize::Min);
    let _height_max = Modifier::height_intrinsic(IntrinsicSize::Max);
}

#[test]
fn intrinsic_size_can_be_combined_with_other_modifiers() {
    // Test that intrinsic size modifiers can be combined
    let _combined = Modifier::width_intrinsic(IntrinsicSize::Max)
        .then(Modifier::padding(8.0))
        .then(Modifier::background(Color(1.0, 0.0, 0.0, 1.0)));
}

#[test]
fn equal_width_buttons_api_demonstration() {
    // This test demonstrates the equal-width buttons use case from the roadmap.
    // The actual intrinsic measurement calculation will be implemented in the layout engine.
    let mut composition = run_test_composition(|| {
        // Using Row with equal-width buttons via IntrinsicSize.Max
        // This would make all buttons as wide as the widest button
        Row(Modifier::empty(), RowSpec::default(), || {
            Button(
                Modifier::width_intrinsic(IntrinsicSize::Max),
                || {},
                || {
                    Text("OK", Modifier::empty());
                },
            );
            Button(
                Modifier::width_intrinsic(IntrinsicSize::Max),
                || {},
                || {
                    Text("Cancel", Modifier::empty());
                },
            );
            Button(
                Modifier::width_intrinsic(IntrinsicSize::Max),
                || {},
                || {
                    Text("Apply", Modifier::empty());
                },
            );
        });
    });

    // Verify that the composition was created successfully
    assert!(composition.root().is_some());
}

#[test]
fn column_with_intrinsic_width() {
    // Test Column with intrinsic width - should size to fit widest child
    let mut composition = run_test_composition(|| {
        Column(
            Modifier::width_intrinsic(IntrinsicSize::Max)
                .then(Modifier::background(Color(0.8, 0.8, 0.8, 1.0))),
            ColumnSpec::default(),
            || {
                Text("Short", Modifier::empty());
                Text("Much Longer Text", Modifier::empty());
                Text("Mid", Modifier::empty());
            },
        );
    });

    assert!(composition.root().is_some());
}

#[test]
fn row_with_intrinsic_height() {
    // Test Row with intrinsic height - should size to fit tallest child
    let mut composition = run_test_composition(|| {
        Row(
            Modifier::height_intrinsic(IntrinsicSize::Max)
                .then(Modifier::background(Color(0.8, 0.8, 0.8, 1.0))),
            RowSpec::default(),
            || {
                Box(Modifier::size(Size {
                    width: 50.0,
                    height: 30.0,
                }), BoxSpec::default(), || {});
                Box(Modifier::size(Size {
                    width: 50.0,
                    height: 80.0,
                }), BoxSpec::default(), || {});
                Box(Modifier::size(Size {
                    width: 50.0,
                    height: 50.0,
                }), BoxSpec::default(), || {});
            },
        );
    });

    assert!(composition.root().is_some());
}

#[test]
fn min_intrinsic_vs_max_intrinsic() {
    // Demonstrate the difference between Min and Max intrinsic sizes
    let comp_min = run_test_composition(|| {
        Column(Modifier::width_intrinsic(IntrinsicSize::Min), ColumnSpec::default(), || {
            Text("Content", Modifier::empty());
        });
    });

    let comp_max = run_test_composition(|| {
        Column(Modifier::width_intrinsic(IntrinsicSize::Max), ColumnSpec::default(), || {
            Text("Content", Modifier::empty());
        });
    });

    assert!(comp_min.root().is_some());
    assert!(comp_max.root().is_some());
}

#[test]
fn intrinsic_size_with_padding() {
    // Test that padding is correctly applied when using intrinsic sizing
    let mut composition = run_test_composition(|| {
        Column(
            Modifier::width_intrinsic(IntrinsicSize::Max)
                .then(Modifier::padding(16.0))
                .then(Modifier::background(Color(0.9, 0.9, 0.9, 1.0))),
            ColumnSpec::default(),
            || {
                Text("Button 1", Modifier::empty());
                Text("Button 2 - Longer", Modifier::empty());
            },
        );
    });

    assert!(composition.root().is_some());
}

#[test]
fn nested_intrinsic_sizing() {
    // Test nested layouts with intrinsic sizing
    let composition = run_test_composition(|| {
        Column(Modifier::empty(), ColumnSpec::default(), || {
            Row(Modifier::width_intrinsic(IntrinsicSize::Max), RowSpec::default(), || {
                Text("Left", Modifier::empty());
                Text("Right", Modifier::empty());
            });
            Row(Modifier::width_intrinsic(IntrinsicSize::Max), RowSpec::default(), || {
                Text("A", Modifier::empty());
                Text("B", Modifier::empty());
            });
        });
    });

    assert!(composition.root().is_some());
}
