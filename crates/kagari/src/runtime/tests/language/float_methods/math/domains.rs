use super::{FLOATS, FRAC_PI_2, FRAC_PI_4, PI, close, exact, execute_checks};

#[test]
fn unary_domains_classification_and_signed_zeros_do_not_trap() {
    let mut checks = Vec::new();
    for float in FLOATS {
        let ty = float.ty;
        for nan in [float.quiet_nan, float.signaling_nan | float.sign] {
            for method in ["sin", "cos", "tan", "asin", "acos", "atan", "exp", "ln"] {
                checks.push(format!("{}.{method}().is_nan()", float.value(nan)));
            }
        }
        for input in [f64::INFINITY, f64::NEG_INFINITY] {
            let value = float.value(float.bits(input));
            for method in ["sin", "cos", "tan", "asin", "acos"] {
                checks.push(format!("{value}.{method}().is_nan()"));
            }
            checks.push(close(
                float,
                &format!("{value}.atan()"),
                if input.is_sign_negative() {
                    -FRAC_PI_2
                } else {
                    FRAC_PI_2
                },
            ));
        }
        // The first representable values outside the closed inverse domains
        // must not be clamped to the endpoints, nor trap.
        for bits in [
            float.bits(1.0) + 1,
            (float.bits(1.0) + 1) | float.sign,
            float.bits(2.0),
            float.bits(-2.0),
        ] {
            for method in ["asin", "acos"] {
                checks.push(format!("{}.{method}().is_nan()", float.value(bits)));
            }
        }
        for (input, asin, acos) in [(-1.0, -FRAC_PI_2, PI), (1.0, FRAC_PI_2, 0.0)] {
            let value = float.value(float.bits(input));
            checks.push(close(float, &format!("{value}.asin()"), asin));
            checks.push(close(float, &format!("{value}.acos()"), acos));
        }
        for zero in [0.0, -0.0] {
            let value = float.value(float.bits(zero));
            for method in ["sin", "tan", "asin", "atan"] {
                checks.push(exact(float, &format!("{value}.{method}()"), zero));
            }
            checks.push(exact(float, &format!("{value}.cos()"), 1.0));
            checks.push(close(float, &format!("{value}.acos()"), FRAC_PI_2));
            checks.push(exact(float, &format!("{value}.exp()"), 1.0));
            checks.push(exact(float, &format!("{value}.ln()"), f64::NEG_INFINITY));
        }
        for input in [
            float.bits(-1.0),
            float.sign | 1,
            float.infinity | float.sign,
        ] {
            checks.push(format!("{}.ln().is_nan()", float.value(input)));
        }
        for (method, input, expected) in [
            ("ln", f64::INFINITY, f64::INFINITY),
            ("ln", 1.0, 0.0),
            ("exp", f64::INFINITY, f64::INFINITY),
            ("exp", f64::NEG_INFINITY, 0.0),
        ] {
            checks.push(exact(
                float,
                &format!("{}.{method}()", float.value(float.bits(input))),
                expected,
            ));
        }
        // Finite overflow/underflow are nontrapping too; these are safely beyond
        // either width's transition, not claims about its exact threshold.
        checks.push(exact(float, &format!("(1000.0{ty}).exp()"), f64::INFINITY));
        checks.push(exact(float, &format!("(-1000.0{ty}).exp()"), 0.0));
    }
    execute_checks(checks);
}

#[test]
fn atan2_uses_receiver_y_and_argument_x_for_signed_axes_and_infinite_quadrants() {
    let mut checks = Vec::new();
    for float in FLOATS {
        // atan2 has a branch cut on the negative x axis, including signed x=0.
        for y in [0.0_f64, -0.0] {
            let sign = if y.is_sign_negative() { -1.0 } else { 1.0 };
            for x in [0.0, 1.0, f64::INFINITY] {
                checks.push(exact(
                    float,
                    &format!(
                        "{}.atan2({})",
                        float.value(float.bits(y)),
                        float.value(float.bits(x)),
                    ),
                    y,
                ));
            }
            for x in [-0.0, -1.0, f64::NEG_INFINITY] {
                checks.push(close(
                    float,
                    &format!(
                        "{}.atan2({})",
                        float.value(float.bits(y)),
                        float.value(float.bits(x)),
                    ),
                    sign * PI,
                ));
            }
        }
        for y in [1.0_f64, -1.0, f64::INFINITY, f64::NEG_INFINITY] {
            let sign = if y.is_sign_negative() { -1.0 } else { 1.0 };
            for x in [0.0, -0.0] {
                checks.push(close(
                    float,
                    &format!(
                        "{}.atan2({})",
                        float.value(float.bits(y)),
                        float.value(float.bits(x)),
                    ),
                    sign * FRAC_PI_2,
                ));
            }
            if y.is_infinite() {
                for x in [1.0, -1.0] {
                    checks.push(close(
                        float,
                        &format!(
                            "{}.atan2({})",
                            float.value(float.bits(y)),
                            float.value(float.bits(x)),
                        ),
                        sign * FRAC_PI_2,
                    ));
                }
            } else {
                checks.push(exact(
                    float,
                    &format!(
                        "{}.atan2({})",
                        float.value(float.bits(y)),
                        float.value(float.infinity),
                    ),
                    sign * 0.0,
                ));
                checks.push(close(
                    float,
                    &format!(
                        "{}.atan2({})",
                        float.value(float.bits(y)),
                        float.value(float.infinity | float.sign),
                    ),
                    sign * PI,
                ));
            }
        }
        for (y, x, expected) in [
            (f64::INFINITY, f64::INFINITY, FRAC_PI_4),
            (f64::INFINITY, f64::NEG_INFINITY, 3.0 * FRAC_PI_4),
            (f64::NEG_INFINITY, f64::INFINITY, -FRAC_PI_4),
            (f64::NEG_INFINITY, f64::NEG_INFINITY, -3.0 * FRAC_PI_4),
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
        for nan in [float.quiet_nan, float.signaling_nan | float.sign] {
            for other in [0, float.infinity, float.quiet_nan] {
                for (y, x) in [(nan, other), (other, nan)] {
                    checks.push(format!(
                        "{}.atan2({}).is_nan()",
                        float.value(y),
                        float.value(x),
                    ));
                }
            }
        }
    }
    execute_checks(checks);
}
