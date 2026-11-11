//! Collection comparison contracts bind the shared algorithms by semantic operation.
pub(super) mod operations;
use crate::foundation::{
    comparison::operations::{self as comparison, Shape},
    interface, override_entry,
};
use kagari_runtime::native::{
    assembly::{
        authoring::PendingBinding,
        draft::{ModuleDraft, TraitImplementation},
    },
    binding::{NativeBinding, NativeResult},
    declarations::FunctionDecl,
    interfaces::Interface,
    types::Type,
};

pub(super) fn public_functions(module: &mut ModuleDraft<'_>) -> NativeResult<()> {
    for (family, collection, shape, names) in [
        ("list", "List", Shape::List, &["T"][..]),
        ("set", "Set", Shape::Set, &["T"][..]),
        ("map", "Map", Shape::Map, &["K", "V"][..]),
    ] {
        let collection = interface(module, collection, names.len())?;
        let partial_eq = interface(module, "PartialEq", 0)?;
        let key_bound = interface(module, if family == "list" { "PartialEq" } else { "Eq" }, 0)?;
        let eq = module.reference_method(&partial_eq, "eq", 0)?;
        module.define_function(
            FunctionDecl::new(format!("{family}_content_equal"))
                .returns(Type::bool())
                .documentation(format!("Compare {family} contents across compatible implementations using element protocols. Receiver object equality and lookup policies do not change this comparison. Traversal preserves completed callback effects and iterator invalidation."))
                .configure(move |function| {
                    let items = names.iter().map(|name| function.type_parameter(*name).map(|parameter| parameter.ty())).collect::<NativeResult<Vec<_>>>()?;
                    let receiver = collection.apply(items.clone())?;
                    function.parameter("left", receiver.clone())?;
                    function.parameter("right", receiver)?;
                    function.bound_draft(items[0].clone(), key_bound.apply([])?)?;
                    if let Some(value) = items.get(1) {
                        function.bound_draft(value.clone(), partial_eq.apply([])?)?;
                    }
                    for item in items {
                        function.requires_draft(eq.requirement(item, partial_eq.apply([])?, [])?)?;
                    }
                    Ok(())
                })
                .implementation_with_send(move |view| comparison::binding(shape, view.declaration())),
        )?;
    }
    Ok(())
}

pub(super) fn interface_debug(module: &mut ModuleDraft<'_>) -> NativeResult<()> {
    let debug = interface(module, "Debug", 0)?;
    for (name, arity) in [
        ("List", 1),
        ("MutableList", 1),
        ("Set", 1),
        ("MutableSet", 1),
        ("Map", 2),
        ("MutableMap", 2),
    ] {
        let collection = interface(module, name, arity)?;
        let entry = override_entry(
            module,
            &debug,
            "debug",
            0,
            PendingBinding::implementation_with_send(|view| {
                Ok(NativeBinding::declared(
                    view.declaration(),
                    |cx, (value,): (Interface,)| value.debug_preview(cx),
                ))
            }),
        )?;
        module.trait_implementation_with(arity, |items| {
            Ok(TraitImplementation {
                receiver: collection.apply(items.to_vec())?,
                interface: debug.apply([])?,
                bounds: vec![],
                bindings: vec![],
            }
            .with_overrides([entry]))
        })?;
    }
    Ok(())
}
