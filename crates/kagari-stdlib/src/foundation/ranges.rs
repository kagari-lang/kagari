//! Range constructors and receiver-owned implementations; iterator algorithms remain shared.
// Preserve module children during Runtime-only test source sharing.
#[cfg_attr(test, path = "ranges/operations.rs")]
pub(super) mod operations;
use crate::foundation::{enumerations::value as enums, interface, override_entry, type_docs};
use kagari_runtime::native::{
    assembly::draft::{ModuleDraft, TraitImplementation},
    binding::NativeResult,
};
use kagari_types::{declaration::native::NativeTypeConstructor, range::RangeKind};

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
    for kind in KINDS {
        let range = module.define_constructor(kind.name(), NativeTypeConstructor::Range(kind))?;
        type_docs(module, &range, kind.name(), TYPE_DOCS, &[])?;
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
    }
    Ok(())
}

const TYPE_DOCS: (&str, &str) = (
    "A range value carrying its declared endpoints and inclusion policy. Open bounds are available through RangeBounds; concrete cursor providers are installed separately.",
    "fn lower() -> core::ops::Bound<i32> { (1..4).start_bound() }",
);

pub(super) const MODULE_DOCS: (&str, &str) = (
    "Operator contracts, range values and boundary policies. Operators dispatch through checked implementations and preserve evaluation order.",
    "fn plus(left: i32, right: i32) -> i32 { left + right }",
);
