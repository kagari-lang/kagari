//! Unicode scalar construction and conversions, independent of numeric policies.
use crate::foundation::{
    Entry, default_impl, enumeration,
    enumerations::value::{self as enums, EnumSymbols},
    interface, override_entry,
};
use kagari_runtime::{
    error::RuntimeError,
    native::{
        assembly::{
            authoring::PendingBinding,
            draft::{ModuleDraft, TraitImplementation},
        },
        binding::{Codec, NativeBinding, NativeResult},
        context::CallContext,
        scalar::NativeScalar,
        types::Type,
    },
    value::Value,
};
use kagari_types::scalar::BuiltinType;

pub(super) fn declare(module: &mut ModuleDraft<'_>) -> NativeResult<()> {
    let character = Type::scalar(BuiltinType::Char);
    default_impl(
        module,
        character.clone(),
        unsafe {
            NativeBinding::new_transport(vec![], Codec::Exact(character.clone()), |_| {
                Ok(Value::Char('\0'))
            })
        },
        "Return the NUL Unicode scalar U+0000.",
    )?;
    let from = interface(module, "From", 1)?;
    for (target, callback, documentation) in [
        (
            Type::scalar(BuiltinType::U32),
            to_u32 as Entry,
            "Return the Unicode scalar value as u32.",
        ),
        (
            Type::scalar(BuiltinType::String),
            to_string as Entry,
            "Encode the character as an immutable UTF-8 String.",
        ),
    ] {
        let entry = override_entry(
            module,
            &from,
            "from",
            0,
            PendingBinding::implementation_send(unsafe {
                NativeBinding::new_transport(
                    vec![Codec::Exact(character.clone())],
                    Codec::Exact(target.clone()),
                    callback,
                )
            }),
        )?
        .documentation(documentation);
        module.trait_implementation_with(0, |_| {
            Ok(TraitImplementation {
                receiver: target,
                interface: from.apply([character.clone()])?,
                bounds: vec![],
                bindings: vec![],
            }
            .with_overrides([entry]))
        })?;
    }
    let conversion = interface(module, "TryFrom", 1)?;
    let error = module.reference_associated_type(&conversion, "Error", 0)?;
    let error_type = enumeration(module, "CharTryFromError", 0)?.apply([])?;
    let applied = error.bind(
        conversion.apply([Type::scalar(BuiltinType::U32)])?,
        error_type,
    )?;
    let entry = override_entry(
        module,
        &conversion,
        "try_from",
        0,
        enums::pending(
            1,
            &[enums::RESULT, ("CharTryFromError", &["InvalidScalar"])],
            try_from_u32,
        ),
    )?
    .documentation("Convert a Unicode scalar value to char. Surrogates U+D800 through U+DFFF and values above U+10FFFF return CharTryFromError::InvalidScalar.");
    module.trait_implementation_with(0, |_| {
        Ok(TraitImplementation {
            receiver: character,
            interface: applied,
            bounds: vec![],
            bindings: vec![],
        }
        .with_overrides([entry]))
    })
}

fn argument(cx: &CallContext<'_>) -> NativeResult<char> {
    // SAFETY: Decode the scalar immediately while the argument frame is live.
    let Value::Char(value) = unsafe { cx.argument_transport(0) }? else {
        return Err(RuntimeError::module_validation("character argument"));
    };
    Ok(value)
}

fn to_u32(cx: &mut CallContext<'_>) -> NativeResult<Value> {
    Ok(u32::from(argument(cx)?).encode())
}

fn to_string(cx: &mut CallContext<'_>) -> NativeResult<Value> {
    let text = argument(cx)?.to_string();
    // SAFETY: This terminal return publishes the encoded result before any further callback or
    // safepoint.
    unsafe { cx.encode_result_transport(text) }
}

fn try_from_u32(symbols: &EnumSymbols, cx: &mut CallContext<'_>) -> NativeResult<Value> {
    // SAFETY: Decode the scalar immediately while the argument frame is live.
    let scalar = u32::decode(unsafe { cx.argument_transport(0) }?)?;
    let (member, value) = match char::from_u32(scalar) {
        Some(value) => ("Ok", Value::Char(value)),
        None => (
            "Err",
            enums::allocate(
                symbols,
                cx,
                &cx.result_type_parameter(1)?,
                "CharTryFromError",
                "InvalidScalar",
                vec![],
            )?,
        ),
    };
    let _root = unsafe {
        // SAFETY: The scalar/error conversion result transfers directly into its root before carrier allocation.
        cx.heap().root_value_transport(value)
    }
    .ok_or_else(|| RuntimeError::module_validation("character conversion result"))?;
    enums::allocate(
        symbols,
        cx,
        &cx.result_type_argument()?,
        "Result",
        member,
        vec![value],
    )
}

pub(super) const MODULE_DOCS: (&str, &str) = (
    "Unicode scalar values, distinct from integers. char has scalar-value equality, ordering and hashing, UTF-8 Display, escaped single-quoted Debug and a NUL default. Explicit conversions provide char to u32 or String and checked u32 to char; Into and TryInto use the corresponding forward implementations. No numeric parsing, arithmetic, aggregation or character-range iteration is provided.",
    "fn scalar(value: u32) -> Result<char, core::char::CharTryFromError> { char::try_from(value) }",
);
