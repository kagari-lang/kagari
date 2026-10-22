//! Width-matched Rust math with explicit power unit and integer-parity semantics.
use crate::foundation::numeric::binding::{binary, unary};
use kagari_runtime::native::{
    assembly::draft::ModuleDraft, binding::NativeResult, declarations::MethodDecl,
    scalar::NativeScalar, types::Type,
};

fn documentation(description: &str, parameters: &str, expression: &str) -> String {
    format!(
        "{description}\n\nUse matching Rust float-width math, with the explicit power special-case guards described below; f32 is not widened to f64. Operands must match the declared types exactly. The receiver and arguments are evaluated once, left to right. Numeric domain errors, overflow and underflow return ordinary float results, not arithmetic traps or host panics. NaN signs and payloads are not guaranteed; runtime errors still propagate.\n\n# Precision\n\nPrecision is unspecified and may vary by platform, Rust version and even between invocations within one execution. No universal ULP bound, cross-platform bit identity or bit-identical repeated approximate result is promised. No fast-math, reassociation or implicit fused multiply-add is introduced.\n\n# Examples\n\n```kgr\nfn example({parameters}) {{ val result = {expression}; }}\n```"
    )
}

fn powf_f32(base: f32, exponent: f32) -> f32 {
    if exponent == 0.0 || base == 1.0 {
        return 1.0;
    }
    if base.is_nan() || exponent.is_nan() {
        return f32::NAN;
    }
    base.powf(exponent)
}

fn powf_f64(base: f64, exponent: f64) -> f64 {
    if exponent == 0.0 || base == 1.0 {
        return 1.0;
    }
    if base.is_nan() || exponent.is_nan() {
        return f64::NAN;
    }
    base.powf(exponent)
}

fn powi_f32(base: f32, exponent: i32) -> f32 {
    if exponent == 0 {
        return 1.0;
    }
    if base.is_nan() {
        return f32::NAN;
    }
    let absolute = base.abs();
    let magnitude = if absolute == 1.0 {
        1.0
    } else {
        absolute.powi(exponent)
    };
    // Some host powi implementations lose odd parity for large i32 exponents.
    if base.is_sign_negative() && exponent & 1 != 0 {
        -magnitude
    } else {
        magnitude
    }
}

fn powi_f64(base: f64, exponent: i32) -> f64 {
    if exponent == 0 {
        return 1.0;
    }
    if base.is_nan() {
        return f64::NAN;
    }
    let absolute = base.abs();
    let magnitude = if absolute == 1.0 {
        1.0
    } else {
        absolute.powi(exponent)
    };
    if base.is_sign_negative() && exponent & 1 != 0 {
        -magnitude
    } else {
        magnitude
    }
}

macro_rules! declare_math {
    ($module:expr, $float:ident, $powf:path, $powi:path) => {{
        let scalar = Type::scalar($float::abi_type());
        let receiver = stringify!($float);
        $module.inherent(scalar.clone(), |methods| {
            for (name, description, binding) in [
                (
                    "sin",
                    "Return the sine of the receiver, an angle in radians. Finite inputs produce a value in [-1, 1]. Either signed zero is returned unchanged. Either infinity and NaN return NaN.",
                    unary($float::sin),
                ),
                (
                    "cos",
                    "Return the cosine of the receiver, an angle in radians. Finite inputs produce a value in [-1, 1]. Either signed zero returns 1. Either infinity and NaN return NaN.",
                    unary($float::cos),
                ),
                (
                    "tan",
                    "Return the tangent of the receiver, an angle in radians. Either signed zero is returned unchanged. Either infinity and NaN return NaN. Near odd multiples of pi/2, results can be very large; a floating approximation to a pole does not promise an exact pole or a particular infinity.",
                    unary($float::tan),
                ),
                (
                    "asin",
                    "Return the arcsine in radians in [-pi/2, pi/2]. The domain is [-1, 1]; inputs outside it, including either infinity, return NaN. NaN returns NaN. Either signed zero is returned unchanged; -1 and 1 yield -pi/2 and pi/2 respectively, subject to the precision boundary.",
                    unary($float::asin),
                ),
                (
                    "acos",
                    "Return the arccosine in radians in [0, pi]. The domain is [-1, 1]; inputs outside it, including either infinity, return NaN. NaN returns NaN. Either signed zero yields pi/2; 1 returns positive zero and -1 yields pi, subject to the precision boundary for nonzero angles.",
                    unary($float::acos),
                ),
                (
                    "atan",
                    "Return the arctangent in radians in [-pi/2, pi/2]. All finite inputs are in the domain and either signed zero is returned unchanged. Negative and positive infinity yield -pi/2 and pi/2 respectively, subject to the precision boundary. NaN returns NaN.",
                    unary($float::atan),
                ),
                (
                    "exp",
                    "Return e raised to the receiver's power. All finite inputs are in the domain; either signed zero returns 1. Positive infinity returns positive infinity, negative infinity returns positive zero, and NaN returns NaN. Large finite positive inputs may overflow to positive infinity; large finite negative inputs may underflow to a positive subnormal or positive zero.",
                    unary($float::exp),
                ),
                (
                    "ln",
                    "Return the natural logarithm. Positive finite inputs are in the domain and 1 returns positive zero. Either signed zero returns negative infinity. Negative inputs, including negative infinity, return NaN. Positive infinity returns positive infinity and NaN returns NaN.",
                    unary($float::ln),
                ),
            ] {
                methods.define(
                    MethodDecl::instance(name)
                        .returns(scalar.clone())
                        .documentation(documentation(
                            description,
                            &format!("value: {receiver}"),
                            &format!("value.{name}()"),
                        ))
                        .implementation_send(binding),
                )?;
            }
            methods.define(
                MethodDecl::instance("atan2")
                    .parameter("other", scalar.clone())
                    .returns(scalar.clone())
                    .documentation(documentation(
                        "Return the four-quadrant arctangent atan2(y, x) in radians in [-pi, pi], with the receiver as y and other as x. For finite nonzero coordinates this is the angle from the positive x axis, positive counterclockwise, with the quadrant determined by both signs; it is not simply atan(y/x).\n\nSigned zeros select the axis and side: atan2(+0, +0) = +0, atan2(-0, +0) = -0, atan2(+0, -0) = +pi and atan2(-0, -0) = -pi. More generally, zero y with positive x returns y's signed zero, and zero y with negative x yields pi with y's sign. Nonzero y with either zero x yields pi/2 with y's sign.\n\nInfinite y with finite x yields pi/2 with y's sign. Finite y with positive infinite x returns zero with y's sign; finite y with negative infinite x yields pi with y's sign. When both are infinite, positive x yields pi/4 with y's sign and negative x yields 3*pi/4 with y's sign. If either operand is NaN, return NaN. Nonzero angles remain subject to the precision boundary.",
                        &format!("y: {receiver}, x: {receiver}"),
                        "y.atan2(x)",
                    ))
                    .implementation_send(binary($float::atan2)),
            )?;
            methods.define(
                MethodDecl::instance("powf")
                    .parameter("exponent", scalar.clone())
                    .returns(scalar.clone())
                    .documentation(documentation(
                        "Raise the receiver to a floating exponent of the same width, not exp(exponent * ln(self)). Explicit library guards return 1 for either signed zero exponent with every base, including quiet or signaling NaN, zero and infinities, and for base 1 with every exponent, including quiet or signaling NaN. These unit guards run before NaN handling; otherwise a NaN operand returns a quiet NaN with unspecified sign and payload. Remaining inputs delegate to the matching Rust powf operation.\n\nA finite negative nonzero base with a finite noninteger exponent returns NaN. With a finite integer exponent its result is negative for odd exponents and positive for even exponents. For positive exponents either zero base returns zero; for negative exponents it returns infinity. A negative zero base gives a negative result only for a finite odd integer exponent; all other nonzero exponents give a positive result.\n\nPositive infinite base returns positive infinity for positive exponents and positive zero for negative exponents. Negative infinite base has the same magnitudes, with a negative sign only for finite odd integer exponents; finite noninteger exponents do not make an infinite base a domain NaN.\n\nFor either infinite exponent, a base with absolute value 1 returns 1, including base -1. For positive infinite exponent, absolute base below 1 returns positive zero and above 1 returns positive infinity; negative infinite exponent reverses those results. Finite computations may overflow to infinity or underflow to subnormal values or zero with the power's sign. Results need not agree bit-for-bit with powi even for integer exponents.",
                        &format!("value: {receiver}, exponent: {receiver}"),
                        "value.powf(exponent)",
                    ))
                    .implementation_send(binary($powf)),
            )?;
            methods.define(
                MethodDecl::instance("powi")
                    .parameter("exponent", Type::scalar(i32::abi_type()))
                    .returns(scalar.clone())
                    .documentation(documentation(
                        "Raise the receiver to an exact i32 exponent without converting the exponent to a float. The entire i32 range is accepted, including its minimum and maximum values. Explicit library guards return 1 for exponent zero with every base, including quiet or signaling NaN, either signed zero and either infinity. A NaN base with a nonzero exponent returns a quiet NaN with unspecified sign and payload. Base 1 always returns 1; base -1 returns exactly -1 for odd exponents and 1 for even exponents. For other non-NaN bases, the magnitude delegates to the matching Rust powi on the absolute base; the library reapplies the negative base's sign exactly when the original i32 exponent is odd. This preserves parity even when a host's direct signed-base powi would lose it, while ordinary finite magnitude precision remains Rust-dependent.\n\nNegative finite bases are valid for every integer exponent; odd exponents give negative results and even exponents give positive results. For nonzero exponents, either signed zero base returns zero for positive exponents and infinity for negative exponents, negative only when the base is negative zero and the exponent is odd. Either infinite base returns infinity for positive exponents and zero for negative exponents, negative only when the base is negative infinity and the exponent is odd. Finite computations may overflow to infinity or underflow to subnormal values or zero with the power's sign. Negative exponents compute reciprocal powers; these numeric exceptional results do not trap. The rounding sequence and result may differ from powf.",
                        &format!("value: {receiver}, exponent: i32"),
                        "value.powi(exponent)",
                    ))
                    .implementation_send(binary($powi)),
            )?;
            Ok(())
        })
    }};
}

pub(super) fn declare(module: &mut ModuleDraft<'_>) -> NativeResult<()> {
    declare_math!(module, f32, powf_f32, powi_f32)?;
    declare_math!(module, f64, powf_f64, powi_f64)
}
