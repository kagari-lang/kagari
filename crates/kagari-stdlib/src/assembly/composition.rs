//! Pure composition of canonical owners, their std facades and the explicit prelude.
//! Paths, explicit dependencies and presentation use the same checked draft as declarations.
use crate::{catalog::documentation, foundation, identity, namespaces};
use kagari_common::identity::{
    DefinitionKind, DefinitionPath, DefinitionPathSegment, ModuleIdentity,
};
use kagari_runtime::{
    error::RuntimeError,
    native::{
        assembly::{ModuleAssembly, ModuleDefinition, ModuleSetBuilder},
        binding::NativeResult,
        catalog::DeclarationCatalog,
    },
};
use kagari_types::declaration::names::{ExportName, NameNamespace};
use std::collections::BTreeMap;

#[derive(Default)]
struct Composition {
    documentation: String,
    exports: Vec<(ExportName, DefinitionPath)>,
    dependencies: Vec<ModuleIdentity>,
    prelude: bool,
}

pub(super) fn collect() -> NativeResult<ModuleAssembly> {
    let mut owners = BTreeMap::new();
    for &(package, path, docs) in foundation::MODULES {
        let identity = namespaces::module(package, path);
        let declaration = Composition {
            documentation: documentation::markdown(docs),
            ..Default::default()
        };
        if owners.insert(identity, declaration).is_some() {
            return Err(conflict("duplicate canonical module owner"));
        }
    }
    let canonical = owners.keys().cloned().collect::<Vec<_>>();
    let public = foundation::public_types();
    for owner in &canonical {
        if owner.package.0 != "std" {
            let facade = namespaces::module("std", &owner.path.join("::"));
            let declaration = Composition {
                documentation: owners[owner].documentation.clone(),
                ..Default::default()
            };
            if owners.insert(facade, declaration).is_some() {
                return Err(conflict("duplicate standard facade"));
            }
        }
    }
    for (owner, kind, name) in public {
        let target = DefinitionPath {
            module: owner.clone(),
            path: vec![DefinitionPathSegment {
                kind,
                name: name.clone(),
                occurrence: 0,
            }],
        };
        if owner.package.0 != "std" {
            let facade = owners
                .get_mut(&namespaces::module("std", &owner.path.join("::")))
                .expect("explicit facade");
            facade
                .exports
                .push((ExportName::new(NameNamespace::Type, name), target));
        }
    }
    let mut prelude = Composition {
        prelude: true,
        documentation: documentation::example(
            "Automatically imported standard names. Re-exports retain the canonical declaration identity and documentation of their owner.",
            "fn length(values: Vec<i32>) -> u64 { values.len() }",
        ),
        dependencies: canonical,
        ..Default::default()
    };
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
        prelude.exports.push((
            ExportName::new(NameNamespace::Type, name),
            identity::trait_id(name),
        ));
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
        prelude.exports.push((
            ExportName::new(NameNamespace::Type, name),
            identity::type_id(name, kind),
        ));
    }
    for (name, variants) in [("Option", ["Some", "None"]), ("Result", ["Ok", "Err"])] {
        let enumeration = identity::type_id(name, DefinitionKind::Enum);
        for variant in variants {
            let mut target = enumeration.clone();
            target.path.push(DefinitionPathSegment {
                kind: DefinitionKind::Variant,
                name: variant.into(),
                occurrence: 0,
            });
            prelude
                .exports
                .push((ExportName::new(NameNamespace::Value, variant), target));
        }
    }
    owners.insert(namespaces::prelude(), prelude);
    let mut set = ModuleSetBuilder::new(&DeclarationCatalog::default())?;
    for (owner, composition) in owners {
        set.add(ModuleDefinition::new(owner, move |module| {
            match module.identity().package.0.as_str() {
                "kagari-core" => module.package_alias("core"),
                "kagari-alloc" => module.package_alias("alloc"),
                _ => {}
            }
            module.documentation(composition.documentation);
            module.prelude(composition.prelude);
            for dependency in composition.dependencies {
                module.dependency(dependency)?;
            }
            for (name, target) in composition.exports {
                module.export_path(name, target)?;
            }
            foundation::contribute(module)
        }))?;
    }
    set.finish()
}

fn conflict(error: impl ToString) -> RuntimeError {
    RuntimeError::metadata_conflict(error.to_string())
}
