//! Explicit standard owner contributors; composition contains no implementation policy.
mod arrays;
mod character;
mod comparison;
pub(crate) mod enumerations;
mod numeric;
mod protocols;
pub(crate) mod string;
mod tasks;

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
        context::NativeCall,
        types::Type,
    },
    value::view::ValueView,
};
use kagari_types::{
    declaration::TypeDefKind,
    ty::{GenericBound, Ty},
};

type Entry = for<'call> fn(&mut NativeCall<'call>) -> NativeResult<ValueView<'call>>;

pub(crate) fn contribute(module: &mut ModuleDraft<'_>) -> NativeResult<()> {
    protocols::declare(module)?;
    enumerations::declare(module)?;
    let owner = module.identity().clone();
    if owner == namespaces::module("std", "string") {
        string::declare(module)?;
    }
    if owner == namespaces::module("std", "collections") {
        comparison::public_functions(module)?;
        comparison::interface_debug(module)?;
    }

    if owner == namespaces::module("core", "task") {
        tasks::declare(module)?;
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
    types.extend(["Task", "TaskScope"].into_iter().map(|name| {
        (
            namespaces::type_owner(name),
            DefinitionKind::AssociatedType,
            name.into(),
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

/// Native canonical owners only; std facades reuse their documentation.
pub(crate) const MODULES: &[(&str, &str, (&str, &str))] = &[
    ("core", "array", arrays::MODULE_DOCS),
    ("core", "char", character::MODULE_DOCS),
    ("core", "task", tasks::MODULE_DOCS),
    ("core", "num", numeric::MODULE_DOCS),
    (
        "std",
        "collections",
        (
            "Collection interface declarations and checked comparisons. Concrete storage providers are installed separately.",
            "fn length(values: std::collections::List<i32>) -> u64 { values.len() }",
        ),
    ),
];
