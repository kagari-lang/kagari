//! Nominal result codecs preserve scoped identity and checked executable results.
use crate::{
    Runtime,
    error::RuntimeErrorKind,
    frame::types::{EnvironmentRecord, operations::OperationBindings},
    native::{
        binding::{Codec, NativeBinding},
        builder::ModuleBuilder,
        catalog::DeclarationCatalog,
        declarations::FunctionDecl,
        module::NativeModule,
        types::{Type, TypeRef},
    },
    test_support::native::{invoke::invoke, program::ProgramFixture},
    value::Value,
};
use kagari_bytecode::{
    instruction::NativeImportId,
    module::BytecodeModule,
    program::{BytecodeProgram, ModuleRef},
};
use kagari_common::identity::{DefinitionKind, metadata::DefinitionMetadata};
use kagari_contract::{
    layout::{EnumLayout, EnumVariantLayout},
    native_import::NativeImport,
    types::{ConcreteFunctionIdentity, PublicItem},
};
use kagari_types::{
    callable::{CallableImplementation, Signature},
    declaration::TypeDefKind,
    scalar::BuiltinType,
    ty::{GenericParam, Ty},
};
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

fn enum_provider(name: &str) -> (NativeModule, TypeRef) {
    let mut builder = ModuleBuilder::new(name, &DeclarationCatalog::default()).unwrap();
    let mut choice = builder.define_enum("Choice").unwrap();
    let item = choice.type_parameter("T").unwrap();
    choice.variant("Some", [item.ty()]).unwrap();
    choice.variant("None", []).unwrap();
    let choice = choice.finish().unwrap();
    (builder.finish().unwrap(), choice)
}

#[test]
fn nominal_enum_codec_checks_scoped_identity_arity_and_catalog_presence() {
    let (provider, choice) = enum_provider("codec::provider");
    let (equivalent, same_choice) = enum_provider("codec::provider");
    let (foreign, other_choice) = enum_provider("codec::foreign");
    let catalog = DeclarationCatalog::from_modules(&[&provider]).unwrap();
    let equivalent_catalog = DeclarationCatalog::from_modules(&[&equivalent]).unwrap();
    let foreign_catalog = DeclarationCatalog::from_modules(&[&foreign]).unwrap();
    let codec = Codec::NominalEnum {
        declaration: choice.clone(),
        arity: 1,
    };
    for child in [Type::i32(), Type::bool()] {
        let applied = same_choice
            .apply([child])
            .unwrap()
            .adopt(equivalent_catalog.types.context())
            .unwrap();
        assert!(codec.accepts(&applied, &equivalent_catalog.definitions(), &catalog));
        assert!(!codec.accepts(&applied, &foreign_catalog.definitions(), &catalog));
        let absent = DeclarationCatalog::in_context(catalog.types.context().clone());
        assert!(!codec.accepts(&applied, &equivalent_catalog.definitions(), &absent));
        assert!(!codec.accepts(
            &applied,
            &equivalent_catalog.definitions(),
            &foreign_catalog
        ));
        let mut wrong_arity = applied.clone();
        let Ty::Enum(nominal) = &mut wrong_arity else {
            unreachable!()
        };
        nominal.arguments.clear();
        assert!(!codec.accepts(&wrong_arity, &equivalent_catalog.definitions(), &catalog));
        assert!(
            !Codec::NominalEnum {
                declaration: choice.clone(),
                arity: 0,
            }
            .accepts(&applied, &equivalent_catalog.definitions(), &catalog)
        );
    }
    let foreign_type = other_choice
        .apply([Type::i32()])
        .unwrap()
        .adopt(foreign_catalog.types.context())
        .unwrap();
    assert!(!codec.accepts(&foreign_type, &foreign_catalog.definitions(), &catalog));
    assert!(!codec.accepts(
        &Ty::Builtin(BuiltinType::I32),
        &catalog.definitions(),
        &catalog
    ));
    let mut wrong_kind = catalog.clone();
    let id = wrong_kind
        .definitions()
        .lookup(&choice.definitions().resolve(choice.id()).unwrap().to_path())
        .unwrap();
    let mut record = wrong_kind.types.get_id(id).unwrap().clone();
    record.kind = TypeDefKind::Struct;
    Arc::make_mut(&mut wrong_kind.types)
        .insert_id(id, record)
        .unwrap();
    let applied = choice
        .apply([Type::i32()])
        .unwrap()
        .adopt(catalog.types.context())
        .unwrap();
    assert!(!codec.accepts(&applied, &catalog.definitions(), &wrong_kind));
    assert!(codec.receiver_shape_matches(&Codec::NominalEnum {
        declaration: same_choice,
        arity: 1,
    }));
    assert!(!codec.receiver_shape_matches(&Codec::NominalEnum {
        declaration: other_choice,
        arity: 1,
    }));
}

fn result_module() -> (NativeModule, Arc<AtomicUsize>) {
    let mut builder = ModuleBuilder::new("codec::results", &DeclarationCatalog::default()).unwrap();
    let mut choice = builder.define_enum("Choice").unwrap();
    let item = choice.type_parameter("T").unwrap();
    let some = choice.variant("Some", [item.ty()]).unwrap();
    let none = choice.variant("None", []).unwrap();
    let choice = choice.finish().unwrap();
    let calls = Arc::new(AtomicUsize::new(0));
    for (name, declared, wrong_child, empty) in [
        ("number", Type::i32(), false, false),
        ("flag", Type::bool(), false, false),
        ("empty", Type::i32(), false, true),
        ("wrong_some", Type::i32(), true, false),
        ("wrong_none", Type::i32(), true, true),
    ] {
        let function = builder
            .define_function(FunctionDecl::new(name).returns(choice.apply([declared]).unwrap()))
            .unwrap();
        let calls = calls.clone();
        let boolean = choice.apply([Type::bool()]).unwrap();
        let variant = if empty { none.clone() } else { some.clone() };
        builder
            .bind(
                function,
                NativeBinding::new(
                    [],
                    Codec::NominalEnum {
                        declaration: choice.clone(),
                        arity: 1,
                    },
                    move |call| {
                        calls.fetch_add(1, Ordering::SeqCst);
                        let result = if wrong_child {
                            call.runtime.native_type_argument(call.owner(), &boolean)?
                        } else {
                            call.result_type_argument()?
                        };
                        let fields = if empty {
                            vec![]
                        } else {
                            let Ty::Enum(nominal) = result.ty() else {
                                unreachable!()
                            };
                            vec![if nominal.arguments[0] == Ty::Builtin(BuiltinType::Bool) {
                                Value::Bool(true)
                            } else {
                                Value::I32(7)
                            }]
                        };
                        call.allocate_enum(&result, &variant, fields)
                    },
                ),
            )
            .unwrap();
    }
    (builder.finish().unwrap(), calls)
}

fn result_program(native: &NativeModule) -> DefinitionMetadata<BytecodeProgram> {
    let metadata = native.to_declaration();
    let module = metadata.records();
    let declarations = module.native_declarations(metadata.definitions()).unwrap();
    let imports = declarations
        .iter()
        .map(|declaration| {
            let CallableImplementation::Native(binding) = declaration.function.implementation
            else {
                unreachable!()
            };
            NativeImport {
                receiver_access: None,
                result_adapter: None,
                generic: None,
                instance: ConcreteFunctionIdentity {
                    declaration: declaration.declaration,
                    arguments: vec![],
                },
                binding,
                signature: Signature {
                    params: vec![],
                    result: declaration.function.return_type.clone(),
                },
                requirements: vec![],
                callables: vec![],
            }
        })
        .collect();
    let choice = module
        .definition(DefinitionKind::Enum, "Choice", metadata.definitions())
        .unwrap();
    let enumerations = [BuiltinType::I32, BuiltinType::Bool]
        .into_iter()
        .map(|child| {
            let variant = |name: &str, payload| EnumVariantLayout {
                declaration: metadata
                    .definitions()
                    .lookup_child(choice, DefinitionKind::Variant, name, 0)
                    .unwrap(),
                payload,
                reports_failure: false,
            };
            EnumLayout {
                declaration: choice,
                arguments: vec![Ty::Builtin(child)],
                variants: vec![
                    variant("Some", vec![Ty::Builtin(child)]),
                    variant("None", vec![]),
                ],
            }
        })
        .collect();
    ProgramFixture {
        root: ModuleRef::new(0),
        exports: Default::default(),
        modules: vec![BytecodeModule {
            identity: module.identity.clone(),
            public_items: module
                .types
                .iter()
                .cloned()
                .map(PublicItem::Type)
                .chain(
                    declarations
                        .iter()
                        .map(|declaration| PublicItem::Function(declaration.function.clone())),
                )
                .collect(),
            native_declarations: declarations,
            native_imports: imports,
            enumerations,
            ..Default::default()
        }],
    }
    .metadata(metadata.definitions().clone(), &Default::default())
    .unwrap()
}

#[test]
fn nominal_enum_linking_requires_the_actual_provider_and_result_contract_before_callbacks() {
    let (native, calls) = result_module();
    let code = result_program(&native);
    for missing_provider in [true, false] {
        let mut runtime = Runtime::default();
        native.install(&mut runtime).unwrap();
        if missing_provider {
            Arc::make_mut(&mut runtime.native_entries.catalog.types)
                .retain_ids(|_, record| record.kind != TypeDefKind::Enum);
        } else {
            let (_, foreign) = enum_provider("codec::wrong_result");
            let id = runtime.native_entries.entries.ids().next().unwrap();
            let mut registration = runtime
                .native_entries
                .entries
                .get_id(id)
                .unwrap()
                .as_ref()
                .clone();
            registration.binding.result = Codec::NominalEnum {
                declaration: foreign,
                arity: 1,
            };
            runtime
                .native_entries
                .entries
                .insert_id(id, Arc::new(registration))
                .unwrap();
        }
        let error = runtime.load_program("wrong", code.clone()).unwrap_err();
        assert!(error.message().contains("codec"), "{error}");
        assert_eq!(calls.load(Ordering::SeqCst), 0);
    }
}

#[test]
fn nominal_enum_results_keep_children_return_checks_and_applied_type_arguments() {
    let (native, calls) = result_module();
    let mut runtime = Runtime::default();
    native.install(&mut runtime).unwrap();
    let program = runtime
        .load_program("results", result_program(&native))
        .unwrap();
    let owner = runtime.select_program(&program).unwrap();
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    for (slot, import) in owner.bytecode.native_imports.iter().enumerate() {
        let declaration = owner
            .bytecode
            .native_declarations
            .iter()
            .find(|declaration| declaration.declaration == import.instance.declaration)
            .unwrap();
        let linked = runtime
            .modules
            .native_binding(&owner, NativeImportId::new(slot))
            .unwrap();
        assert!(!linked.binding.converted_result);
        let result = invoke(&runtime, &owner, slot, &[]);
        if declaration.function.name.starts_with("wrong_") {
            let error = result.unwrap_err();
            assert_eq!(error.kind(), RuntimeErrorKind::ModuleValidation);
            assert!(error.message().contains("native result differs"));
            continue;
        }
        let value = result.unwrap();
        assert!(
            linked
                .type_signature()
                .unwrap()
                .result
                .matches(&runtime, &value, &owner)
        );
        let Value::Enum(id) = value else {
            unreachable!()
        };
        let snapshot = runtime.gc().enum_view(id).unwrap();
        match declaration.function.name.as_str() {
            "number" => assert_eq!(snapshot.fields, [Value::I32(7)]),
            "flag" => assert_eq!(snapshot.fields, [Value::Bool(true)]),
            "empty" => assert!(snapshot.fields.is_empty()),
            _ => unreachable!(),
        }
        drop(snapshot);
        let mut template = linked.as_ref().clone();
        let Ty::Enum(nominal) = &mut template.signature.result else {
            unreachable!()
        };
        let parameter = GenericParam {
            owner: nominal.declaration,
            position: 0,
        };
        nominal.arguments[0] = parameter.as_type();
        for child in [BuiltinType::I32, BuiltinType::Bool] {
            let arguments = runtime
                .type_arguments(&owner, None, &[Ty::Builtin(child)])
                .unwrap();
            let environment = runtime
                .alloc_environment(
                    EnvironmentRecord::new(
                        runtime.definition_context(),
                        vec![parameter.clone()],
                        arguments,
                    )
                    .unwrap(),
                )
                .unwrap();
            let applied = template
                .apply(&runtime, &owner, environment, &OperationBindings::default())
                .unwrap();
            let Ty::Enum(actual) = applied.type_signature().unwrap().result.ty() else {
                unreachable!()
            };
            assert_eq!(actual.arguments, [Ty::Builtin(child)]);
            assert_eq!(actual.declaration, parameter.owner);
            assert!(!applied.binding.converted_result);
        }
    }
    assert_eq!(calls.load(Ordering::SeqCst), 5);
}
