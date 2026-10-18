//! Range constructors and receiver-owned implementations; iterator algorithms remain shared.
use crate::{
    bindings,
    foundation::{interface, override_entry, raw, type_docs},
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
        type_docs(module, &range, kind.name(), &[])?;
        let start = override_entry(
            module,
            &bounds,
            "start_bound",
            0,
            raw(1, bindings::start_bound),
        )?;
        let end = override_entry(module, &bounds, "end_bound", 0, raw(1, bindings::end_bound))?;
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
            let entry = override_entry(module, &iterable, "iter", 0, raw(1, bindings::iter))?;
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
