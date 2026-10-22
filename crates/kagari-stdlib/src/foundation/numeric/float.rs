//! Width-matched float utilities use ordinary exact scalar native bindings.
use crate::foundation::numeric::binding::{binary, unary};
use kagari_runtime::{
    error::{RuntimeError, RuntimeErrorKind},
    native::{
        assembly::draft::ModuleDraft,
        binding::{Codec, NativeBinding, NativeResult},
        declarations::MethodDecl,
        scalar::NativeScalar,
        types::Type,
    },
};

fn documentation(description: &str, parameters: &str, expression: &str) -> String {
    format!(
        "{description}\n\nOperands must match the declared types exactly; f32 operations do not widen to f64. The receiver and arguments are evaluated once, left to right. Domain NaNs are returned normally, not converted into arithmetic traps; arithmetic NaN signs and payloads are not guaranteed. Runtime errors still propagate.\n\n# Examples\n\n```kgr\nfn example({parameters}) {{ val result = {expression}; }}\n```"
    )
}

macro_rules! declare_float {
    ($module:expr, $float:ident, $bits:ident) => {{
        let scalar = Type::scalar($float::abi_type());
        let bits = Type::scalar($bits::abi_type());
        let receiver = stringify!($float);
        let bit_width = stringify!($bits);
        $module.inherent(scalar.clone(), |methods| {
            for (name, description, binding) in [
                (
                    "abs",
                    "Return the absolute value at the receiver's width. Negative zero becomes positive zero and either infinity becomes positive infinity. NaN returns NaN without a payload guarantee.",
                    unary($float::abs),
                ),
                (
                    "sqrt",
                    "Return the correctly rounded square root at the receiver's width. Negative nonzero inputs, including negative infinity, return NaN. Positive infinity stays positive infinity, negative zero remains negative zero, and NaN returns NaN.",
                    unary($float::sqrt),
                ),
                (
                    "floor",
                    "Return the greatest integral float less than or equal to the receiver, rounding toward negative infinity. Signed zero and infinities are retained; NaN returns NaN. The result remains a float, not an integer conversion.",
                    unary($float::floor),
                ),
                (
                    "ceil",
                    "Return the least integral float greater than or equal to the receiver, rounding toward positive infinity. Signed zero and infinities are retained; NaN returns NaN. The result remains a float, not an integer conversion.",
                    unary($float::ceil),
                ),
                (
                    "round",
                    "Return the nearest integral float, rounding halfway cases away from zero. Signed zero and infinities are retained; NaN returns NaN. The result remains a float, not an integer conversion.",
                    unary($float::round),
                ),
                (
                    "trunc",
                    "Return the integral part as a float, rounding toward zero. Signed zero and infinities are retained; NaN returns NaN. The result remains a float, not an integer conversion.",
                    unary($float::trunc),
                ),
                (
                    "fract",
                    "Return the fractional part using the matching Rust primitive: self - self.trunc(). Negative nonintegral inputs have negative fractions. Infinite inputs and NaN return NaN; zero signs follow that subtraction rather than a separate sign-preservation rule.",
                    unary($float::fract),
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
            for (name, description, binding) in [
                (
                    "is_nan",
                    "Return true exactly when the receiver is NaN, including quiet and signaling NaN bit patterns. Infinities, finite values, subnormals and either signed zero return false. No NaN payload comparison is performed.",
                    unary($float::is_nan),
                ),
                (
                    "is_infinite",
                    "Return true exactly for positive or negative infinity. NaNs, finite values, subnormals and either signed zero return false.",
                    unary($float::is_infinite),
                ),
                (
                    "is_finite",
                    "Return true for finite values, including subnormals and either signed zero. Positive infinity, negative infinity and every NaN return false.",
                    unary($float::is_finite),
                ),
            ] {
                methods.define(
                    MethodDecl::instance(name)
                        .returns(Type::bool())
                        .documentation(documentation(
                            description,
                            &format!("value: {receiver}"),
                            &format!("value.{name}()"),
                        ))
                        .implementation_send(binding),
                )?;
            }
            for (name, description, binding) in [
                (
                    "min",
                    "Return the smaller operand using Rust's NaN-ignoring float contract, not total ordering. If exactly one operand is NaN, return the other; if both are NaN, return NaN. Equal positive and negative zero operands may return either sign. Infinities compare normally.",
                    binary($float::min),
                ),
                (
                    "max",
                    "Return the larger operand using Rust's NaN-ignoring float contract, not total ordering. If exactly one operand is NaN, return the other; if both are NaN, return NaN. Equal positive and negative zero operands may return either sign. Infinities compare normally.",
                    binary($float::max),
                ),
            ] {
                methods.define(
                    MethodDecl::instance(name)
                        .parameter("other", scalar.clone())
                        .returns(scalar.clone())
                        .documentation(documentation(
                            description,
                            &format!("value: {receiver}, other: {receiver}"),
                            &format!("value.{name}(other)"),
                        ))
                        .implementation_send(binding),
                )?;
            }
            methods.define(
                MethodDecl::instance("clamp")
                    .parameter("min", scalar.clone())
                    .parameter("max", scalar.clone())
                    .returns(scalar.clone())
                    .documentation(documentation(
                        "Restrict the receiver to the inclusive bounds using Rust's float clamp. A NaN bound or min > max returns ScriptTrap, never a host panic; bounds are validated even when the receiver is NaN. Infinite bounds are legal. With valid bounds a NaN receiver returns NaN, and an in-range receiver is retained, including negative zero. Equal opposite-signed zero bounds are valid. This is not a max/min composition.",
                        &format!("value: {receiver}, min: {receiver}, max: {receiver}"),
                        "value.clamp(min, max)",
                    ))
                    .implementation_send(NativeBinding::new(
                        vec![Codec::Exact(scalar.clone()); 3],
                        Codec::Exact(scalar.clone()),
                        |cx| {
                            let value = $float::decode(cx.argument(0)?)?;
                            let min = $float::decode(cx.argument(1)?)?;
                            let max = $float::decode(cx.argument(2)?)?;
                            if min.is_nan() || max.is_nan() || min > max {
                                return Err(RuntimeError::new(
                                    RuntimeErrorKind::ScriptTrap,
                                    concat!(stringify!($float), "::clamp requires non-NaN bounds with min <= max"),
                                ));
                            }
                            Ok(value.clamp(min, max).encode())
                        },
                    )),
            )?;
            methods.define(
                MethodDecl::instance("to_bits")
                    .returns(bits.clone())
                    .documentation(documentation(
                        &format!("Reinterpret the receiver's IEEE 754 bit pattern as {bit_width}, without a numeric cast. Every bit is retained, including zero signs, subnormals, infinities and quiet/signaling NaN signs and payloads. An immediate {receiver}::from_bits(bits).to_bits() round trip preserves bits exactly; subsequent arithmetic, casts, formatting and math need not preserve NaN payloads. No signaling-NaN exception mode is introduced."),
                        &format!("value: {receiver}"),
                        "value.to_bits()",
                    ))
                    .implementation_send(unary($float::to_bits)),
            )?;
            methods.define(
                MethodDecl::static_method("from_bits")
                    .parameter("bits", bits)
                    .returns(scalar)
                    .documentation(documentation(
                        &format!("Reinterpret {bit_width} bits as an IEEE 754 {receiver}, without a numeric cast. Every bit pattern is accepted, including signed zeros, subnormals, infinities and quiet/signaling NaNs. An immediate {receiver}::from_bits(bits).to_bits() round trip preserves every sign and payload bit; subsequent arithmetic, casts, formatting and math need not preserve NaN payloads. No signaling-NaN exception mode is introduced."),
                        &format!("bits: {bit_width}"),
                        &format!("{receiver}::from_bits(bits)"),
                    ))
                    .implementation_send(unary($float::from_bits)),
            )?;
            Ok(())
        })
    }};
}

pub(super) fn declare(module: &mut ModuleDraft<'_>) -> NativeResult<()> {
    declare_float!(module, f32, u32)?;
    declare_float!(module, f64, u64)
}
