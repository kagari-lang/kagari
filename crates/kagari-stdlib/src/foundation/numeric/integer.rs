//! Installed fixed-width integer policies reuse the shared runtime arithmetic.
use crate::foundation::{
    enumeration,
    enumerations::value::{self as enums, EnumSymbols},
};
use kagari_contract::numeric::method::IntegerMethodContract;
use kagari_runtime::{
    error::RuntimeError,
    native::{
        assembly::draft::ModuleDraft,
        binding::{Codec, NativeBinding, NativeResult},
        context::CallContext,
        declarations::MethodDecl,
        types::Type,
    },
    numeric,
    value::Value,
};
use kagari_types::{integer::IntegerMethod, scalar::BuiltinType, surface::builtin_type_spec};

const INTEGERS: [BuiltinType; 8] = [
    BuiltinType::I8,
    BuiltinType::I16,
    BuiltinType::I32,
    BuiltinType::I64,
    BuiltinType::U8,
    BuiltinType::U16,
    BuiltinType::U32,
    BuiltinType::U64,
];

const METHODS: [(IntegerMethod, &str, &str); 17] = [
    (
        IntegerMethod::WrappingAdd,
        "wrapping_add",
        "Return the sum modulo 2^width, interpreted at the receiver's signedness. Arithmetic overflow wraps instead of trapping.",
    ),
    (
        IntegerMethod::WrappingSub,
        "wrapping_sub",
        "Return the difference modulo 2^width, interpreted at the receiver's signedness. Arithmetic overflow or unsigned underflow wraps instead of trapping.",
    ),
    (
        IntegerMethod::WrappingMul,
        "wrapping_mul",
        "Return the product modulo 2^width, interpreted at the receiver's signedness. Arithmetic overflow wraps instead of trapping.",
    ),
    (
        IntegerMethod::CheckedAdd,
        "checked_add",
        "Return Some(sum) when the sum fits the receiver's range, or None on arithmetic overflow instead of trapping.",
    ),
    (
        IntegerMethod::CheckedSub,
        "checked_sub",
        "Return Some(difference) when the difference fits the receiver's range, or None on arithmetic overflow or unsigned underflow instead of trapping.",
    ),
    (
        IntegerMethod::CheckedMul,
        "checked_mul",
        "Return Some(product) when the product fits the receiver's range, or None on arithmetic overflow instead of trapping.",
    ),
    (
        IntegerMethod::CheckedDiv,
        "checked_div",
        "Return Some(quotient), truncating toward zero. Return None for a zero divisor or signed MIN / -1 overflow instead of trapping.",
    ),
    (
        IntegerMethod::CheckedRem,
        "checked_rem",
        "Return Some(remainder) with the receiver's sign when nonzero. Return None for a zero divisor or signed MIN % -1, even though its mathematical remainder is zero, instead of trapping.",
    ),
    (
        IntegerMethod::OverflowingAdd,
        "overflowing_add",
        "Return (wrapped_sum, overflowed), wrapping modulo 2^width at the receiver's signedness. The flag reports arithmetic overflow in the receiver's range; signed overflow is not an unsigned carry flag. Overflow does not trap.",
    ),
    (
        IntegerMethod::OverflowingSub,
        "overflowing_sub",
        "Return (wrapped_difference, overflowed), wrapping modulo 2^width at the receiver's signedness. The flag reports arithmetic overflow in the receiver's range, including unsigned underflow; signed overflow is not an unsigned borrow flag. Overflow does not trap.",
    ),
    (
        IntegerMethod::OverflowingMul,
        "overflowing_mul",
        "Return (wrapped_product, overflowed), wrapping modulo 2^width at the receiver's signedness. The flag reports arithmetic overflow in the receiver's range, not an unsigned carry flag for signed receivers. Overflow does not trap.",
    ),
    (
        IntegerMethod::SaturatingAdd,
        "saturating_add",
        "Return the sum saturated to the receiver's minimum or maximum when its mathematical value is out of range. Arithmetic overflow does not trap.",
    ),
    (
        IntegerMethod::SaturatingSub,
        "saturating_sub",
        "Return the difference saturated to the receiver's minimum or maximum when its mathematical value is out of range. Unsigned subtraction saturates at zero. Arithmetic overflow does not trap.",
    ),
    (
        IntegerMethod::SaturatingMul,
        "saturating_mul",
        "Return the product saturated to the receiver's minimum or maximum when its mathematical value is out of range. Arithmetic overflow does not trap.",
    ),
    (
        IntegerMethod::WrappingAddSigned,
        "wrapping_add_signed",
        "Add a signed offset of the corresponding width to an unsigned receiver and return the unsigned result modulo 2^width. Overflow and underflow wrap instead of trapping. Available only on u8/i8, u16/i16, u32/i32 and u64/i64 receiver/offset pairs.",
    ),
    (
        IntegerMethod::RotateLeft,
        "rotate_left",
        "Rotate the receiver's fixed-width bit pattern left, moving high bits into low positions. The count is u32 and is reduced modulo the receiver's bit width; zero and multiples of the width leave the value unchanged. Signed results reinterpret the rotated bits at the same signedness. Unlike ordinary shifts, counts at or above the width do not trap.",
    ),
    (
        IntegerMethod::RotateRight,
        "rotate_right",
        "Rotate the receiver's fixed-width bit pattern right, moving low bits into high positions. The count is u32 and is reduced modulo the receiver's bit width; zero and multiples of the width leave the value unchanged. Signed results reinterpret the rotated bits at the same signedness. Unlike ordinary shifts, counts at or above the width do not trap.",
    ),
];

pub(super) fn declare(module: &mut ModuleDraft<'_>) -> NativeResult<()> {
    let option = enumeration(module, "Option", 1)?;
    for receiver in INTEGERS {
        let receiver_name = builtin_type_spec(receiver)
            .ok_or_else(|| RuntimeError::module_validation("integer receiver spelling"))?
            .name;
        let scalar = Type::scalar(receiver);
        let checked_result = option.apply([scalar.clone()])?;
        let overflowing_result = Type::tuple([scalar.clone(), Type::bool()])?;
        module.inherent(scalar.clone(), |methods| {
            for (operation, name, description) in METHODS {
                let Some(contract) = IntegerMethodContract::new(operation, receiver) else {
                    continue;
                };
                let rhs = Type::scalar(contract.rhs());
                let result = if operation.checked() {
                    checked_result.clone()
                } else if operation.overflowing() {
                    overflowing_result.clone()
                } else {
                    scalar.clone()
                };
                let rotating = matches!(
                    operation,
                    IntegerMethod::RotateLeft | IntegerMethod::RotateRight
                );
                let parameter = if rotating { "count" } else { "rhs" };
                let example = if rotating { "1u32" } else { "rhs" };
                let rhs_name = builtin_type_spec(contract.rhs())
                    .ok_or_else(|| RuntimeError::module_validation("integer operand spelling"))?
                    .name;
                let example_parameters = if rotating {
                    format!("value: {receiver_name}")
                } else {
                    format!("value: {receiver_name}, rhs: {rhs_name}")
                };
                let declaration = MethodDecl::instance(name)
                    .parameter(parameter, rhs.clone())
                    .returns(result.clone())
                    .documentation(format!(
                        "{description}\n\nOperands must match the declared types exactly; widths and signedness are not implicitly converted. The receiver and argument are evaluated once, left to right. This method does not change the checked-trap behavior of ordinary arithmetic operators. Runtime and allocation errors still propagate.\n\n# Examples\n\n```kgr\nfn example({example_parameters}) {{ val result = value.{name}({example}); }}\n```"
                    ));
                let parameters = vec![Codec::Exact(scalar.clone()), Codec::Exact(rhs)];
                let result = Codec::Exact(result);
                if operation.checked() {
                    methods.define(declaration.implementation_with_send(move |view| {
                        let symbols = EnumSymbols::new(view.catalog(), &[enums::OPTION])?;
                        Ok(unsafe { NativeBinding::new_transport(parameters, result, move |cx| {
                            // Checked outcomes are bare scalars, so no temporary heap root is needed.
                            let value = invoke(cx, operation, receiver)?;
                            enums::option(&symbols, cx, value)
                        }) })
                    }))?;
                } else {
                    methods.define(declaration.implementation_send(unsafe { NativeBinding::new_transport(
                        parameters,
                        result,
                        move |cx| {
                            // Return an overflowing tuple without allocation or reentry after the helper.
                            invoke(cx, operation, receiver)?.ok_or_else(|| {
                                RuntimeError::module_validation("integer policy result missing")
                            })
                        },
                    ) }))?;
                }
            }
            Ok(())
        })?;
    }
    Ok(())
}

fn invoke(
    cx: &mut CallContext<'_>,
    operation: IntegerMethod,
    receiver: BuiltinType,
) -> NativeResult<Option<Value>> {
    // SAFETY: Both scalar inputs remain in the call frame through checked numeric conversion.
    let arguments = [unsafe { cx.argument_transport(0) }?, unsafe {
        cx.argument_transport(1)
    }?];
    unsafe { numeric::integer_method_transport(cx.heap(), operation, receiver, &arguments) }
}
