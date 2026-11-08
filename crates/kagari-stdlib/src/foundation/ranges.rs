//! Range constructors and receiver-owned implementations; iterator algorithms remain shared.
pub(super) mod operations;
use crate::foundation::{
    enumerations::value as enums, interface, iteration::operations as iteration, override_entry,
    raw, type_docs,
};
use kagari_runtime::native::{
    assembly::draft::{ModuleDraft, TraitImplementation},
    binding::NativeResult,
    types::Type,
};
use kagari_types::{
    declaration::native::NativeTypeConstructor, range::RangeKind, scalar::BuiltinType,
};

pub(crate) const KINDS: [RangeKind; 6] = [
    RangeKind::Exclusive,
    RangeKind::Inclusive,
    RangeKind::From,
    RangeKind::To,
    RangeKind::ToInclusive,
    RangeKind::Full,
];

pub(super) fn declare(module: &mut ModuleDraft<'_>) -> NativeResult<()> {
    let bounds = interface(module, "RangeBounds", 1)?;
    let iterable = interface(module, "Iterable", 0)?;
    let item_member = module.reference_associated_type(&iterable, "Item", 0)?;
    let cursor_member = module.reference_associated_type(&iterable, "Iter", 0)?;
    for kind in KINDS {
        let range = module.define_constructor(kind.name(), NativeTypeConstructor::Range(kind))?;
        type_docs(module, &range, kind.name(), documentation(kind), &[])?;
        let start = override_entry(
            module,
            &bounds,
            "start_bound",
            0,
            enums::pending(1, &[enums::BOUND], operations::start_bound),
        )?;
        let end = override_entry(
            module,
            &bounds,
            "end_bound",
            0,
            enums::pending(1, &[enums::BOUND], operations::end_bound),
        )?;
        module.trait_implementation_with(1, |arguments| {
            Ok(TraitImplementation {
                receiver: range.apply(if kind == RangeKind::Full {
                    vec![]
                } else {
                    arguments.to_vec()
                })?,
                interface: bounds.apply(arguments.to_vec())?,
                bounds: vec![],
                bindings: vec![],
            }
            .with_overrides([start, end]))
        })?;
        if !kind.has_start() {
            continue;
        }
        for scalar in [
            BuiltinType::I8,
            BuiltinType::I16,
            BuiltinType::I32,
            BuiltinType::I64,
            BuiltinType::U8,
            BuiltinType::U16,
            BuiltinType::U32,
            BuiltinType::U64,
        ] {
            let item = Type::scalar(scalar);
            let interface = cursor_member.bind(
                item_member.bind(iterable.apply([])?, item.clone())?,
                Type::iterator(item.clone())?,
            )?;
            let entry = override_entry(module, &iterable, "iter", 0, raw(1, iteration::iter))?;
            module.trait_implementation_with(0, |_| {
                Ok(TraitImplementation {
                    receiver: range.apply([item])?,
                    interface,
                    bounds: vec![],
                    bindings: vec![],
                }
                .with_overrides([entry]))
            })?;
        }
    }
    Ok(())
}

fn documentation(kind: RangeKind) -> (&'static str, &'static str) {
    match kind {
        RangeKind::Exclusive => (
            "A half-open range `start..end`, including start and excluding end. Integer iteration is empty when start is at least end. The value stores endpoints; iteration creates a fresh cursor.",
            "fn main() -> bool {\n    val cursor = (2..4).iter();\n    cursor.next() == Some(2) && cursor.next() == Some(3) && cursor.next() == None &&\n        (4..2).iter().next() == None\n}",
        ),
        RangeKind::Inclusive => (
            "A closed range `start..=end`, including both endpoints. Integer iteration yields the last representable endpoint once and then stops without overflowing. Reversed ranges are empty.",
            "fn main() -> bool {\n    val cursor = (254u8..=255u8).iter();\n    cursor.next() == Some(254u8) && cursor.next() == Some(255u8) && cursor.next() == None\n}",
        ),
        RangeKind::From => (
            "A range `start..` with an included start and no end bound. Each iteration creates a fresh ascending integer cursor.\n\n# Traps\n\nOpen-ended iteration traps when advancing would exceed the integer type; it does not wrap or become a finite inclusive range. Use `take` or a bounded range when the consumer needs a finite sequence.",
            "use core::ops::Bound;\nfn main() -> bool {\n    val range = 2..;\n    val cursor = range.take(3u64);\n    range.end_bound() == Bound::Unbounded && cursor.next() == Some(2) &&\n        cursor.next() == Some(3) && cursor.next() == Some(4) && cursor.next() == None\n}",
        ),
        RangeKind::To => (
            "A range `..end` with no start bound and an excluded end. It implements RangeBounds, but has no integer iteration because no start value is specified.",
            "use core::ops::Bound;\nfn main() -> bool {\n    val range = ..4;\n    range.start_bound() == Bound::Unbounded && range.end_bound() == Bound::Excluded(4)\n}",
        ),
        RangeKind::ToInclusive => (
            "A range `..=end` with no start bound and an included end. It implements RangeBounds, but has no integer iteration because no start value is specified.",
            "use core::ops::Bound;\nfn main() -> bool {\n    val range = ..=4;\n    range.start_bound() == Bound::Unbounded && range.end_bound() == Bound::Included(4)\n}",
        ),
        RangeKind::Full => (
            "The fully unbounded range `..`. It has no endpoint element type of its own. RangeBounds can be selected for a caller's element type; this value is not an integer iterator.",
            "use core::ops::Bound;\nfn inspect(value: core::ops::RangeBounds<i32>) -> bool {\n    value.start_bound() == Bound::Unbounded && value.end_bound() == Bound::Unbounded\n}\nfn main() -> bool {\n    inspect(..)\n}",
        ),
    }
}

pub(super) const MODULE_DOCS: (&str, &str) = (
    "Operator contracts, range values and boundary policies. Operators dispatch through checked implementations and preserve evaluation order.",
    "fn main() -> bool {\n    2 + 3 == 5 && -2 + 3 == 1\n}",
);
