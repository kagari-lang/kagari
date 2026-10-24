use super::{FLOATS, Float, execute_checks};
use rstest::rstest;

#[rstest]
#[case::f32(FLOATS[0])]
#[case::f64(FLOATS[1])]
fn classification_and_absolute_value_distinguish_ieee_categories(#[case] float: Float) {
    let mut checks = Vec::new();
    let uint = float.uint;
    for bits in float.patterns() {
        let magnitude = bits & !float.sign;
        let nan = magnitude > float.infinity;
        let infinite = magnitude == float.infinity;
        let finite = magnitude < float.infinity;
        let value = float.value(bits);
        checks.push(format!(
            "{{ val value = {value}; value.is_nan() == {nan} \
             && value.is_infinite() == {infinite} && value.is_finite() == {finite} }}"
        ));
        checks.push(if nan {
            format!("{value}.abs().is_nan()")
        } else {
            format!("{value}.abs().to_bits() == {magnitude}{uint}")
        });
    }
    execute_checks(checks);
}

#[rstest]
#[case::f32(FLOATS[0])]
#[case::f64(FLOATS[1])]
fn square_root_has_width_matched_rounding_and_nontrapping_domains(#[case] float: Float) {
    let mut checks = Vec::new();
    let ty = float.ty;
    let uint = float.uint;
    for (input, expected) in [(0.0, 0.0), (-0.0, -0.0), (1.0, 1.0), (4.0, 2.0)] {
        checks.push(format!(
            "{}.sqrt().to_bits() == {}{uint}",
            float.value(float.bits(input)),
            float.bits(expected)
        ));
    }
    // Correctly rounded sqrt(2), expressed independently of the implementation.
    let sqrt_two: u64 = if ty == "f32" {
        0x3fb5_04f3
    } else {
        0x3ff6_a09e_667f_3bcd
    };
    checks.push(format!("(2.0{ty}).sqrt().to_bits() == {sqrt_two}{uint}"));
    checks.push(format!(
        "{}.sqrt().to_bits() == {}{uint}",
        float.value(float.infinity),
        float.infinity
    ));
    for bits in [
        float.bits(-1.0),
        float.sign | 1,
        float.sign | float.infinity,
        float.quiet_nan,
        float.signaling_nan,
    ] {
        checks.push(format!("{}.sqrt().is_nan()", float.value(bits)));
    }
    execute_checks(checks);
}

#[rstest]
#[case::f32(FLOATS[0])]
#[case::f64(FLOATS[1])]
fn rounding_retains_float_width_tie_direction_and_fraction_sign(#[case] float: Float) {
    let mut checks = Vec::new();
    let uint = float.uint;
    // Columns: input, floor, ceil, round, trunc, fract. Zero signs are deliberate.
    for [input, floor, ceil, round, trunc, fract] in [
        [-2.5, -3.0, -2.0, -3.0, -2.0, -0.5],
        [-1.5, -2.0, -1.0, -2.0, -1.0, -0.5],
        [-0.5, -1.0, -0.0, -1.0, -0.0, -0.5],
        [-0.25, -1.0, -0.0, -0.0, -0.0, -0.25],
        [-0.0, -0.0, -0.0, -0.0, -0.0, 0.0],
        [0.0, 0.0, 0.0, 0.0, 0.0, 0.0],
        [0.25, 0.0, 1.0, 0.0, 0.0, 0.25],
        [0.5, 0.0, 1.0, 1.0, 0.0, 0.5],
        [1.5, 1.0, 2.0, 2.0, 1.0, 0.5],
        [2.5, 2.0, 3.0, 3.0, 2.0, 0.5],
        [-2.0, -2.0, -2.0, -2.0, -2.0, 0.0],
    ] {
        for (method, expected) in [
            ("floor", floor),
            ("ceil", ceil),
            ("round", round),
            ("trunc", trunc),
            ("fract", fract),
        ] {
            checks.push(format!(
                "{}.{method}().to_bits() == {}{uint}",
                float.value(float.bits(input)),
                float.bits(expected)
            ));
        }
    }
    for bits in [float.infinity, float.infinity | float.sign] {
        for method in ["floor", "ceil", "round", "trunc"] {
            checks.push(format!(
                "{}.{method}().to_bits() == {bits}{uint}",
                float.value(bits)
            ));
        }
        checks.push(format!("{}.fract().is_nan()", float.value(bits)));
    }
    for bits in [float.quiet_nan, float.signaling_nan | float.sign] {
        for method in ["floor", "ceil", "round", "trunc", "fract"] {
            checks.push(format!("{}.{method}().is_nan()", float.value(bits)));
        }
    }
    execute_checks(checks);
}

#[rstest]
#[case::f32(FLOATS[0])]
#[case::f64(FLOATS[1])]
fn min_max_ignore_one_nan_and_allow_either_equal_zero_sign(#[case] float: Float) {
    let mut checks = Vec::new();
    let uint = float.uint;
    for (a, b, min, max) in [
        (
            float.bits(-2.0),
            float.bits(3.0),
            float.bits(-2.0),
            float.bits(3.0),
        ),
        (
            float.infinity | float.sign,
            float.infinity,
            float.infinity | float.sign,
            float.infinity,
        ),
        (1, float.infinity - 1, 1, float.infinity - 1),
    ] {
        for (left, right) in [(a, b), (b, a)] {
            for (method, expected) in [("min", min), ("max", max)] {
                checks.push(format!(
                    "{}.{method}({}).to_bits() == {expected}{uint}",
                    float.value(left),
                    float.value(right)
                ));
            }
        }
    }
    let nans = [float.quiet_nan, float.signaling_nan | float.sign];
    for method in ["min", "max"] {
        for nan in nans {
            for other in [float.bits(2.0), 0, float.sign, float.infinity] {
                for (left, right) in [(nan, other), (other, nan)] {
                    checks.push(format!(
                        "{}.{method}({}).to_bits() == {other}{uint}",
                        float.value(left),
                        float.value(right)
                    ));
                }
            }
            for other in nans {
                checks.push(format!(
                    "{}.{method}({}).is_nan()",
                    float.value(nan),
                    float.value(other)
                ));
            }
        }
        for (left, right) in [(0, float.sign), (float.sign, 0)] {
            checks.push(format!(
                "{{ val bits = {}.{method}({}).to_bits(); \
                 bits == 0{uint} || bits == {}{uint} }}",
                float.value(left),
                float.value(right),
                float.sign
            ));
        }
    }
    execute_checks(checks);
}

#[rstest]
#[case::f32(FLOATS[0])]
#[case::f64(FLOATS[1])]
fn valid_clamp_preserves_in_range_bits_and_accepts_infinite_or_equal_bounds(#[case] float: Float) {
    let mut checks = Vec::new();
    let uint = float.uint;
    for (input, low, high, expected) in [
        (-2.0, -1.0, 1.0, -1.0),
        (0.5, -1.0, 1.0, 0.5),
        (2.0, -1.0, 1.0, 1.0),
        (-0.0, -1.0, 1.0, -0.0),
        (0.0, -0.0, -0.0, 0.0),
        (-0.0, 0.0, 0.0, -0.0),
        (-0.0, 0.0, -0.0, -0.0),
        (0.0, 0.0, -0.0, 0.0),
        (2.0, 1.0, 1.0, 1.0),
        (f64::NEG_INFINITY, -1.0, 1.0, -1.0),
        (f64::INFINITY, -1.0, 1.0, 1.0),
        (-0.0, f64::NEG_INFINITY, f64::INFINITY, -0.0),
        (
            f64::NEG_INFINITY,
            f64::NEG_INFINITY,
            f64::INFINITY,
            f64::NEG_INFINITY,
        ),
        (
            f64::INFINITY,
            f64::NEG_INFINITY,
            f64::INFINITY,
            f64::INFINITY,
        ),
        (0.0, f64::INFINITY, f64::INFINITY, f64::INFINITY),
        (0.0, f64::NEG_INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY),
    ] {
        checks.push(format!(
            "{}.clamp({}, {}).to_bits() == {}{uint}",
            float.value(float.bits(input)),
            float.value(float.bits(low)),
            float.value(float.bits(high)),
            float.bits(expected)
        ));
    }
    for nan in [float.quiet_nan, float.signaling_nan | float.sign] {
        for (low, high) in [
            (float.bits(-1.0), float.bits(1.0)),
            (float.infinity | float.sign, float.infinity),
            (0, 0),
        ] {
            checks.push(format!(
                "{}.clamp({}, {}).is_nan()",
                float.value(nan),
                float.value(low),
                float.value(high)
            ));
        }
    }
    execute_checks(checks);
}
