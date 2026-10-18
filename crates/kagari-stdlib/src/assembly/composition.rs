//! Pure public composition over the explicit new and remaining contributor inventory.
//! No body lookup or semantic lowering occurs here. SR05 removes only the raw seed bridge.
use crate::{
    bindings,
    catalog::{self, documentation},
    foundation, identity, namespaces,
};
use kagari_common::{
    cancellation::CancellationToken,
    identity::{DefinitionKind, map::DefinitionContext, metadata::DefinitionMetadata},
};
use kagari_runtime::{
    error::RuntimeError,
    native::{
        assembly::{ModuleAssembly, ModuleDefinition, ModuleSetBuilder},
        binding::NativeResult,
        catalog::DeclarationCatalog,
    },
};
use kagari_types::declaration::{
    module::ModuleDecl,
    names::{ExportName, NameNamespace},
};
use std::collections::BTreeMap;

pub(super) fn collect() -> NativeResult<ModuleAssembly> {
    let context = DefinitionContext::new().map_err(conflict)?;
    let cancel = CancellationToken::default();
    let mut owners = BTreeMap::new();
    for seed in catalog::remaining().map_err(conflict)? {
        let seed = seed
            .import_into(&context, &cancel)
            .map_err(conflict)?
            .into_records();
        if owners.insert(seed.identity.clone(), seed).is_some() {
            return Err(conflict("duplicate legacy owner"));
        }
    }
    for (package, names) in [
        (
            "core",
            &[
                "array", "cmp", "convert", "default", "fmt", "future", "hash", "host", "iter",
                "num", "ops", "option", "result", "str", "task",
            ][..],
        ),
        ("alloc", &["string", "vec"][..]),
        ("std", &["collections"][..]),
    ] {
        for name in names {
            owners
                .entry(namespaces::module(package, name))
                .or_insert_with(|| ModuleDecl::new(namespaces::module(package, name)));
        }
    }
    let canonical = owners.keys().cloned().collect::<Vec<_>>();
    let mut public = Vec::new();
    for module in owners.values() {
        for ty in &module.types {
            public.push((
                module.identity.clone(),
                ty.kind.definition_kind(),
                ty.name.clone(),
            ));
        }
        for contract in &module.traits {
            public.push((
                module.identity.clone(),
                DefinitionKind::Trait,
                contract.name.clone(),
            ));
        }
    }
    public.extend(foundation::public_types());
    for owner in &canonical {
        context.intern_root(owner).map_err(conflict)?;
        if owner.package.0 != "std" {
            let facade = namespaces::module("std", &owner.path.join("::"));
            owners
                .entry(facade.clone())
                .or_insert_with(|| ModuleDecl::new(facade));
        }
    }
    for (owner, kind, name) in public {
        let root = context.intern_root(&owner).map_err(conflict)?;
        let id = context
            .intern_child(root, kind, &name, 0)
            .map_err(conflict)?;
        if owner.package.0 != "std" {
            let facade = owners
                .get_mut(&namespaces::module("std", &owner.path.join("::")))
                .expect("explicit facade");
            if facade
                .exports
                .insert(ExportName::new(NameNamespace::Type, name), id)
                .is_some()
            {
                return Err(conflict("duplicate public contributor"));
            }
        }
    }
    let mut prelude = ModuleDecl::new(namespaces::prelude());
    prelude.prelude = true;
    prelude.dependencies.extend(canonical);
    for name in [
        "Iterator",
        "Iterable",
        "FromIterator",
        "Default",
        "PartialEq",
        "Eq",
        "PartialOrd",
        "Ord",
        "From",
        "Into",
        "TryFrom",
        "TryInto",
        "Fn",
    ] {
        prelude.exports.insert(
            ExportName::new(NameNamespace::Type, name),
            context
                .intern(&identity::trait_id(name))
                .map_err(conflict)?,
        );
    }
    for (name, kind) in [
        ("String", DefinitionKind::AssociatedType),
        ("Ref", DefinitionKind::AssociatedType),
        ("RefMut", DefinitionKind::AssociatedType),
        ("Vec", DefinitionKind::AssociatedType),
        ("Option", DefinitionKind::Enum),
        ("Result", DefinitionKind::Enum),
        ("Future", DefinitionKind::AssociatedType),
        ("Task", DefinitionKind::AssociatedType),
        ("TaskScope", DefinitionKind::AssociatedType),
        ("SpawnError", DefinitionKind::Enum),
    ] {
        prelude.exports.insert(
            ExportName::new(NameNamespace::Type, name),
            context
                .intern(&identity::type_id(name, kind))
                .map_err(conflict)?,
        );
    }
    for (name, variants) in [("Option", ["Some", "None"]), ("Result", ["Ok", "Err"])] {
        let id = context
            .intern(&identity::type_id(name, DefinitionKind::Enum))
            .map_err(conflict)?;
        for variant in variants {
            prelude.exports.insert(
                ExportName::new(NameNamespace::Value, variant),
                context
                    .intern_child(id, DefinitionKind::Variant, variant, 0)
                    .map_err(conflict)?,
            );
        }
    }
    owners.insert(prelude.identity.clone(), prelude);
    let mut set = ModuleSetBuilder::new(&DeclarationCatalog::default())?;
    for (owner, mut seed) in owners {
        seed.package_alias = match owner.package.0.as_str() {
            "kagari-core" => Some("core".into()),
            "kagari-alloc" => Some("alloc".into()),
            _ => None,
        };
        let definitions = seed.collect_definitions(&context).map_err(conflict)?;
        documentation::complete(&definitions, &mut seed).map_err(conflict)?;
        let seed = DefinitionMetadata::checked(definitions, seed, &cancel).map_err(conflict)?;
        set.add(ModuleDefinition::new(owner, move |module| {
            bindings::collect(module, seed)?;
            foundation::contribute(module)
        }))?;
    }
    set.finish()
}

fn conflict(error: impl ToString) -> RuntimeError {
    RuntimeError::metadata_conflict(error.to_string())
}
