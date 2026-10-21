//! Scalar construction keeps independent declaration targets for shared Rust bodies.
pub(super) mod aggregation;
pub(super) mod conversion;
mod float;
mod integer;
use crate::foundation::{
    default_impl, enumeration,
    enumerations::value::{self as enums, Entry},
    interface, override_entry,
};
use kagari_runtime::native::{
    assembly::draft::{ModuleDraft, TraitImplementation},
    binding::{Codec, NativeBinding, NativeResult},
    scalar::NativeScalar,
    types::Type,
};
use kagari_types::{conversion::checked_conversion_fallible, scalar::BuiltinType};

const SCALARS: [BuiltinType; 11] = [
    BuiltinType::Bool,
    BuiltinType::I8,
    BuiltinType::I16,
    BuiltinType::I32,
    BuiltinType::I64,
    BuiltinType::U8,
    BuiltinType::U16,
    BuiltinType::U32,
    BuiltinType::U64,
    BuiltinType::F32,
    BuiltinType::F64,
];

fn constant<T: NativeScalar>(value: T) -> (Type, NativeBinding) {
    let ty = Type::scalar(T::abi_type());
    let value = value.encode();
    (
        ty.clone(),
        NativeBinding::new(vec![], Codec::Exact(ty), move |_| Ok(value)),
    )
}

pub(super) fn declare(module: &mut ModuleDraft<'_>) -> NativeResult<()> {
    integer::declare(module)?;
    float::declare(module)?;
    for ((ty, binding), description) in [
        (constant(()), "Return the Unit value ()."),
        (constant(false), "Return false."),
        (constant(0_i8), "Return zero of type i8."),
        (constant(0_i16), "Return zero of type i16."),
        (constant(0_i32), "Return zero of type i32."),
        (constant(0_i64), "Return zero of type i64."),
        (constant(0_u8), "Return zero of type u8."),
        (constant(0_u16), "Return zero of type u16."),
        (constant(0_u32), "Return zero of type u32."),
        (constant(0_u64), "Return zero of type u64."),
        (constant(0_f32), "Return positive zero of type f32."),
        (constant(0_f64), "Return positive zero of type f64."),
    ] {
        default_impl(module, ty, binding, description)?;
    }
    let conversion = interface(module, "TryFrom", 1)?;
    let error = module.reference_associated_type(&conversion, "Error", 0)?;
    let fallible = enumeration(module, "TryFromIntError", 0)?.apply([])?;
    let infallible = enumeration(module, "Infallible", 0)?.apply([])?;
    for source in SCALARS {
        for target in SCALARS {
            let Some(can_fail) = checked_conversion_fallible(source, target) else {
                continue;
            };
            let applied = error.bind(
                conversion.apply([Type::scalar(source)])?,
                if can_fail {
                    fallible.clone()
                } else {
                    infallible.clone()
                },
            )?;
            let entry = override_entry(
                module,
                &conversion,
                "try_from",
                0,
                enums::pending(
                    1,
                    &[enums::RESULT, enums::CONVERSION_ERROR],
                    conversion::try_from,
                ),
            )?;
            module.trait_implementation_with(0, |_| {
                Ok(TraitImplementation {
                    receiver: Type::scalar(target),
                    interface: applied,
                    bounds: vec![],
                    bindings: vec![],
                }
                .with_overrides([entry]))
            })?;
        }
    }
    let parse = interface(module, "FromStr", 0)?;
    let parse_error = module.reference_associated_type(&parse, "Err", 0)?;
    let parse_error_type = enumeration(module, "ParseError", 0)?.apply([])?;
    for target in SCALARS {
        let applied = parse_error.bind(parse.apply([])?, parse_error_type.clone())?;
        let entry = override_entry(
            module,
            &parse,
            "from_str",
            0,
            enums::pending(
                1,
                &[enums::RESULT, enums::PARSE_ERROR],
                conversion::from_str,
            ),
        )?;
        module.trait_implementation_with(0, |_| {
            Ok(TraitImplementation {
                receiver: Type::scalar(target),
                interface: applied,
                bounds: vec![],
                bindings: vec![],
            }
            .with_overrides([entry]))
        })?;
        if target.number_type().is_some() {
            for (contract, method, callback) in [
                ("Sum", "sum", aggregation::sum as Entry),
                ("Product", "product", aggregation::product),
            ] {
                aggregate(module, target, contract, method, callback)?;
            }
        }
    }
    Ok(())
}

fn aggregate(
    module: &mut ModuleDraft<'_>,
    target: BuiltinType,
    contract: &str,
    method: &str,
    callback: Entry,
) -> NativeResult<()> {
    let aggregate = interface(module, contract, 1)?;
    let source = module
        .reference_method(&aggregate, method, 1)?
        .parameter(0)?;
    let item = Type::scalar(target);
    let iterable = interface(module, "Iterable", 0)?;
    let iterable_item = module.reference_associated_type(&iterable, "Item", 0)?;
    let iterable_cursor = module.reference_associated_type(&iterable, "Iter", 0)?;
    let iter = module.reference_method(&iterable, "iter", 0)?;
    let iterable = iterable_item.bind(iterable.apply([])?, item.clone())?;
    let cursor = iterable_cursor.projection(source.clone(), iterable.clone(), [])?;
    let iterator = interface(module, "Iterator", 0)?;
    let iterator_item = module.reference_associated_type(&iterator, "Item", 0)?;
    let next = module.reference_method(&iterator, "next", 0)?;
    let iterator = iterator_item.bind(iterator.apply([])?, item.clone())?;
    let entry = override_entry(
        module,
        &aggregate,
        method,
        1,
        enums::pending(1, &[enums::OPTION], callback),
    )?
    .requires(iter.requirement(source, iterable, [])?)
    .requires(next.requirement(cursor, iterator, [])?);
    module.trait_implementation_with(0, |_| {
        Ok(TraitImplementation {
            receiver: item.clone(),
            interface: aggregate.apply([item])?,
            bounds: vec![],
            bindings: vec![],
        }
        .with_overrides([entry]))
    })
}

pub(super) const MODULE_DOCS: (&str, &str) = (
    "Numeric parsing, aggregation and explicit fixed-width integer policies. Ordinary arithmetic remains checked and traps on overflow; integer methods explicitly select wrapping, checked Option, overflowing tuple or saturating results. Rotations use u32 counts modulo the receiver's width. Parsing and conversion failures use the declared Result error.",
    "fn parse(text: String) -> Result<i32, core::num::ParseError> { i32::from_str(text) }",
);
