//! Pure composition of canonical owners, their std facades and the explicit prelude.
//! Only exports use the raw metadata import bridge; all declarations are authored recipes.
use crate::{catalog::documentation, foundation, identity, namespaces};
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
    for &(package, path, docs) in foundation::MODULES {
        let identity = namespaces::module(package, path);
        let mut declaration = ModuleDecl::new(identity.clone());
        declaration.module_documentation = documentation::markdown(docs);
        if owners.insert(identity, declaration).is_some() {
            return Err(conflict("duplicate canonical module owner"));
        }
    }
    let canonical = owners.keys().cloned().collect::<Vec<_>>();
    let public = foundation::public_types();
    for owner in &canonical {
        context.intern_root(owner).map_err(conflict)?;
        if owner.package.0 != "std" {
            let facade = namespaces::module("std", &owner.path.join("::"));
            let mut declaration = ModuleDecl::new(facade.clone());
            declaration.module_documentation = owners[owner].module_documentation.clone();
            if owners.insert(facade, declaration).is_some() {
                return Err(conflict("duplicate standard facade"));
            }
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
    prelude.module_documentation = documentation::example(
        "Automatically imported standard names. Re-exports retain the canonical declaration identity and documentation of their owner.",
        "fn length(values: Vec<i32>) -> u64 { values.len() }",
    );
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

        let seed = DefinitionMetadata::checked(definitions, seed, &cancel).map_err(conflict)?;
        set.add(ModuleDefinition::new(owner, move |module| {
            module.import_declaration(&seed, Default::default())?;
            foundation::contribute(module)
        }))?;
    }
    set.finish()
}

fn conflict(error: impl ToString) -> RuntimeError {
    RuntimeError::metadata_conflict(error.to_string())
}
