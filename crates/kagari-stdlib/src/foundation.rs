//! Explicit SR02 owner contributors. Unmigrated declarations join through raw seeds.
mod arrays;
pub(crate) mod enumerations;
mod numeric;
mod propagation;
mod ranges;
pub(crate) mod string;

use crate::{bindings::Entry, catalog::documentation, identity, namespaces};
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
        types::Type,
    },
};
use kagari_types::{
    declaration::TypeDefKind,
    ty::{GenericBound, Ty},
};

pub(crate) fn contribute(module: &mut ModuleDraft<'_>) -> NativeResult<()> {
    enumerations::declare(module)?;
    let owner = module.identity().clone();
    if owner == namespaces::module("alloc", "string") {
        string::declare(module)?;
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
    types.push((
        namespaces::type_owner("String"),
        DefinitionKind::AssociatedType,
        "String".into(),
    ));
    types.extend(ranges::KINDS.iter().map(|kind| {
        (
            namespaces::type_owner(kind.name()),
            DefinitionKind::AssociatedType,
            kind.name().into(),
        )
    }));
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
    let contract = module
        .definitions()
        .resolve(owner.id())
        .map_err(conflict)?
        .last()
        .expect("trait identity")
        .name
        .to_owned();
    Ok(TraitOverride::new(member, binding)
        .documentation(documentation::implementation_docs(&contract, name)))
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
    variants: &[&str],
) -> NativeResult<()> {
    module.document(reference.id(), documentation::type_markdown(name))?;
    let (description, _) = documentation::type_docs(name);
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
