//! Builtin array contracts and direct native associations belong to core::array.
pub(super) mod operations;
use crate::{
    catalog::documentation,
    foundation::{
        Entry, arrays::operations as arrays, bound, interface, iteration::operations as iteration,
        map_type, override_entry, raw,
    },
};
use kagari_runtime::native::{
    assembly::draft::{ModuleDraft, TraitImplementation},
    binding::{Codec, NativeBinding, NativeResult},
    declarations::MethodDecl,
    types::Type,
};
use kagari_types::ty::Ty;

pub(super) fn declare(module: &mut ModuleDraft<'_>) -> NativeResult<()> {
    module.inherent_with(
        1,
        |arguments| array(arguments[0].clone()),
        |_, methods| {
            for (name, result, callback) in [
                ("len", Type::u64(), arrays::length as Entry),
                ("is_empty", Type::bool(), arrays::is_empty),
            ] {
                methods.define(
                    MethodDecl::instance(name)
                        .returns(result)
                        .documentation(documentation::example(
                            if name == "len" {
                                "Return the number of entries."
                            } else {
                                "Return whether the collection has no entries."
                            },
                            &format!(
                                "fn example(values: [i32]) {{ val result = values.{name}(); }}"
                            ),
                        ))
                        .implementation_send(unsafe {
                            NativeBinding::new_transport(vec![Codec::Value], Codec::Value, callback)
                        }),
                )?;
            }
            Ok(())
        },
    )?;
    let index = interface(module, "Index", 1)?;
    let output = module.reference_associated_type(&index, "Output", 0)?;
    let entry = override_entry(module, &index, "index", 0, raw(2, arrays::index))?;
    module.trait_implementation_with(1, |arguments| {
        Ok(TraitImplementation {
            receiver: array(arguments[0].clone())?,
            interface: output.bind(index.apply([Type::u64()])?, arguments[0].clone())?,
            bounds: vec![],
            bindings: vec![],
        }
        .with_overrides([entry]))
    })?;
    let iterable = interface(module, "Iterable", 0)?;
    let item = module.reference_associated_type(&iterable, "Item", 0)?;
    let cursor = module.reference_associated_type(&iterable, "Iter", 0)?;
    let entry = override_entry(module, &iterable, "iter", 0, raw(1, iteration::iter))?;
    module.trait_implementation_with(1, |arguments| {
        Ok(TraitImplementation {
            receiver: array(arguments[0].clone())?,
            interface: cursor.bind(
                item.bind(iterable.apply([])?, arguments[0].clone())?,
                Type::iterator(arguments[0].clone())?,
            )?,
            bounds: vec![],
            bindings: vec![],
        }
        .with_overrides([entry]))
    })?;
    let equal = interface(module, "PartialEq", 0)?;
    let member = module.reference_method(&equal, "eq", 0)?;
    let entry = override_entry(module, &equal, "eq", 0, raw(2, arrays::equal))?;
    module.trait_implementation_with(1, |arguments| {
        Ok(TraitImplementation {
            receiver: array(arguments[0].clone())?,
            interface: equal.apply([])?,
            bounds: vec![bound(&arguments[0], equal.apply([])?)],
            bindings: vec![],
        }
        .with_overrides([entry.requires(member.requirement(
            arguments[0].clone(),
            equal.apply([])?,
            [],
        )?)]))
    })
}

pub(super) fn array(item: Type) -> NativeResult<Type> {
    map_type(item, |item| Ty::Array(Box::new(item)))
}

pub(super) const MODULE_DOCS: (&str, &str) = (
    "Builtin shared arrays have construction-time fixed length and checked element access.",
    "fn length(values: Vec<i32>) -> u64 { values.len() }",
);
