mod lifecycle;
mod scalar_targets;

use super::*;
use crate::native::{
    adapter::{Explicit, NativeAdapter},
    assembly::authoring::PendingBinding,
    binding::{Codec, NativeBinding},
    declarations::{FunctionDecl, MethodDecl},
    primitive::NativePrimitive,
    types::Type,
};
use kagari_common::{
    cancellation::CancellationToken,
    identity::{DefinitionKind, DefinitionPath, DefinitionPathSegment, PackageId},
};
use kagari_types::{
    declaration::{
        FnDecl, TypeDefKind, VariantDef,
        names::{ExportName, NameNamespace},
        native::NativeTypeConstructor,
    },
    scalar::BuiltinType,
};
use std::{
    cell::{Cell, RefCell},
    rc::Rc,
    sync::atomic::{AtomicUsize, Ordering},
};

fn owner(name: &str) -> ModuleIdentity {
    ModuleIdentity {
        package: PackageId("assembly-test".into()),
        path: vec![name.into()],
    }
}

fn path(module: &str, kind: DefinitionKind, name: &str) -> DefinitionPath {
    DefinitionPath {
        module: owner(module),
        path: vec![DefinitionPathSegment {
            kind,
            name: name.into(),
            occurrence: 0,
        }],
    }
}

fn empty_binding() -> PendingBinding {
    PendingBinding::implementation(NativeBinding::new(
        vec![],
        Codec::Exact(Type::unit()),
        |_| panic!("callback must not run"),
    ))
}

fn enumeration(module: &mut ModuleDraft<'_>, name: &str) -> NativeResult<()> {
    module.define_type(name, TypeDefKind::Enum, 0, |record, _| {
        record.variants.push(VariantDef {
            name: "Value".into(),
            payload: vec![],
            reports_failure: false,
        });
        Ok(())
    })?;
    Ok(())
}

#[test]
fn declaration_projection_never_prepares_factories_and_native_projection_is_identical() {
    let recipes = Arc::new(AtomicUsize::new(0));
    let factories = Arc::new(AtomicUsize::new(0));
    let mut set = ModuleSetBuilder::new(&DeclarationCatalog::default()).unwrap();
    let recipe_count = recipes.clone();
    let factory_count = factories.clone();
    set.add(ModuleDefinition::new(owner("string"), move |module| {
        recipe_count.fetch_add(1, Ordering::SeqCst);
        module.documentation("String owner");
        module.package_alias("test");
        module.prelude(true);
        module.define_constructor("String", NativeTypeConstructor::String)?;
        module.inherent(Type::scalar(BuiltinType::String), |methods| {
            assert!(
                methods
                    .define(
                        MethodDecl::instance("len")
                            .returns(Type::u64())
                            .configure(|_| Err(conflict("reject this submission")))
                            .configure(|_| panic!("configuration must stop at the first error"))
                            .implementation(NativeBinding::primitive(
                                NativePrimitive::StringByteLength
                            )),
                    )
                    .is_err()
            );
            methods.define(
                MethodDecl::instance("len")
                    .returns(Type::u64())
                    .documentation("Byte length")
                    .implementation(NativeBinding::primitive(NativePrimitive::StringByteLength)),
            )?;
            assert!(
                methods
                    .define(
                        MethodDecl::instance("len")
                            .returns(Type::u64())
                            .implementation(NativeBinding::primitive(
                                NativePrimitive::StringByteLength
                            )),
                    )
                    .is_err()
            );
            methods.define(
                MethodDecl::instance("accepts")
                    .parameter("flag", Type::bool())
                    .returns(Type::bool())
                    .implementation_with(move |_| {
                        factory_count.fetch_add(1, Ordering::SeqCst);
                        Ok(NativeBinding::new(
                            vec![
                                Codec::Exact(Type::scalar(BuiltinType::String)),
                                Codec::Exact(Type::bool()),
                            ],
                            Codec::Exact(Type::bool()),
                            |_| panic!("callback must not run"),
                        ))
                    }),
            )?;
            Ok(())
        })
    }))
    .unwrap();
    let assembly = set.finish().unwrap();
    let snapshot = assembly.declarations();
    assert_eq!(recipes.load(Ordering::SeqCst), 1);
    assert_eq!(factories.load(Ordering::SeqCst), 0);
    let before = snapshot.modules()[0]
        .records()
        .compiler_input_bytes(
            snapshot.modules()[0].definitions(),
            &CancellationToken::default(),
        )
        .unwrap();
    let modules = assembly.into_modules().unwrap();
    assert_eq!(factories.load(Ordering::SeqCst), 1);
    assert_eq!(recipes.load(Ordering::SeqCst), 1);
    assert_eq!(
        before,
        modules[0]
            .declaration()
            .compiler_input_bytes(modules[0].definitions(), &CancellationToken::default())
            .unwrap()
    );
    assert_eq!(
        snapshot.modules()[0].records().module_documentation,
        "String owner"
    );
    assert!(snapshot.modules()[0].records().prelude);
}

struct LocalPreparation {
    local: Rc<Cell<usize>>,
    prepared: Arc<AtomicUsize>,
}

impl NativeAdapter<Explicit, ()> for LocalPreparation {
    fn prepare(self, _: &FnDecl, _: &DeclarationCatalog) -> NativeResult<NativeBinding> {
        self.local.set(self.local.get() + 1);
        self.prepared.fetch_add(1, Ordering::SeqCst);
        Ok(NativeBinding::new(
            vec![],
            Codec::Exact(Type::unit()),
            |_| panic!("callback must not run"),
        ))
    }
}

#[test]
fn direct_and_factory_adapters_can_own_thread_local_preparation_state() {
    let prepared = Arc::new(AtomicUsize::new(0));
    let factories = Arc::new(AtomicUsize::new(0));
    let prepare_count = prepared.clone();
    let factory_count = factories.clone();
    let mut set = ModuleSetBuilder::new(&DeclarationCatalog::default()).unwrap();
    set.add(ModuleDefinition::new(
        owner("local_preparation"),
        move |module| {
            module.define_constructor("String", NativeTypeConstructor::String)?;
            let local = Rc::new(Cell::new(0));
            module.inherent(Type::scalar(BuiltinType::String), |methods| {
                methods.define(MethodDecl::static_method("direct").implementation(
                    LocalPreparation {
                        local: local.clone(),
                        prepared: prepare_count.clone(),
                    },
                ))?;
                methods.define(MethodDecl::static_method("factory").implementation_with(
                    move |_| {
                        factory_count.fetch_add(1, Ordering::SeqCst);
                        Ok(LocalPreparation {
                            local,
                            prepared: prepare_count,
                        })
                    },
                ))?;
                Ok(())
            })
        },
    ))
    .unwrap();
    let assembly = set.finish().unwrap();
    let snapshot = assembly.declarations();
    assert_eq!(prepared.load(Ordering::SeqCst), 0);
    assert_eq!(factories.load(Ordering::SeqCst), 0);
    assert_eq!(
        snapshot.modules()[0].records().implementations[0]
            .methods
            .len(),
        2
    );
    assert_eq!(assembly.into_modules().unwrap().len(), 1);
    assert_eq!(prepared.load(Ordering::SeqCst), 2);
    assert_eq!(factories.load(Ordering::SeqCst), 1);
}

#[test]
fn fluent_configuration_composes_once_after_signature_setters() {
    let mut set = ModuleSetBuilder::new(&DeclarationCatalog::default()).unwrap();
    set.add(ModuleDefinition::new(owner("configuration"), |module| {
        module.define_constructor("String", NativeTypeConstructor::String)?;
        let calls = Rc::new(RefCell::new(vec![]));
        let first_calls = calls.clone();
        let second_calls = calls.clone();
        module.inherent(Type::scalar(BuiltinType::String), |methods| {
            methods.define(
                MethodDecl::static_method("configured")
                    .configure(move |function| {
                        first_calls.borrow_mut().push("first");
                        function.parameter("configured", Type::i32())?;
                        function.returns(Type::bool())
                    })
                    .parameter("declared", Type::bool())
                    .returns(Type::unit())
                    .documentation("Configuration follows declarative setters.")
                    .configure(move |function| {
                        second_calls.borrow_mut().push("second");
                        function.parameter("last", Type::u64())
                    })
                    .implementation(NativeBinding::new(
                        vec![
                            Codec::Exact(Type::bool()),
                            Codec::Exact(Type::i32()),
                            Codec::Exact(Type::u64()),
                        ],
                        Codec::Exact(Type::bool()),
                        |_| panic!("callback must not run"),
                    )),
            )?;
            assert_eq!(*calls.borrow(), ["first", "second"]);
            Ok(())
        })
    }))
    .unwrap();
    let assembly = set.finish().unwrap();
    let snapshot = assembly.declarations();
    let record = snapshot.modules()[0].records();
    let method = &record.implementations[0].methods[0];
    assert_eq!(
        method
            .params
            .iter()
            .map(|parameter| parameter.name.as_str())
            .collect::<Vec<_>>(),
        ["declared", "configured", "last"]
    );
    assert_eq!(&method.return_type, Type::bool().abi());
    assert!(
        record
            .documentation
            .values()
            .any(|text| text == "Configuration follows declarative setters.")
    );
    assert!(assembly.into_modules().is_ok());
}

#[test]
fn reciprocal_type_references_resolve_in_either_recipe_order() {
    fn recipe(name: &'static str, other: &'static str) -> ModuleDefinition {
        ModuleDefinition::new(owner(name), move |module| {
            let reference = module.reference_type(
                path(other, DefinitionKind::Enum, "Node"),
                TypeDefKind::Enum,
                0,
            )?;
            let payload = reference.apply([])?;
            module.define_type("Node", TypeDefKind::Enum, 0, |record, _| {
                record.variants.push(VariantDef {
                    name: "Next".into(),
                    payload: vec![payload.abi().clone()],
                    reports_failure: false,
                });
                Ok(())
            })?;
            Ok(())
        })
    }
    for order in [
        [("left", "right"), ("right", "left")],
        [("right", "left"), ("left", "right")],
    ] {
        let mut set = ModuleSetBuilder::new(&DeclarationCatalog::default()).unwrap();
        for (name, other) in order {
            set.add(recipe(name, other)).unwrap();
        }
        let assembly = set.finish().unwrap();
        let snapshot = assembly.declarations();
        assert_eq!(snapshot.modules().len(), 2);
        assert!(
            snapshot
                .modules()
                .iter()
                .all(|module| module.records().dependencies.len() == 1)
        );
        assert_eq!(assembly.into_modules().unwrap().len(), 2);
    }
}

#[test]
fn unresolved_category_arity_and_missing_exports_reject_before_preparation() {
    for case in 0..4 {
        let mut set = ModuleSetBuilder::new(&DeclarationCatalog::default()).unwrap();
        set.add(ModuleDefinition::new(owner("references"), move |module| {
            match case {
                0 => {
                    module.reference_type(
                        path("absent", DefinitionKind::Enum, "Missing"),
                        TypeDefKind::Enum,
                        0,
                    )?;
                }
                1 => {
                    module.reference_type(
                        path("references", DefinitionKind::Enum, "Node"),
                        TypeDefKind::Enum,
                        1,
                    )?;
                    enumeration(module, "Node")?;
                }
                2 => {
                    module.reference_trait(path("references", DefinitionKind::Trait, "Node"), 0)?;
                    enumeration(module, "Node")?;
                }
                _ => {
                    let id = module.definition(DefinitionKind::Enum, "Missing")?;
                    module.export(
                        ExportName {
                            namespace: NameNamespace::Type,
                            name: "Alias".into(),
                        },
                        id,
                    )?;
                }
            }
            module.define_function((
                FunctionDecl::new("unused"),
                PendingBinding::implementation_with(|_| -> NativeResult<NativeBinding> {
                    panic!("must not prepare an invalid assembly")
                }),
            ))?;
            Ok(())
        }))
        .unwrap();
        assert!(set.finish().is_err(), "case {case}");
    }
}

#[test]
fn duplicate_owners_members_and_foreign_receiver_are_rejected() {
    let mut set = ModuleSetBuilder::new(&DeclarationCatalog::default()).unwrap();
    set.add(ModuleDefinition::new(owner("duplicate"), |_| Ok(())))
        .unwrap();
    assert!(
        set.add(ModuleDefinition::new(owner("duplicate"), |_| Ok(())))
            .is_err()
    );
    let mut set = ModuleSetBuilder::new(&DeclarationCatalog::default()).unwrap();
    set.add(ModuleDefinition::new(owner("local"), |module| {
        enumeration(module, "Node")?;
        assert!(enumeration(module, "Node").is_err());
        module.define_function((FunctionDecl::new("f"), empty_binding()))?;
        assert!(
            module
                .define_function((FunctionDecl::new("f"), empty_binding()))
                .is_err()
        );
        Ok(())
    }))
    .unwrap();
    assert!(set.finish().unwrap().into_modules().is_ok());
    let mut set = ModuleSetBuilder::new(&DeclarationCatalog::default()).unwrap();
    set.add(ModuleDefinition::new(owner("foreign"), |module| {
        let node = module.reference_type(
            path("local", DefinitionKind::Enum, "Node"),
            TypeDefKind::Enum,
            0,
        )?;
        module.inherent(node.apply([])?, |methods| {
            methods.define(
                MethodDecl::static_method("bad").implementation(NativeBinding::new(
                    vec![],
                    Codec::Exact(Type::unit()),
                    |_| panic!("not called"),
                )),
            )?;
            Ok(())
        })
    }))
    .unwrap();
    set.add(ModuleDefinition::new(owner("local"), |module| {
        enumeration(module, "Node")
    }))
    .unwrap();
    assert!(set.finish().is_err());
}

#[test]
fn external_providers_are_adopted_but_cannot_be_redefined() {
    let mut providers = ModuleSetBuilder::new(&DeclarationCatalog::default()).unwrap();
    providers
        .add(ModuleDefinition::new(owner("provider"), |module| {
            enumeration(module, "Node")
        }))
        .unwrap();
    let provider = providers.finish().unwrap().declarations();
    let mut set = ModuleSetBuilder::new(provider.catalog()).unwrap();
    assert!(
        set.add(ModuleDefinition::new(owner("provider"), |_| Ok(())))
            .is_err()
    );
    let foreign = provider
        .catalog()
        .type_reference(&path("provider", DefinitionKind::Enum, "Node"))
        .unwrap()
        .id();
    set.add(ModuleDefinition::new(owner("consumer"), move |module| {
        assert!(
            module
                .export(
                    ExportName {
                        namespace: NameNamespace::Type,
                        name: "ForeignScope".into()
                    },
                    foreign
                )
                .is_err()
        );
        let local = module.reference_type(
            path("provider", DefinitionKind::Enum, "Node"),
            TypeDefKind::Enum,
            0,
        )?;
        module.export(
            ExportName {
                namespace: NameNamespace::Type,
                name: "Node".into(),
            },
            local.id(),
        )?;
        Ok(())
    }))
    .unwrap();
    let assembly = set.finish().unwrap();
    assert_eq!(
        assembly.declarations().modules()[0].records().dependencies,
        BTreeSet::from([owner("provider")])
    );
    assert!(assembly.into_modules().is_ok());
}

#[test]
fn signature_mismatch_fails_native_projection_without_invalidating_snapshot() {
    let mut set = ModuleSetBuilder::new(&DeclarationCatalog::default()).unwrap();
    set.add(ModuleDefinition::new(owner("mismatch"), |module| {
        module.define_function((
            FunctionDecl::new("f").returns(Type::bool()),
            empty_binding(),
        ))?;
        Ok(())
    }))
    .unwrap();
    let assembly = set.finish().unwrap();
    let snapshot = assembly.declarations();
    assert!(assembly.into_modules().is_err());
    assert_eq!(
        snapshot.modules()[0].records().functions[0].return_type,
        Type::bool().abi().clone()
    );
}
