use crate::{
    Runtime,
    native::{
        binding::{Codec, NativeBinding},
        builder::ModuleBuilder,
        catalog::DeclarationCatalog,
        declarations::{FunctionDecl, MethodDecl},
        module::NativeModule,
        storage::NativeStorage,
        types::Type,
    },
    value::Value,
};
use kagari_common::identity::DefinitionPath;
use kagari_stdlib::catalog as standard_catalog;
use kagari_types::{
    callable::CallableImplementation,
    declaration::{
        TypeDefKind,
        names::{ExportName, NameNamespace},
        native::NativeStorageLayout,
    },
};
use std::collections::BTreeMap;

fn assert_catalog_eq(left: &DeclarationCatalog, right: &DeclarationCatalog) {
    assert_path_catalog_eq(&left.to_paths().unwrap(), &right.to_paths().unwrap());
}

fn assert_path_catalog_eq(
    left: &DeclarationCatalog<DefinitionPath>,
    right: &DeclarationCatalog<DefinitionPath>,
) {
    assert_eq!(left.types, right.types);
    assert_eq!(left.traits, right.traits);
    assert_eq!(left.declarations, right.declarations);
    assert_eq!(left.implementations, right.implementations);
}

#[test]
fn reused_closures_preserve_exact_foundation_binding_requirements() {
    let declarations = standard_catalog::declarations();
    let providers = DeclarationCatalog::from_declarations(&declarations).unwrap();
    let modules = declarations
        .into_iter()
        .map(|declaration| {
            let mut bindings = BTreeMap::new();
            for entry in declaration.native_declarations() {
                let CallableImplementation::Native(id) = entry.function.implementation else {
                    panic!("native template");
                };
                bindings.entry(id).or_insert_with(|| {
                    NativeBinding::new(
                        vec![Codec::Value; entry.function.params.len()],
                        Codec::Value,
                        |_| Ok(Value::Unit),
                    )
                });
            }
            let owned = DeclarationCatalog::declared([&declaration]).unwrap();
            let storage = owned
                .types
                .iter()
                .filter_map(|(id, ty)| {
                    let TypeDefKind::NativeStorage(layout) = ty.kind else {
                        return None;
                    };
                    let storage = match layout {
                        NativeStorageLayout::Future => NativeStorage::future(),
                        NativeStorageLayout::Sequence { element } => {
                            NativeStorage::sequence(element)
                        }
                        NativeStorageLayout::Task => NativeStorage::task(),
                        NativeStorageLayout::TaskScope => NativeStorage::task_scope(),
                        _ => panic!("unexpected foundation storage: {layout:?}"),
                    };
                    Some((id.clone(), storage))
                })
                .collect();
            NativeModule::checked(
                declaration,
                bindings.into_iter().collect(),
                storage,
                &providers,
            )
            .unwrap()
        })
        .collect::<Vec<_>>();
    for module in &modules {
        for registration in module.bindings.iter() {
            let declarations = registration
                .required_catalog
                .paths(&registration.declarations)
                .unwrap();
            let module_declaration = module.to_declaration().unwrap();
            let fresh = module
                .catalog
                .dependency_closure(
                    module.owned.traits.keys(),
                    &declarations,
                    [&module_declaration],
                )
                .unwrap();
            assert_path_catalog_eq(
                &registration.required_catalog.to_paths().unwrap(),
                &fresh.catalog,
            );
        }
    }
}

#[test]
fn binding_specific_foreign_dependencies_do_not_leak_to_other_bindings() {
    let providers = DeclarationCatalog::from_declarations(
        standard_catalog::shared()
            .iter()
            .map(|module| module.as_ref()),
    )
    .unwrap();
    let mut provider = ModuleBuilder::new("test::provider", &providers);
    let mut remote = provider.define_trait("Remote");
    remote.define_method(MethodDecl::instance("read")).unwrap();
    let remote = remote.finish().unwrap();
    let provider = provider.finish().unwrap();
    let mut consumer = ModuleBuilder::new("test::consumer", &providers)
        .with_modules(&[&provider])
        .unwrap();
    let inspect = consumer
        .define_function(FunctionDecl::new("inspect").parameter("value", remote.apply([]).ty()))
        .unwrap();
    consumer
        .bind_with(
            inspect,
            NativeBinding::new(vec![Codec::Value], Codec::Value, |_| Ok(Value::Unit)),
        )
        .unwrap();
    let local = consumer
        .define_function(FunctionDecl::new("local"))
        .unwrap();
    consumer
        .bind_with(
            local,
            NativeBinding::new(vec![], Codec::Value, |_| Ok(Value::Unit)),
        )
        .unwrap();
    let module = consumer.finish().unwrap();
    for registration in module.bindings.iter() {
        let declarations = registration
            .required_catalog
            .paths(&registration.declarations)
            .unwrap();
        let module_declaration = module.to_declaration().unwrap();
        let declaration = &declarations[0];
        let name = declaration.declaration.path.last().unwrap().name.as_str();
        assert_eq!(
            registration.required_catalog.traits.len(),
            usize::from(name == "inspect")
        );
        let fresh = module
            .catalog
            .dependency_closure(
                module.owned.traits.keys(),
                &declarations,
                [&module_declaration],
            )
            .unwrap();
        assert_path_catalog_eq(
            &registration.required_catalog.to_paths().unwrap(),
            &fresh.catalog,
        );
    }
    let mut runtime = Runtime::default();
    let before = runtime.native_entries.catalog.clone();
    assert!(module.install(&mut runtime).is_err());
    assert_catalog_eq(&runtime.native_entries.catalog, &before);
    provider.install(&mut runtime).unwrap();
    module.install(&mut runtime).unwrap();
}

fn application(names: &[&str]) -> NativeModule {
    let mut module = ModuleBuilder::new("test::application", &DeclarationCatalog::default());
    for name in names {
        let function = module
            .define_function(FunctionDecl::new(*name).returns(Type::i32()))
            .unwrap();
        module
            .bind_with(
                function,
                NativeBinding::new(vec![], Codec::Scalar(Type::i32().abi().clone()), |_| {
                    Ok(Value::I32(42))
                }),
            )
            .unwrap();
    }
    module.finish().unwrap()
}

#[test]
fn shared_registrations_keep_installation_and_failure_state_runtime_local() {
    let module = application(&["value"]);
    let mut first = Runtime::default();
    let mut second = Runtime::default();
    module.install(&mut first).unwrap();
    let before = first.native_entries.catalog.clone();
    assert!(
        application(&["added", "value"])
            .install(&mut first)
            .is_err()
    );
    assert_catalog_eq(&first.native_entries.catalog, &before);
    // A failed batch must not retain its earlier successful entry.
    application(&["added"]).install(&mut first).unwrap();
    module.install(&mut second).unwrap();
    assert!(module.install(&mut second).is_err());
    assert!(
        !second.native_entries.catalog.declarations.keys().any(|id| {
            id.path
                .last()
                .is_some_and(|segment| segment.name == "added")
        })
    );
    assert_eq!(first.gc().active_roots(), 0);
    assert_eq!(second.gc().active_roots(), 0);
}

#[test]
fn independently_scoped_catalogs_match_complete_contracts_after_import() {
    let first = application(&["value"]);
    let second = application(&["value"]);
    let first_id = first.declaration().functions[0].implementation.clone();
    let second_id = second.declaration().functions[0].implementation.clone();
    assert_ne!(first_id, second_id);
    let expected = first.to_declaration().unwrap();
    let merged = DeclarationCatalog::from_modules(&[&first, &second]).unwrap();
    assert_eq!(merged.declarations.len(), 1);
    assert!(first.catalog().satisfied_by(&merged).unwrap());
    assert!(second.catalog().satisfied_by(&merged).unwrap());
    let retained = merged.to_paths().unwrap();
    assert_eq!(
        retained.declarations.values().next().unwrap(),
        &expected.native_declarations()[0]
    );
    drop(first);
    drop(second);
    assert_eq!(
        merged
            .to_paths()
            .unwrap()
            .declarations
            .values()
            .next()
            .unwrap(),
        &expected.native_declarations()[0]
    );
    let mut incompatible = expected;
    incompatible.functions[0].return_type = Type::bool().abi().clone();
    let incompatible = DeclarationCatalog::declared([&incompatible]).unwrap();
    assert!(!incompatible.satisfied_by(&merged).unwrap());
    let mut rejected = merged.clone();
    assert!(rejected.merge(&incompatible).is_err());
    assert_catalog_eq(&rejected, &merged);
}

#[test]
fn registration_scope_survives_catalog_growth_and_module_drop() {
    let module = application(&["value"]);
    let registration = module.bindings[0].clone();
    let expected = registration
        .required_catalog
        .paths(&registration.declarations)
        .unwrap();
    let mut runtime = Runtime::default();
    module.install(&mut runtime).unwrap();
    drop(module);
    application(&["added", "later"])
        .install(&mut runtime)
        .unwrap();
    assert_eq!(
        registration
            .required_catalog
            .paths(&registration.declarations)
            .unwrap(),
        expected
    );
    let installed = runtime.native_entries.catalog.to_paths().unwrap();
    assert!(
        installed
            .declarations
            .values()
            .any(|record| record == &expected[0])
    );
    let declarations = registration.required_catalog.definitions();
    assert!(
        runtime
            .native_entries
            .catalog
            .definitions()
            .resolve(registration.declarations[0].declaration)
            .is_err()
    );
    assert!(
        declarations
            .resolve(registration.declarations[0].declaration)
            .is_ok()
    );
}

#[test]
fn installed_reexports_require_declared_and_installed_canonical_targets() {
    use kagari_common::identity::{DefinitionKind, DefinitionPathSegment};
    use std::collections::BTreeMap;
    use {
        kagari_stdlib::namespaces,
        kagari_types::{declaration::module::ModuleDecl, language, language::Protocol},
    };

    let providers = DeclarationCatalog::from_declarations(
        standard_catalog::shared()
            .iter()
            .map(|module| module.as_ref()),
    )
    .unwrap();
    let mut declaration = ModuleDecl::new(namespaces::module("test", "exports"));
    declaration.exports.insert(
        ExportName::new(NameNamespace::Type, "Hash"),
        language::identity(Protocol::Hash),
    );
    declaration.exports.insert(
        ExportName::new(NameNamespace::Type, "Vec"),
        ModuleDecl::new(namespaces::type_owner("Vec"))
            .definition(DefinitionKind::AssociatedType, "Vec"),
    );
    let mut some = ModuleDecl::new(namespaces::type_owner("Option"))
        .definition(DefinitionKind::Enum, "Option");
    some.path.push(DefinitionPathSegment {
        kind: DefinitionKind::Variant,
        name: "Some".into(),
        occurrence: 0,
    });
    declaration
        .exports
        .insert(ExportName::new(NameNamespace::Value, "Some"), some.clone());
    // One alias may denote a type and a value; category mismatches are invalid metadata.
    declaration
        .exports
        .insert(ExportName::new(NameNamespace::Value, "Vec"), some.clone());
    let mut wrong_category = declaration.clone();
    wrong_category
        .exports
        .insert(ExportName::new(NameNamespace::Type, "Variant"), some);
    assert!(NativeModule::checked(wrong_category, vec![], BTreeMap::new(), &providers).is_err());
    assert!(
        NativeModule::checked(
            declaration.clone(),
            vec![],
            BTreeMap::new(),
            &Default::default()
        )
        .is_err()
    );
    let module =
        NativeModule::checked(declaration.clone(), vec![], BTreeMap::new(), &providers).unwrap();
    assert_eq!(
        module.to_declaration().unwrap().exports,
        declaration.exports
    );
    let mut empty = Default::default();
    assert!(module.install_into(&mut empty).is_err());
    assert!(empty.catalog.traits.is_empty() && empty.catalog.types.is_empty());
    let mut runtime = Runtime::default();
    for declaration in standard_catalog::shared() {
        runtime
            .native_entries
            .catalog
            .merge(&DeclarationCatalog::declared([declaration.as_ref()]).unwrap())
            .unwrap();
    }
    module.install(&mut runtime).unwrap();

    for name in ["Hash", "Vec", "Some"] {
        let mut forged = declaration.clone();
        forged
            .exports
            .get_mut(&ExportName::new(
                if name == "Some" {
                    NameNamespace::Value
                } else {
                    NameNamespace::Type
                },
                name,
            ))
            .unwrap()
            .path
            .last_mut()
            .unwrap()
            .name = "Missing".into();
        assert!(
            NativeModule::checked(forged, vec![], BTreeMap::new(), &providers).is_err(),
            "{name}"
        );
    }
}
