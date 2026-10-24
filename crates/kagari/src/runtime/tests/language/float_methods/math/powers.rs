use super::{FLOATS, Float, exact, execute_checks};
use rstest::rstest;

#[rstest]
#[case::f32(FLOATS[0])]
#[case::f64(FLOATS[1])]
fn powi_preserves_exact_integer_parity_and_unit_rules_including_i32_extremes(#[case] float: Float) {
    let mut checks = Vec::new();
    for base in [0.0_f64, -0.0, f64::INFINITY, f64::NEG_INFINITY] {
        for exponent in [-3, -2, -1, 0, 1, 2, 3] {
            // Integer power rules, not another call to Rust's powi: odd
            // powers retain the base sign; negative powers reciprocate.
            let negative = base.is_sign_negative() && exponent % 2 != 0;
            let magnitude = if exponent == 0 {
                1.0
            } else if (base == 0.0) == (exponent > 0) {
                0.0
            } else {
                f64::INFINITY
            };
            let expected = if negative { -magnitude } else { magnitude };
            checks.push(exact(
                float,
                &format!("{}.powi({exponent}i32)", float.value(float.bits(base))),
                expected,
            ));
        }
    }
    for nan in [float.quiet_nan, float.signaling_nan | float.sign] {
        checks.push(exact(
            float,
            &format!("{}.powi(0i32)", float.value(nan)),
            1.0,
        ));
        for exponent in [i32::MIN, -1, 1, i32::MAX] {
            checks.push(format!("{}.powi({exponent}i32).is_nan()", float.value(nan)));
        }
    }
    // Independent mathematical limits: MIN is negative and even; MAX is
    // positive and odd. These check the wrapper's frozen sign/unit contract,
    // not agreement with a host primitive that may lose exponent parity.
    // Columns: base, result at MIN, result at MAX. Nonunit finite magnitudes
    // are far beyond both widths' overflow/underflow thresholds.
    for (base, at_min, at_max) in [
        (0.0, f64::INFINITY, 0.0),
        (-0.0, f64::INFINITY, -0.0),
        (1.0, 1.0, 1.0),
        (-1.0, 1.0, -1.0),
        (2.0, 0.0, f64::INFINITY),
        (-2.0, 0.0, f64::NEG_INFINITY),
        (0.5, f64::INFINITY, 0.0),
        (-0.5, f64::INFINITY, -0.0),
        (f64::INFINITY, 0.0, f64::INFINITY),
        (f64::NEG_INFINITY, 0.0, f64::NEG_INFINITY),
    ] {
        for (exponent, expected) in [(i32::MIN, at_min), (i32::MAX, at_max)] {
            checks.push(exact(
                float,
                &format!("{}.powi({exponent}i32)", float.value(float.bits(base))),
                expected,
            ));
        }
    }
    for (base, exponent, expected) in [
        (2.0, 1024, f64::INFINITY),
        (-2.0, 1025, f64::NEG_INFINITY),
        (2.0, -2000, 0.0),
        (-2.0, -2001, -0.0),
    ] {
        checks.push(exact(
            float,
            &format!("{}.powi({exponent}i32)", float.value(float.bits(base))),
            expected,
        ));
    }
    execute_checks(checks);
}

#[rstest]
#[case::f32(FLOATS[0])]
#[case::f64(FLOATS[1])]
fn powf_nan_units_fractional_domains_and_infinite_exponents_follow_special_rules(
    #[case] float: Float,
) {
    let mut checks = Vec::new();
    // Unit exceptions precede NaN propagation for both quiet and signaling
    // encodings; the wrapper must enforce this independently of host libm.
    for nan in [float.quiet_nan, float.signaling_nan | float.sign] {
        for zero in [0, float.sign] {
            checks.push(exact(
                float,
                &format!("{}.powf({})", float.value(nan), float.value(zero)),
                1.0,
            ));
        }
        checks.push(exact(
            float,
            &format!(
                "{}.powf({})",
                float.value(float.bits(1.0)),
                float.value(nan)
            ),
            1.0,
        ));
        for exponent in [float.bits(-1.0), float.bits(1.0), float.infinity, nan] {
            checks.push(format!(
                "{}.powf({}).is_nan()",
                float.value(nan),
                float.value(exponent),
            ));
        }
        for base in [
            0,
            float.sign,
            float.bits(-1.0),
            float.bits(2.0),
            float.infinity,
        ] {
            checks.push(format!(
                "{}.powf({}).is_nan()",
                float.value(base),
                float.value(nan),
            ));
        }
    }
    for base in [-2.0, -0.5] {
        for exponent in [0.5, -0.5, 1.5, -1.5] {
            checks.push(format!(
                "{}.powf({}).is_nan()",
                float.value(float.bits(base)),
                float.value(float.bits(exponent)),
            ));
        }
    }
    for base in [0.0_f64, -0.0, f64::INFINITY, f64::NEG_INFINITY] {
        for exponent in [-3.0_f64, -2.0, -0.5, -0.0, 0.0, 0.5, 2.0, 3.0] {
            // Only odd integral exponents carry a negative base's sign;
            // fractional powers of -infinity/-zero use positive magnitude.
            let negative = base.is_sign_negative() && exponent.abs() == 3.0;
            let magnitude = if exponent == 0.0 {
                1.0
            } else if (base == 0.0) == (exponent > 0.0) {
                0.0
            } else {
                f64::INFINITY
            };
            let expected = if negative { -magnitude } else { magnitude };
            checks.push(exact(
                float,
                &format!(
                    "{}.powf({})",
                    float.value(float.bits(base)),
                    float.value(float.bits(exponent)),
                ),
                expected,
            ));
        }
    }
    for base in [
        0.0_f64,
        -0.0,
        0.5,
        -0.5,
        1.0,
        -1.0,
        2.0,
        -2.0,
        f64::INFINITY,
        f64::NEG_INFINITY,
    ] {
        for exponent in [f64::INFINITY, f64::NEG_INFINITY] {
            let magnitude = base.abs();
            let expected = if magnitude == 1.0 {
                1.0
            } else if (magnitude < 1.0) == exponent.is_sign_positive() {
                0.0
            } else {
                f64::INFINITY
            };
            checks.push(exact(
                float,
                &format!(
                    "{}.powf({})",
                    float.value(float.bits(base)),
                    float.value(float.bits(exponent)),
                ),
                expected,
            ));
        }
    }
    for (base, exponent, expected) in [
        (2.0, 1024.0, f64::INFINITY),
        (-2.0, 1025.0, f64::NEG_INFINITY),
        (2.0, -2000.0, 0.0),
        (-2.0, -2001.0, -0.0),
    ] {
        checks.push(exact(
            float,
            &format!(
                "{}.powf({})",
                float.value(float.bits(base)),
                float.value(float.bits(exponent)),
            ),
            expected,
        ));
    }
    execute_checks(checks);
}
