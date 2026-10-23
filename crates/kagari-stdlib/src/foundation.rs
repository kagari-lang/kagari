//! Explicit standard owner contributors; composition contains no implementation policy.
mod arrays;
mod character;
mod comparison;
pub(crate) mod enumerations;
mod future;
mod hash;
mod host;
mod iteration;
mod lists;
mod numeric;
mod propagation;
mod protocols;
mod ranges;
pub(crate) mod string;
mod tasks;
mod vector;

use crate::{catalog::documentation, identity, namespaces};
use kagari_common::identity::{DefinitionKind, ModuleIdentity, map::DefinitionContext};
use kagari_runtime::{
    error::RuntimeError,
    native::{
        assembly::{
            authoring::PendingBinding,
            draft::{ModuleDraft, TraitImplementation, traits::TraitOverride},
            references::DraftReference,
        },
        binding::{Codec, NativeBinding, NativeResult},
        context::CallContext,
        types::Type,
    },
    value::Value,
};
use kagari_types::{
    declaration::TypeDefKind,
    ty::{GenericBound, Ty},
};

type Entry = for<'call> fn(&mut CallContext<'call>) -> NativeResult<Value>;

pub(crate) fn contribute(module: &mut ModuleDraft<'_>) -> NativeResult<()> {
    protocols::declare(module)?;
    enumerations::declare(module)?;
    let owner = module.identity().clone();
    if owner == namespaces::module("alloc", "string") {
        string::declare(module)?;
    }
    if owner == namespaces::module("alloc", "vec") {
        vector::declare(module)?;
        comparison::concrete(module, "List")?;
    }
    if owner == namespaces::module("std", "collections") {
        hash::declare(module)?;
        lists::defaults(module)?;
        comparison::public_functions(module)?;
        comparison::concrete(module, "Set")?;
        comparison::concrete(module, "Map")?;
        comparison::interface_debug(module)?;
    }
    if owner == namespaces::module("core", "host") {
        host::declare(module)?;
    }
    if owner == namespaces::module("core", "future") {
        future::declare(module)?;
    }
    if owner == namespaces::module("core", "task") {
        tasks::declare(module)?;
    }
    if owner == namespaces::module("core", "iter") {
        iteration::declare(module)?;
    }
    if owner == namespaces::module("core", "char") {
        character::declare(module)?;
    }
    if owner == namespaces::module("core", "num") {
        numeric::declare(module)?;
    }
    if owner == namespaces::module("core", "array") {
        arrays::declare(module)?;
    }
    if owner == namespaces::module("core", "ops") {
        ranges::declare(module)?;
    }
    for name in ["Option", "Result", "ControlFlow"] {
        if owner == namespaces::type_owner(name) {
            propagation::declare(module, name)?;
        }
    }
    Ok(())
}

pub(crate) fn public_types() -> Vec<(ModuleIdentity, DefinitionKind, String)> {
    let mut types = enumerations::ENUMERATIONS
        .iter()
        .map(|item| {
            (
                namespaces::type_owner(item.name),
                DefinitionKind::Enum,
                item.name.to_owned(),
            )
        })
        .collect::<Vec<_>>();
    for &(package, path, declarations) in protocols::INVENTORY {
        types.extend(declarations.iter().map(|(name, _, _)| {
            (
                namespaces::module(package, path),
                DefinitionKind::Trait,
                (*name).into(),
            )
        }));
    }
    types.extend(
        [
            "HashMap",
            "HashSet",
            "Ref",
            "RefMut",
            "Future",
            "Task",
            "TaskScope",
        ]
        .into_iter()
        .map(|name| {
            (
                namespaces::type_owner(name),
                DefinitionKind::AssociatedType,
                name.into(),
            )
        }),
    );
    types.push((
        namespaces::type_owner("Vec"),
        DefinitionKind::AssociatedType,
        "Vec".into(),
    ));
    types.push((
        namespaces::type_owner("String"),
        DefinitionKind::AssociatedType,
        "String".into(),
    ));
    types.extend(iteration::ADAPTERS.iter().map(|(name, _, _)| {
        (
            namespaces::type_owner(name),
            DefinitionKind::AssociatedType,
            (*name).into(),
        )
    }));
    types.push((
        namespaces::type_owner("CollectionCursor"),
        DefinitionKind::AssociatedType,
        "CollectionCursor".into(),
    ));
    types.extend(ranges::KINDS.iter().map(|kind| {
        (
            namespaces::type_owner(kind.name()),
            DefinitionKind::AssociatedType,
            kind.name().into(),
        )
    }));
    types.push((
        namespaces::type_owner("SpawnError"),
        DefinitionKind::Enum,
        "SpawnError".into(),
    ));
    types
}

fn interface(
    module: &mut ModuleDraft<'_>,
    name: &str,
    arity: usize,
) -> NativeResult<DraftReference> {
    module.reference_trait(identity::trait_id(name), arity)
}

fn enumeration(
    module: &mut ModuleDraft<'_>,
    name: &str,
    arity: usize,
) -> NativeResult<DraftReference> {
    module.reference_type(
        identity::type_id(name, DefinitionKind::Enum),
        TypeDefKind::Enum,
        arity,
    )
}

fn raw(arguments: usize, entry: Entry) -> PendingBinding {
    PendingBinding::implementation_send(NativeBinding::new(
        vec![Codec::Value; arguments],
        Codec::Value,
        entry,
    ))
}

fn override_entry(
    module: &mut ModuleDraft<'_>,
    owner: &DraftReference,
    name: &str,
    arity: usize,
    binding: PendingBinding,
) -> NativeResult<TraitOverride> {
    let member = module.reference_method(owner, name, arity)?;
    Ok(TraitOverride::new(member, binding))
}

fn bound(ty: &Type, interface: Type) -> GenericBound {
    let Ty::Trait(interface) = interface.abi() else {
        unreachable!("trait application")
    };
    GenericBound {
        ty: ty.abi().clone(),
        constraints: vec![interface.clone()],
    }
}

fn map_type(item: Type, build: impl FnOnce(Ty) -> Ty) -> NativeResult<Type> {
    let scope = match item.definitions() {
        Some(scope) => scope.clone(),
        None => DefinitionContext::new().map_err(conflict)?.snapshot(),
    };
    Type::from_semantic(build(item.abi().clone()), &scope)
}

fn type_docs(
    module: &mut ModuleDraft<'_>,
    reference: &DraftReference,
    name: &str,
    document: (&str, &str),
    variants: &[&str],
) -> NativeResult<()> {
    module.document(reference.id(), documentation::markdown(document))?;
    let (description, _) = document;
    for variant in variants {
        let id = module.member(reference.id(), DefinitionKind::Variant, variant)?;
        module.document(
            id,
            format!("The `{variant}` case of `{name}`. {description}"),
        )?;
    }
    Ok(())
}

fn default_impl(
    module: &mut ModuleDraft<'_>,
    receiver: Type,
    binding: NativeBinding,
    description: &str,
) -> NativeResult<()> {
    let default = interface(module, "Default", 0)?;
    let entry = override_entry(
        module,
        &default,
        "default",
        0,
        PendingBinding::implementation_send(binding),
    )?
    .documentation(format!(
        "{description}\n\nSee `core::default::Default` for the construction contract."
    ));
    module.trait_implementation_with(0, |_| {
        Ok(TraitImplementation {
            receiver,
            interface: default.apply([])?,
            bounds: vec![],
            bindings: vec![],
        }
        .with_overrides([entry]))
    })
}

fn conflict(error: impl ToString) -> RuntimeError {
    RuntimeError::metadata_conflict(error.to_string())
}

/// Canonical module inventory; facades reuse their owner documentation.
pub(crate) const MODULES: &[(&str, &str, (&str, &str))] = &[
    ("core", "array", arrays::MODULE_DOCS),
    ("core", "char", character::MODULE_DOCS),
    ("core", "iter", iteration::MODULE_DOCS),
    ("core", "ops", ranges::MODULE_DOCS),
    ("core", "cmp", protocols::CMP_MODULE_DOCS),
    ("core", "hash", protocols::HASH_MODULE_DOCS),
    ("core", "fmt", protocols::FMT_MODULE_DOCS),
    ("core", "future", future::MODULE_DOCS),
    ("core", "task", tasks::MODULE_DOCS),
    ("core", "host", host::MODULE_DOCS),
    ("core", "convert", protocols::CONVERT_MODULE_DOCS),
    ("core", "default", protocols::DEFAULT_MODULE_DOCS),
    ("core", "num", numeric::MODULE_DOCS),
    ("core", "option", enumerations::OPTION_MODULE_DOCS),
    ("core", "result", enumerations::RESULT_MODULE_DOCS),
    ("alloc", "vec", vector::MODULE_DOCS),
    ("core", "str", protocols::STR_MODULE_DOCS),
    ("alloc", "string", string::MODULE_DOCS),
    ("std", "collections", hash::MODULE_DOCS),
];
