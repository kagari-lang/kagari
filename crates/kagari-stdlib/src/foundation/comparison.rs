//! Collection comparison contracts bind the shared algorithms by semantic operation.
pub(super) mod operations;
use crate::{
    foundation::{
        bound,
        comparison::operations::{self as comparison, Operation, Shape},
        conflict, interface, override_entry,
    },
    identity,
};
use kagari_common::{cancellation::CancellationToken, identity::DefinitionKind};
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
use kagari_types::{
    collection::CollectionAccess,
    declaration::{
        TypeDefKind,
        native::{NativeStorageLayout, NativeTypeConstructor},
    },
    ty::{GenericBound, Ty},
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
                .implementation_with_send(move |view| comparison::binding(shape, Operation::InterfaceEqual, view.declaration())),
        )?;
    }
    Ok(())
}

pub(super) fn concrete(module: &mut ModuleDraft<'_>, collection: &str) -> NativeResult<()> {
    let (shape, name, kind, arity) = match collection {
        "List" => (
            Shape::List,
            "Vec",
            TypeDefKind::NativeStorage(NativeStorageLayout::Sequence { element: 0 }),
            1,
        ),
        "Set" => (
            Shape::Set,
            "HashSet",
            TypeDefKind::Native(NativeTypeConstructor::Set),
            1,
        ),
        "Map" => (
            Shape::Map,
            "HashMap",
            TypeDefKind::Native(NativeTypeConstructor::Map),
            2,
        ),
        _ => unreachable!("collection comparison contributor"),
    };
    let receiver = module.reference_type(
        identity::type_id(name, DefinitionKind::AssociatedType),
        kind,
        arity,
    )?;
    let collection = interface(module, collection, arity)?;
    let len = module.reference_method(&collection, "len", 0)?;
    let iterable = interface(module, "Iterable", 0)?;
    let iterable_item = module.reference_associated_type(&iterable, "Item", 0)?;
    let iterable_cursor = module.reference_associated_type(&iterable, "Iter", 0)?;
    let iter = module.reference_method(&iterable, "iter", 0)?;
    let iterator = interface(module, "Iterator", 0)?;
    let iterator_item = module.reference_associated_type(&iterator, "Item", 0)?;
    let next = module.reference_method(&iterator, "next", 0)?;
    let eq = interface(module, "Eq", 0)?;
    let hash = interface(module, "Hash", 0)?;
    let partial_eq = interface(module, "PartialEq", 0)?;
    for access in [CollectionAccess::Mutable, CollectionAccess::ReadOnly] {
        if matches!(shape, Shape::List) && access == CollectionAccess::ReadOnly {
            continue;
        }
        for protocol_name in ["PartialEq", "Eq", "Hash"] {
            let protocol = interface(module, protocol_name, 0)?;
            let operation = if protocol_name == "Hash" {
                Operation::Hash
            } else {
                Operation::Equal
            };
            let member = match protocol_name {
                "PartialEq" => Some("eq"),
                "Hash" => Some("hash"),
                _ => None,
            };
            let entry = member
                .map(|name| {
                    override_entry(
                        module,
                        &protocol,
                        name,
                        0,
                        PendingBinding::implementation_with_send(move |view| {
                            comparison::binding(shape, operation, view.declaration())
                        }),
                    )
                })
                .transpose()?;
            let selected = member
                .map(|name| module.reference_method(&protocol, name, 0))
                .transpose()?;

            module.trait_implementation_with(arity, |items| {
                let mut receiver = receiver.apply(items.to_vec())?;
                if access == CollectionAccess::ReadOnly {
                    let ty = match receiver.abi().clone() {
                        Ty::Set(item, _) => Ty::Set(item, access),
                        Ty::Map { key, value, .. } => Ty::Map { key, value, access },
                        _ => unreachable!("builtin hash receiver"),
                    };
                    receiver =
                        Type::from_semantic(ty, receiver.definitions().expect("generic scope"))?;
                }
                let hash_bounds = |item: &Type| -> NativeResult<_> {
                    [&eq, &hash, &partial_eq]
                        .into_iter()
                        .map(|protocol| Ok(bound(item, protocol.apply([])?)))
                        .collect::<NativeResult<Vec<_>>>()
                };
                let mut bounds = if matches!(shape, Shape::List) {
                    vec![]
                } else {
                    hash_bounds(&items[0])?
                };
                if !matches!(shape, Shape::Set) {
                    let item = items.last().expect("collection element");
                    if protocol_name == "Hash" {
                        bounds.extend(hash_bounds(item)?);
                    } else {
                        bounds.push(bound(item, protocol.apply([])?));
                    }
                }
                let mut overrides = vec![];
                if let (Some(mut entry), Some(selected)) = (entry, selected) {
                    for item in items {
                        entry = entry.requires(selected.requirement(
                            item.clone(),
                            protocol.apply([])?,
                            [],
                        )?);
                    }
                    entry = entry.requires(len.requirement(
                        receiver.clone(),
                        collection.apply(items.to_vec())?,
                        [],
                    )?);
                    let item = if matches!(shape, Shape::Map) {
                        Type::tuple(items.to_vec())?
                    } else {
                        items[0].clone()
                    };
                    let cursor = Type::iterator(item.clone())?;
                    entry = entry.requires(iter.requirement(
                        receiver.clone(),
                        iterable_cursor.bind(
                            iterable_item.bind(iterable.apply([])?, item.clone())?,
                            cursor.clone(),
                        )?,
                        [],
                    )?);
                    entry = entry.requires(next.requirement(
                        cursor,
                        iterator_item.bind(iterator.apply([])?, item)?,
                        [],
                    )?);
                    overrides.push(entry);
                }
                let bounds = GenericBound::normalize(
                    bounds,
                    items[0].definitions().expect("implementation scope"),
                    &CancellationToken::default(),
                )
                .map_err(|error| conflict(format!("comparison bounds: {error:?}")))?;
                Ok(TraitImplementation {
                    receiver,
                    interface: protocol.apply([])?,
                    bounds,
                    bindings: vec![],
                }
                .with_overrides(overrides))
            })?;
        }
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
