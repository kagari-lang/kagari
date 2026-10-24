use super::{FLOATS, Float, execute_checks};
use rstest::rstest;
use std::f64::consts::{E, FRAC_PI_2, FRAC_PI_3, FRAC_PI_4, FRAC_PI_6, LN_2, PI};

#[cfg(feature = "native")]
mod backend;
mod contracts;
mod domains;
mod powers;

fn close(float: Float, expression: &str, expected: f64) -> String {
    // Only these small, well-conditioned analytic points are covered. Allow
    // input/reference rounding and platform libm variation: absolute error <=
    // 2e-6 * max(1, |reference|) for f32, 2e-14 * max(1, |reference|) for f64.
    // This is not a universal accuracy/ULP or cross-platform bit-identity claim.
    let tolerance = if float.ty == "f32" { 2e-6 } else { 2e-14 } * expected.abs().max(1.0);
    format!(
        "{{ val result: {} = {expression}; result.is_finite() \
         && (result - {}).abs() <= {} }}",
        float.ty,
        float.value(float.bits(expected)),
        float.value(float.bits(tolerance)),
    )
}

fn exact(float: Float, expression: &str, expected: f64) -> String {
    format!(
        "({expression}).to_bits() == {}{}",
        float.bits(expected),
        float.uint
    )
}

#[rstest]
#[case::f32(FLOATS[0])]
#[case::f64(FLOATS[1])]
fn all_eleven_methods_match_independent_analytic_finite_reference_points(#[case] float: Float) {
    let mut checks = Vec::new();
    let ty = float.ty;
    // Radian-angle identities, inverse identities, e/ln identities and
    // elementary powers. No expected value calls the operation under test.
    for (method, input, expected) in [
        ("sin", FRAC_PI_6, 0.5),
        ("sin", -FRAC_PI_6, -0.5),
        ("cos", FRAC_PI_3, 0.5),
        ("cos", -FRAC_PI_3, 0.5),
        ("tan", FRAC_PI_4, 1.0),
        ("tan", -FRAC_PI_4, -1.0),
        ("asin", 0.5, FRAC_PI_6),
        ("asin", -0.5, -FRAC_PI_6),
        ("acos", 0.5, FRAC_PI_3),
        ("acos", -0.5, 2.0 * FRAC_PI_3),
        ("atan", 1.0, FRAC_PI_4),
        ("atan", -1.0, -FRAC_PI_4),
        ("exp", 1.0, E),
        ("exp", -1.0, 1.0 / E),
        ("ln", E, 1.0),
        ("ln", 2.0, LN_2),
    ] {
        checks.push(close(
            float,
            &format!("{}.{method}()", float.value(float.bits(input))),
            expected,
        ));
    }
    for (base, exponent, expected) in [
        (2.0, 10, 1024.0),
        (-2.0, 3, -8.0),
        (-2.0, 4, 16.0),
        (-2.0, -3, -0.125),
        (2.0, -4, 0.0625),
    ] {
        checks.push(close(
            float,
            &format!("{}.powi({exponent}i32)", float.value(float.bits(base))),
            expected,
        ));
    }
    for (base, exponent, expected) in [
        (9.0, 0.5, 3.0),
        (16.0, -0.5, 0.25),
        (-2.0, 3.0, -8.0),
        (-2.0, 4.0, 16.0),
        (-2.0, -3.0, -0.125),
    ] {
        checks.push(close(
            float,
            &format!(
                "{}.powf({})",
                float.value(float.bits(base)),
                float.value(float.bits(exponent)),
            ),
            expected,
        ));
    }
    // sqrt(3) rounded from its independent mathematical decimal expansion.
    // Unequal y/x and all quadrants distinguish atan2(y, x) from atan2(x, y).
    let sqrt_three = 1.732_050_807_568_877_2;
    for (y, x, expected) in [
        (sqrt_three, 1.0, FRAC_PI_3),
        (1.0, sqrt_three, FRAC_PI_6),
        (1.0, -1.0, 3.0 * FRAC_PI_4),
        (-1.0, -1.0, -3.0 * FRAC_PI_4),
        (-1.0, 1.0, -FRAC_PI_4),
    ] {
        checks.push(close(
            float,
            &format!(
                "{}.atan2({})",
                float.value(float.bits(y)),
                float.value(float.bits(x)),
            ),
            expected,
        ));
    }
    // Also require the result width through annotated locals, even for
    // integer exponents (the exponent is always i32, never receiver-width).
    checks.push(format!(
        "{{ val exponent: i32 = -2i32; val result: {ty} = (2.0{ty}).powi(exponent); \
         result == 0.25{ty} }}"
    ));
    execute_checks(checks);
}
