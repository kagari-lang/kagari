use crate::{
    Runtime,
    host::{
        HostCallContext, HostError, HostObjectId, HostPathAdapter, HostSchemaEpoch,
        PreparedHostPathWrite,
    },
    metadata::{TypeKind, TypeRegistration},
    native::{
        binding::{Codec, NativeBinding, NativeResult},
        builder::ModuleBuilder,
        catalog::{DeclarationCatalog, import::CatalogImports},
        declarations::{FunctionDecl, MethodDecl},
        installation::NativeInstallation,
        module::NativeModule,
        storage::{NativePayload, NativeStorage},
        types::Type,
    },
    resource::HeapCharge,
    value::Value,
};
use kagari_common::identity::{DefinitionKind, metadata::DefinitionMetadata};
use kagari_stdlib::{catalog as standard_catalog, namespaces};
use kagari_types::{
    callable::CallableImplementation,
    declaration::path::{HostPathDeclaration, HostPathSegmentDeclaration},
    declaration::{
        FieldDef, TypeDefKind,
        host::{
            HostCallPolicy, HostFieldPolicy, HostPassingStyle, HostReflectionPolicy,
            HostTypeOwnership, HostTypePolicy, PathAccess,
        },
        module::ModuleDecl,
        names::{ExportName, NameNamespace},
        native::NativeStorageLayout,
    },
    language::Protocol,
    scalar::BuiltinType,
    ty::Ty,
    visibility::Visibility,
};
use std::{collections::BTreeMap, slice};

#[derive(Debug)]
struct CatalogOnlyPayload;

impl NativePayload for CatalogOnlyPayload {
    fn trace<'payload>(&'payload self, _visit: &mut dyn FnMut(&'payload Value)) {}

    fn charge(&self) -> NativeResult<HeapCharge> {
        Ok(HeapCharge::new(0, 0))
    }
}

fn assert_catalog_eq(left: &DeclarationCatalog, right: &DeclarationCatalog) {
    let mut imports = CatalogImports::new(left.types.context().clone());
    let right = imports.import(right).unwrap();
    assert_eq!(left.types, right.types);
    assert_eq!(left.traits, right.traits);
    assert_eq!(left.declarations, right.declarations);
    assert_eq!(left.implementations, right.implementations);
    assert_eq!(left.paths, right.paths);
}

#[test]
fn reused_closures_preserve_exact_foundation_binding_requirements() {
    let declarations = standard_catalog::declarations().unwrap();
    let providers = DeclarationCatalog::from_declarations(&declarations).unwrap();
    let modules = declarations
        .into_iter()
        .map(|declaration| {
            let context = providers.types.context();
            let declaration = declaration
                .import_into(context, &Default::default())
                .unwrap()
                .into_records();
            let mut bindings = BTreeMap::new();
            for entry in declaration
                .native_declarations(&providers.definitions())
                .unwrap()
            {
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
            let owned =
                DeclarationCatalog::declared([&declaration], providers.types.context()).unwrap();
            let storage = owned
                .types
                .entries()
                .filter_map(|(id, ty)| {
                    let TypeDefKind::NativeStorage(layout) = ty.kind else {
                        return None;
                    };
                    let storage = match layout {
                        NativeStorageLayout::Opaque => {
                            NativeStorage::payload::<CatalogOnlyPayload>()
                        }
                        NativeStorageLayout::Future => NativeStorage::future(),
                        NativeStorageLayout::Sequence { element } => {
                            NativeStorage::sequence(element)
                        }
                        NativeStorageLayout::Task => NativeStorage::task(),
                        NativeStorageLayout::TaskScope => NativeStorage::task_scope(),
                        _ => panic!("unexpected foundation storage: {layout:?}"),
                    };
                    Some((id, storage))
                })
                .collect();
            NativeModule::checked(
                declaration,
                bindings.into_iter().collect(),
                storage,
                vec![],
                &providers,
            )
            .unwrap()
        })
        .collect::<Vec<_>>();
    for module in &modules {
        for registration in module.bindings.iter() {
            let declarations = &registration.declarations;
            let module_declaration = module.declaration();
            let fresh = module
                .catalog
                .dependency_closure(
                    module.owned.traits.ids(),
                    declarations,
                    [module_declaration],
                )
                .unwrap();
            assert_catalog_eq(&registration.required_catalog, &fresh.catalog);
        }
    }
}

#[test]
fn binding_specific_foreign_dependencies_do_not_leak_to_other_bindings() {
    let providers = DeclarationCatalog::from_declarations(
        standard_catalog::shared()
            .unwrap()
            .iter()
            .map(|module| module.as_ref()),
    )
    .unwrap();
    let mut provider = ModuleBuilder::new("test::provider", &providers).unwrap();
    let mut remote = provider.define_trait("Remote").unwrap();
    remote.define_method(MethodDecl::instance("read")).unwrap();
    let remote = remote.finish().unwrap();
    let provider = provider.finish().unwrap();
    let mut consumer = ModuleBuilder::new("test::consumer", &providers)
        .unwrap()
        .with_modules(&[&provider])
        .unwrap();
    let inspect = consumer
        .define_function(
            FunctionDecl::new("inspect").parameter("value", remote.apply([]).unwrap().ty()),
        )
        .unwrap();
    consumer
        .bind(
            inspect,
            NativeBinding::new(vec![Codec::Value], Codec::Value, |_| Ok(Value::Unit)),
        )
        .unwrap();
    let local = consumer
        .define_function(FunctionDecl::new("local"))
        .unwrap();
    consumer
        .bind(
            local,
            NativeBinding::new(vec![], Codec::Value, |_| Ok(Value::Unit)),
        )
        .unwrap();
    let module = consumer.finish().unwrap();
    for registration in module.bindings.iter() {
        let declarations = &registration.declarations;
        let module_declaration = module.declaration();
        let declaration = &declarations[0];
        let definitions = registration.required_catalog.definitions();
        let name = definitions
            .resolve(declaration.declaration)
            .unwrap()
            .last()
            .unwrap()
            .name;
        assert_eq!(
            registration.required_catalog.traits.len(),
            usize::from(name == "inspect")
        );
        let fresh = module
            .catalog
            .dependency_closure(
                module.owned.traits.ids(),
                declarations,
                [module_declaration],
            )
            .unwrap();
        assert_catalog_eq(&registration.required_catalog, &fresh.catalog);
    }
    let mut runtime = Runtime::default();
    let before = runtime.native_entries.catalog.clone();
    assert!(module.install(&mut runtime).is_err());
    assert!(NativeInstallation::prepare(slice::from_ref(&module)).is_err());
    assert_catalog_eq(&runtime.native_entries.catalog, &before);
    provider.install(&mut runtime).unwrap();
    module.install(&mut runtime).unwrap();
}

fn application(names: &[&str]) -> NativeModule {
    let mut module =
        ModuleBuilder::new("test::application", &DeclarationCatalog::default()).unwrap();
    for name in names {
        let function = module
            .define_function(FunctionDecl::new(*name).returns(Type::i32()))
            .unwrap();
        module
            .bind(
                function,
                NativeBinding::new(vec![], Codec::Exact(Type::i32()), |_| Ok(Value::I32(42))),
            )
            .unwrap();
    }
    module.finish().unwrap()
}

#[test]
fn shared_registrations_keep_installation_and_failure_state_runtime_local() {
    let module = application(&["value"]);
    let installation = NativeInstallation::prepare(slice::from_ref(&module)).unwrap();
    let mut first = installation.runtime(Default::default());
    let mut second = installation.runtime(Default::default());
    let local = first
        .types
        .register(TypeRegistration::new("runtime-config", TypeKind::Primitive))
        .unwrap();
    let before = first.native_entries.catalog.clone();
    assert!(
        application(&["added", "value"])
            .install(&mut first)
            .is_err()
    );
    assert_catalog_eq(&first.native_entries.catalog, &before);
    assert!(first.types.get(local).is_some());
    // A failed batch must not retain its earlier successful entry.
    application(&["added"]).install(&mut first).unwrap();
    assert!(first.types.get(local).is_some());
    assert!(second.types.get_by_name("runtime-config").is_none());
    assert!(module.install(&mut second).is_err());
    assert!(!second.native_entries.catalog.declarations.ids().any(|id| {
        second
            .native_entries
            .catalog
            .definitions()
            .resolve(id)
            .unwrap()
            .last()
            .is_some_and(|segment| segment.name == "added")
    }));
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
    let expected = first.to_declaration();
    let merged = DeclarationCatalog::from_modules(&[&first, &second]).unwrap();
    assert_eq!(merged.declarations.len(), 1);
    assert!(first.catalog().satisfied_by(&merged).unwrap());
    assert!(second.catalog().satisfied_by(&merged).unwrap());
    let retained = merged.declarations.values().next().unwrap().clone();
    let expected = expected
        .import_into(merged.types.context(), &Default::default())
        .unwrap();
    let expected_scoped = expected
        .records()
        .native_declarations(expected.definitions())
        .unwrap()
        .remove(0);
    assert_eq!(retained, expected_scoped);
    drop(first);
    drop(second);
    assert_eq!(
        merged.declarations.values().next().unwrap(),
        &expected_scoped
    );
    let mut incompatible = expected.into_records();
    incompatible.functions[0].return_type = Type::bool().abi().clone();
    let incompatible =
        DeclarationCatalog::declared([&incompatible], merged.types.context()).unwrap();
    assert!(!incompatible.satisfied_by(&merged).unwrap());
    let mut rejected = merged.clone();
    assert!(rejected.merge(&incompatible).is_err());
    assert_catalog_eq(&rejected, &merged);
}

#[test]
fn registration_scope_survives_catalog_growth_and_module_drop() {
    let module = application(&["value"]);
    let registration = module.bindings[0].clone();
    let expected = registration.declarations.clone();
    let mut runtime = Runtime::default();
    module.install(&mut runtime).unwrap();
    drop(module);
    application(&["added", "later"])
        .install(&mut runtime)
        .unwrap();
    assert_eq!(registration.declarations, expected);
    let installed = &runtime.native_entries.catalog;
    let expected = DefinitionMetadata::checked(
        registration.required_catalog.definitions(),
        expected,
        &Default::default(),
    )
    .unwrap()
    .import_into(installed.types.context(), &Default::default())
    .unwrap();
    assert!(
        installed
            .declarations
            .values()
            .any(|record| record == &expected.records()[0])
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
    let providers = DeclarationCatalog::from_declarations(
        standard_catalog::shared()
            .unwrap()
            .iter()
            .map(|module| module.as_ref()),
    )
    .unwrap();
    let context = providers.types.context();
    let mut declaration = ModuleDecl::new(namespaces::module("test", "exports"));
    declaration.exports.insert(
        ExportName::new(NameNamespace::Type, "Hash"),
        Protocol::Hash.definition(&providers.definitions()).unwrap(),
    );
    declaration.exports.insert(
        ExportName::new(NameNamespace::Type, "Vec"),
        ModuleDecl::new(namespaces::type_owner("Vec"))
            .definition(
                DefinitionKind::AssociatedType,
                "Vec",
                &providers.definitions(),
            )
            .unwrap(),
    );
    let option = ModuleDecl::new(namespaces::type_owner("Option"))
        .definition(DefinitionKind::Enum, "Option", &providers.definitions())
        .unwrap();
    let some = context
        .intern_child(option, DefinitionKind::Variant, "Some", 0)
        .unwrap();
    declaration
        .exports
        .insert(ExportName::new(NameNamespace::Value, "Some"), some);
    // One alias may denote a type and a value; category mismatches are invalid metadata.
    declaration
        .exports
        .insert(ExportName::new(NameNamespace::Value, "Vec"), some);
    let mut wrong_category = declaration.clone();
    wrong_category
        .exports
        .insert(ExportName::new(NameNamespace::Type, "Variant"), some);
    assert!(
        NativeModule::checked(wrong_category, vec![], BTreeMap::new(), vec![], &providers).is_err()
    );
    assert!(
        NativeModule::checked(
            declaration.clone(),
            vec![],
            BTreeMap::new(),
            vec![],
            &Default::default()
        )
        .is_err()
    );
    let module = NativeModule::checked(
        declaration.clone(),
        vec![],
        BTreeMap::new(),
        vec![],
        &providers,
    )
    .unwrap();
    assert_eq!(
        module.to_declaration().records().exports,
        declaration.exports
    );
    let mut empty = Default::default();
    assert!(NativeModule::install_batch(slice::from_ref(&module), &mut empty).is_err());
    assert!(empty.catalog.traits.is_empty() && empty.catalog.types.is_empty());
    let mut runtime = Runtime::default();
    runtime.native_entries.catalog.merge(&providers).unwrap();
    module.install(&mut runtime).unwrap();

    for name in ["Hash", "Vec", "Some"] {
        let mut forged = declaration.clone();
        let target = forged
            .exports
            .get_mut(&ExportName::new(
                if name == "Some" {
                    NameNamespace::Value
                } else {
                    NameNamespace::Type
                },
                name,
            ))
            .unwrap();
        let mut path = context.snapshot().resolve(*target).unwrap().to_path();
        path.path.last_mut().unwrap().name = "Missing".into();
        *target = context.intern(&path).unwrap();
        assert!(
            NativeModule::checked(forged, vec![], BTreeMap::new(), vec![], &providers).is_err(),
            "{name}"
        );
    }
}

#[test]
fn external_authoring_uses_common_type_and_member_binding_policies() {
    fn callback(_: &HostCallContext<'_>, _: &[Value]) -> Result<Value, HostError> {
        Ok(Value::I32(0))
    }

    let policy = |passing| HostCallPolicy {
        passing: vec![passing],
        effects: Default::default(),
    };
    let mut module = ModuleBuilder::new("example::roots", &Default::default()).unwrap();
    let field = DefinitionMetadata::checked(
        module.declarations().definitions(),
        FieldDef {
            name: "hp".into(),
            ty: Ty::Builtin(BuiltinType::I32),
            visibility: Visibility::Public,
            mutable: true,
            host_policy: Some(HostFieldPolicy {
                readable: true,
                path_access: PathAccess::ReadWrite,
            }),
        },
        &Default::default(),
    )
    .unwrap();
    let mut player = module.define_type("Player").unwrap();
    player
        .external(HostTypePolicy {
            ownership: HostTypeOwnership::HostRoot,
            reflection: HostReflectionPolicy::Metadata,
            path_access: PathAccess::ReadWrite,
        })
        .unwrap();
    assert!(player.type_parameter("T").is_err());
    assert!(
        player
            .native_storage(NativeStorage::payload::<CatalogOnlyPayload>())
            .is_err()
    );
    player.field(field).unwrap();
    let player = player.finish().unwrap();
    let receiver = player.apply([]).unwrap();
    let definition = player.id();
    assert!(matches!(receiver.abi(), Ty::Host(_)));
    let mut interface = module.define_trait("HasHp").unwrap();
    interface
        .define_method(MethodDecl::instance("hp").returns(Type::i32()))
        .unwrap();
    let interface = interface.finish().unwrap();
    module
        .implement(player, |group| {
            group.inherent_impl(|methods| {
                let method = methods.define_method(
                    MethodDecl::instance("heal")
                        .host_policy(policy(HostPassingStyle::UniqueBorrow))
                        .returns(Type::i32()),
                )?;
                methods.bind(method, callback)
            })?;
            group.trait_impl(interface.apply([])?, |methods| {
                methods.host_policy("hp", policy(HostPassingStyle::SharedBorrow))?;
                assert!(
                    methods
                        .bind(
                            "hp",
                            NativeBinding::new(
                                vec![Codec::Value],
                                Codec::Exact(Type::i32()),
                                |_| Ok(Value::I32(0))
                            )
                        )
                        .is_err()
                );
                methods.bind("hp", callback)?;
                assert!(
                    methods
                        .host_policy("hp", policy(HostPassingStyle::UniqueBorrow))
                        .is_err()
                );
                Ok(())
            })
        })
        .unwrap();
    let definitions = module.declarations().definitions();
    let owners = module.providers.receiver_owners(None).unwrap();
    module
        .declaration
        .validate(
            &|receiver| owners.owner(receiver, &definitions),
            &definitions,
        )
        .unwrap();
    let catalog = DeclarationCatalog::declared([&module.declaration], module.context()).unwrap();
    catalog
        .check_implementations([&module.declaration])
        .unwrap();
    catalog.validate_callable_contracts().unwrap();
    for declaration in catalog.declarations.values() {
        assert_eq!(declaration.function.params[0].ty, *receiver.abi());
        assert!(declaration.function.host_policy.is_some());
    }
    let field = definitions
        .lookup_child(definition, DefinitionKind::Field, "hp", 0)
        .unwrap();
    let path = module
        .define_path(
            DefinitionMetadata::checked(
                definitions,
                HostPathDeclaration {
                    root: definition,
                    segments: vec![HostPathSegmentDeclaration::Field(field)],
                    access: PathAccess::ReadWrite,
                    schema_epoch: 2,
                },
                &Default::default(),
            )
            .unwrap(),
        )
        .unwrap();
    assert!(
        module
            .bind_path(
                &path,
                HostPathAdapter::new().with_read(|_, _| Ok(Value::I32(0)))
            )
            .is_err()
    );
    module
        .bind_path(
            &path,
            HostPathAdapter::new()
                .with_read(|_, _| Ok(Value::I32(0)))
                .with_prepare_write(|_, _, _| Ok(PreparedHostPathWrite::new(|| {}))),
        )
        .unwrap();
    let module = module.finish().unwrap();
    let installation = NativeInstallation::prepare(slice::from_ref(&module)).unwrap();
    let mut first = installation.runtime(Default::default());
    let second = installation.runtime(Default::default());
    let ty = first
        .types()
        .get_by_name("example::roots::Player")
        .unwrap()
        .id;
    let root = first
        .register_host_root(HostObjectId(9), ty, HostSchemaEpoch::new(2))
        .unwrap();
    assert!(second.host().root(HostObjectId(9)).is_none());
    assert_eq!(first.host().path_descriptors().count(), 1);
    assert_eq!(second.host().path_descriptors().count(), 1);
    assert!(!second.host().matches_root(root));
    let mut conflicting = Runtime::default();
    conflicting
        .types
        .register(TypeRegistration::new(
            "example::roots::Player",
            TypeKind::Primitive,
        ))
        .unwrap();
    let before = conflicting.native_entries.catalog.clone();
    assert!(module.install(&mut conflicting).is_err());
    assert_catalog_eq(&conflicting.native_entries.catalog, &before);
    assert_eq!(conflicting.host().path_descriptors().count(), 0);
    assert!(
        conflicting
            .types
            .get_by_name("example::roots::HasHp")
            .is_none()
    );
}
