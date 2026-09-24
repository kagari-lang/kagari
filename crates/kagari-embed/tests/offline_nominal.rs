use kagari_common::{
    SourceFile,
    host_interface::{
        HostFieldDeclaration, HostFunctionDeclaration, HostInterface, HostParameter,
        HostPassingStyle, HostTypeDeclaration, HostTypeOwnership, HostValueType,
    },
    source_database::SourceLayer,
};
use kagari_embed::{
    ArtifactOptions, CompileOptions, ExecutionContext, HostExposurePolicy, KagariEngine,
};
use kagari_runtime::{
    CapabilitySet, LanguageProfile,
    host::{HostFunction, HostObjectId, HostSchemaEpoch, HostTypeRegistration},
    value::Value,
};
use std::{cell::RefCell, rc::Rc};

fn interface() -> HostInterface {
    let related = HostTypeDeclaration::new("right.Item");
    let mut item = HostTypeDeclaration::new("left.Item");
    item.ownership = HostTypeOwnership::HostRoot;
    item.path_access = kagari_common::host_interface::PathAccess::ReadOnly;
    item.fields.push(HostFieldDeclaration::new(
        &item.id,
        "related",
        HostValueType::Opaque(related.id.clone()),
    ));
    let make =
        HostFunctionDeclaration::new("left.make", vec![], HostValueType::Opaque(item.id.clone()));
    let take = HostFunctionDeclaration::new(
        "left.take",
        vec![HostParameter {
            name: "value".into(),
            ty: HostValueType::Opaque(item.id.clone()),
            passing: HostPassingStyle::SharedBorrow,
        }],
        HostValueType::I32,
    );
    HostInterface {
        paths: vec![],
        types: vec![item, related, HostTypeDeclaration::new("unused.Other")],
        functions: vec![make, take],
    }
}

#[test]
fn offline_host_type_navigation_is_available_from_signature_query() {
    let engine = KagariEngine::default();
    engine.set_host_interface(interface()).unwrap();
    let text = "// 中文 😀\r\nuse left::Item; fn accept(value: Item) -> Item { value }";
    let file = engine
        .set_source("mem://host-signature", text.into(), SourceLayer::Base)
        .unwrap();
    let signatures = engine
        .signatures(engine.source_snapshot(), &Default::default())
        .unwrap();
    let signature = signatures.file(file).unwrap();
    let annotation = text.find("value: Item").unwrap() + "value: ".len();
    assert_eq!(
        signature.host_type_at(annotation).unwrap().symbol,
        "left.Item"
    );
    assert!(signature.definition_at(annotation).is_none());
    assert!(signature.host_type_at(annotation - 1).is_none());
    assert!(signature.diagnostics().is_empty());

    let full = engine
        .analyze(
            engine.source_snapshot(),
            LanguageProfile::default(),
            &Default::default(),
        )
        .unwrap();
    assert_eq!(
        full.file(file).unwrap().host_type_at(annotation),
        signature.host_type_at(annotation)
    );
}

#[test]
fn declared_methods_link_by_identity_and_evaluate_receiver_then_arguments_once() {
    use kagari_common::host_interface::HostMethodDeclaration;
    let mut interface = interface();
    let mut method = HostMethodDeclaration::new(
        &interface.types[0].id,
        "add",
        vec![HostParameter {
            name: "amount".into(),
            ty: HostValueType::I32,
            passing: HostPassingStyle::Owned,
        }],
        HostValueType::I32,
    );
    method.receiver = HostPassingStyle::UniqueBorrow;
    method.effects.may_mutate_host_state = true;
    method.capability_requirements.fs_write = true;
    let method_id = method.id.clone();
    interface.types[0].methods.push(method);
    let rhs = HostFunctionDeclaration::new("left.rhs", vec![], HostValueType::I32);
    interface.functions.push(rhs.clone());
    let engine = KagariEngine::default();
    engine.set_host_interface(interface.clone()).unwrap();
    let profile = LanguageProfile {
        allow_host_calls: true,
        allow_jit: true,
        ..Default::default()
    };
    let artifact = engine
        .compile_to_artifact(
            SourceFile::new(
                "method.kgr",
                "use left as api; fn main() -> i32 { api::make().add(api::rhs()) }",
            ),
            CompileOptions {
                language_profile: profile,
            },
            Default::default(),
        )
        .unwrap();
    let required = &artifact.program.modules[artifact.program.root.index()].host_interface;
    let call = required
        .functions
        .iter()
        .find(|f| f.id == method_id)
        .unwrap();
    assert_eq!(
        call,
        &interface.types[0].method_contract(&method_id).unwrap()
    );
    for (encoded, jit) in [(false, false), (true, false), (true, true)] {
        let artifact = if encoded {
            kagari_ir::bytecode::KbcArtifact::from_bytes(&artifact.to_bytes().unwrap()).unwrap()
        } else {
            artifact.clone()
        };
        let context = ExecutionContext {
            language_profile: profile,
            capabilities: CapabilitySet {
                host_calls: true,
                jit: true,
                fs_write: true,
                ..Default::default()
            },
            host_policy: HostExposurePolicy {
                allowed_host_functions: vec![
                    "left.make".into(),
                    "left.rhs".into(),
                    "left.Item.add".into(),
                ],
                ..Default::default()
            },
            jit_policy: if jit {
                kagari_embed::JitPolicy::Enabled
            } else {
                kagari_embed::JitPolicy::Disabled
            },
            ..Default::default()
        };
        let mut runtime = engine.runtime(context.clone());
        let types = runtime
            .register_host_types(
                interface.types[..2]
                    .iter()
                    .cloned()
                    .map(|ty| HostTypeRegistration::new(ty, "Object"))
                    .collect(),
            )
            .unwrap();
        let root = runtime
            .runtime_mut()
            .register_host_root(HostObjectId(8), types[0], HostSchemaEpoch::new(0))
            .unwrap();
        let trace = Rc::new(RefCell::new(Vec::new()));
        let calls = trace.clone();
        runtime
            .register_host_function(HostFunction::new(
                interface.functions[0].clone(),
                move |_, _| {
                    calls.borrow_mut().push("receiver");
                    Ok(Value::HostRoot(root))
                },
            ))
            .unwrap();
        let calls = trace.clone();
        runtime
            .register_host_function(HostFunction::new(rhs.clone(), move |_, _| {
                calls.borrow_mut().push("argument");
                Ok(Value::I32(2))
            }))
            .unwrap();
        assert!(
            runtime
                .load_program(artifact.clone(), Default::default())
                .is_err()
        );
        assert!(trace.borrow().is_empty());
        let mut wrong = interface.types[0].method_contract(&method_id).unwrap();
        wrong.params[0].passing = HostPassingStyle::Owned;
        assert!(
            runtime
                .register_host_function(HostFunction::new(wrong, |_, _| panic!(
                    "invalid member binding"
                )))
                .is_err()
        );
        let total = Rc::new(std::cell::Cell::new(40));
        let state = total.clone();
        let calls = trace.clone();
        runtime
            .register_host_function(
                HostFunction::method(&interface.types[0], &method_id, move |context, args| {
                    calls.borrow_mut().push("method");
                    assert!(
                        context
                            .runtime()
                            .invoke_host("left.Item.add", args)
                            .is_err(),
                        "the receiver's exclusive lease prevents recursive access"
                    );
                    let Value::I32(amount) = args[1] else {
                        panic!("checked parameter")
                    };
                    state.set(state.get() + amount);
                    context.runtime().collect_garbage().unwrap();
                    Ok(Value::I32(state.get()))
                })
                .unwrap(),
            )
            .unwrap();
        let loaded = runtime.load_program(artifact, Default::default()).unwrap();
        let mut denied = context.clone();
        denied.jit_policy = kagari_embed::JitPolicy::Disabled;
        denied.capabilities.fs_write = false;
        assert!(runtime.execute(&loaded, "main", &[], &denied).is_err());
        assert_eq!(total.get(), 40);
        assert_eq!(*trace.borrow(), ["receiver", "argument"]);
        trace.borrow_mut().clear();
        let mut backend = jit.then(|| kagari_jit_cranelift::CraneliftBackend::for_host().unwrap());
        for expected in [42, 44] {
            let result = if let Some(backend) = &mut backend {
                runtime.execute_with_backend(&loaded, "main", &[], &context, backend)
            } else {
                runtime.execute(&loaded, "main", &[], &context)
            }
            .unwrap();
            assert_eq!(result.return_value, Value::I32(expected));
        }
        assert_eq!(
            *trace.borrow(),
            [
                "receiver", "argument", "method", "receiver", "argument", "method"
            ]
        );
        assert_eq!(runtime.runtime().gc().active_roots(), 0);
    }
}

#[test]
fn source_host_handles_link_offline_contracts_and_execute_across_backends() {
    let interface = interface();
    let engine = KagariEngine::default();
    engine
        .set_host_interface(HostInterface::from_bytes(&interface.to_bytes().unwrap()).unwrap())
        .unwrap();
    let profile = LanguageProfile {
        allow_host_calls: true,
        allow_jit: true,
        ..Default::default()
    };
    let artifact = engine.compile_to_artifact(
        SourceFile::new("nominal.kgr", "use left::Item; use left as api; pub fn pass(value: Item) -> api::Item { value } fn id<T>(value: T) -> T { value } fn main() -> i32 { val value: Item = api::make(); api::take(pass(id(value))) }"),
        CompileOptions { language_profile: profile }, ArtifactOptions::default(),
    ).unwrap();
    let module = &artifact.program.modules[artifact.program.root.index()];
    assert_eq!(module.host_interface.types, interface.types[..2]);
    let kagari_ir::module::PublicAbiItem::Function(pass) = &module.public_items[0] else {
        panic!("public pass")
    };
    assert_eq!(
        pass.return_type,
        kagari_ir::module::abi::AbiType::Host(interface.types[0].id.clone())
    );
    assert_eq!(
        pass.return_type.representation(),
        kagari_ir::module::ValueType::HostHandle
    );
    let mut invalid = artifact.program.clone();
    invalid.modules[artifact.program.root.index()]
        .host_interface
        .types
        .clear();
    assert!(kagari_ir::bytecode::KbcArtifact::from_program(invalid, Default::default()).is_err());
    for (encoded, jit) in [(false, false), (true, false), (true, true)] {
        let artifact = if encoded {
            kagari_ir::bytecode::KbcArtifact::from_bytes(&artifact.to_bytes().unwrap()).unwrap()
        } else {
            artifact.clone()
        };
        let context = ExecutionContext {
            language_profile: profile,
            capabilities: CapabilitySet {
                host_calls: true,
                jit: true,
                ..Default::default()
            },
            host_policy: HostExposurePolicy {
                allowed_host_functions: vec!["left.make".into(), "left.take".into()],
                ..Default::default()
            },
            jit_policy: if jit {
                kagari_embed::JitPolicy::Enabled
            } else {
                kagari_embed::JitPolicy::Disabled
            },
            ..Default::default()
        };
        let mut runtime = engine.runtime(context.clone());
        let ids = runtime
            .register_host_types(
                interface.types[..2]
                    .iter()
                    .cloned()
                    .map(|ty| HostTypeRegistration::new(ty, "Object"))
                    .collect(),
            )
            .unwrap();
        let root = runtime
            .runtime_mut()
            .register_host_root(HostObjectId(7), ids[0], HostSchemaEpoch::new(0))
            .unwrap();
        let trace = Rc::new(RefCell::new(Vec::new()));
        let calls = trace.clone();
        runtime
            .register_host_function(HostFunction::new(
                interface.functions[0].clone(),
                move |_, _| {
                    calls.borrow_mut().push("make");
                    Ok(Value::HostRoot(root))
                },
            ))
            .unwrap();
        let calls = trace.clone();
        runtime
            .register_host_function(HostFunction::new(
                interface.functions[1].clone(),
                move |context, args| {
                    calls.borrow_mut().push("take");
                    assert!(matches!(args[0], Value::HostRoot(_)));
                    context.runtime().collect_garbage().unwrap();
                    Ok(Value::I32(42))
                },
            ))
            .unwrap();
        let loaded = runtime.load_program(artifact, Default::default()).unwrap();
        assert!(trace.borrow().is_empty());
        let report = if jit {
            let mut backend = kagari_jit_cranelift::CraneliftBackend::for_host().unwrap();
            runtime.execute_with_backend(&loaded, "main", &[], &context, &mut backend)
        } else {
            runtime.execute(&loaded, "main", &[], &context)
        }
        .unwrap();
        assert_eq!(report.return_value, Value::I32(42));
        assert_eq!(*trace.borrow(), ["make", "take"]);
        assert_eq!(runtime.runtime().gc().active_roots(), 0);
    }
}

#[test]
fn annotation_only_host_dependencies_are_verified_and_linked() {
    let engine = KagariEngine::default();
    let interface = interface();
    engine.set_host_interface(interface.clone()).unwrap();
    let artifact = engine
        .compile_to_artifact(
            SourceFile::new(
                "annotation.kgr",
                "pub fn pass(value: left::Item) -> left::Item { value } fn main() -> i32 { 7 }",
            ),
            Default::default(),
            Default::default(),
        )
        .unwrap();
    let module = &artifact.program.modules[artifact.program.root.index()];
    assert!(module.host_interface.functions.is_empty());
    assert_eq!(module.host_interface.types.len(), 2);
    let mut runtime = engine.runtime(Default::default());
    assert!(
        runtime
            .load_program(artifact.clone(), Default::default())
            .is_err()
    );
    runtime
        .register_host_types(
            interface.types[..2]
                .iter()
                .cloned()
                .map(|ty| HostTypeRegistration::new(ty, "Object"))
                .collect(),
        )
        .unwrap();
    let loaded = runtime
        .load_program(artifact.clone(), Default::default())
        .unwrap();
    assert_eq!(
        runtime
            .execute(&loaded, "main", &[], &Default::default())
            .unwrap()
            .return_value,
        Value::I32(7)
    );
    let mut invalid = artifact.program;
    invalid.modules[invalid.root.index()]
        .host_interface
        .types
        .clear();
    assert!(kagari_ir::bytecode::verify_program(&invalid).is_err());
}

#[test]
fn source_field_chains_use_offline_contracts_and_evaluate_the_root_once() {
    use kagari_common::host_interface::{HostPathDeclaration, PathAccess};
    let mut declarations = interface();
    declarations.types[0].fields[0].path_access = PathAccess::ReadOnly;
    let mut count =
        HostFieldDeclaration::new(&declarations.types[1].id, "count", HostValueType::I32);
    count.path_access = PathAccess::ReadOnly;
    let path = HostPathDeclaration {
        root: declarations.types[0].id.clone(),
        segments: vec![
            kagari_common::host_interface::HostPathSegmentDeclaration::Field(
                declarations.types[0].fields[0].id.clone(),
            ),
            kagari_common::host_interface::HostPathSegmentDeclaration::Field(count.id.clone()),
        ],
        access: PathAccess::ReadOnly,
        schema_epoch: 7,
        capabilities: CapabilitySet {
            fs_read: true,
            ..Default::default()
        },
    };
    declarations.types[1].fields.push(count);
    declarations.paths.push(path.clone());
    let engine = KagariEngine::default();
    engine.set_host_interface(declarations.clone()).unwrap();
    let profile = LanguageProfile {
        allow_host_calls: true,
        allow_jit: true,
        ..Default::default()
    };
    let artifact = engine
        .compile_to_artifact(
            SourceFile::new(
                "field.kgr",
                "use left as api; fn main() -> i32 { api::make().related.count }",
            ),
            CompileOptions {
                language_profile: profile,
            },
            Default::default(),
        )
        .unwrap();
    let required = &artifact.program.modules[artifact.program.root.index()].host_interface;
    assert_eq!(required.paths, vec![path.clone()]);
    assert_eq!(required.types.len(), 2);
    for (encoded, jit) in [(false, false), (true, false), (true, true)] {
        let artifact = if encoded {
            kagari_ir::bytecode::KbcArtifact::from_bytes(&artifact.to_bytes().unwrap()).unwrap()
        } else {
            artifact.clone()
        };
        let context = ExecutionContext {
            language_profile: profile,
            capabilities: CapabilitySet {
                host_calls: true,
                fs_read: true,
                jit: true,
                ..Default::default()
            },
            host_policy: HostExposurePolicy {
                allowed_host_functions: vec!["left.make".into()],
                allowed_host_types: vec!["left.Item".into()],
                allow_host_path_reads: true,
                ..Default::default()
            },
            jit_policy: if jit {
                kagari_embed::JitPolicy::Enabled
            } else {
                kagari_embed::JitPolicy::Disabled
            },
            ..Default::default()
        };
        let mut runtime = engine.runtime(context.clone());
        let ids = runtime
            .register_host_types(
                declarations.types[..2]
                    .iter()
                    .cloned()
                    .map(|ty| HostTypeRegistration::new(ty, "Object"))
                    .collect(),
            )
            .unwrap();
        let root = runtime
            .runtime_mut()
            .register_host_root(HostObjectId(7), ids[0], HostSchemaEpoch::new(7))
            .unwrap();
        let trace = Rc::new(RefCell::new(Vec::new()));
        let calls = trace.clone();
        runtime
            .register_host_function(HostFunction::new(
                declarations.functions[0].clone(),
                move |_, _| {
                    calls.borrow_mut().push("root");
                    Ok(Value::HostRoot(root))
                },
            ))
            .unwrap();
        assert!(
            runtime
                .load_program(artifact.clone(), Default::default())
                .is_err()
        );
        assert!(trace.borrow().is_empty());
        let descriptor = runtime.runtime_mut().register_host_path(&path).unwrap();
        let calls = trace.clone();
        runtime
            .runtime_mut()
            .register_host_path_adapter(
                descriptor,
                kagari_runtime::HostPathAdapter::new().with_read(move |context, _| {
                    calls.borrow_mut().push("read");
                    context.runtime().collect_garbage().unwrap();
                    Ok(Value::I32(42))
                }),
            )
            .unwrap();
        let loaded = runtime.load_program(artifact, Default::default()).unwrap();
        let mut denied = context.clone();
        denied.capabilities.fs_read = false;
        denied.jit_policy = kagari_embed::JitPolicy::Disabled;
        let denied_error = runtime.execute(&loaded, "main", &[], &denied).unwrap_err();
        assert_eq!(*trace.borrow(), ["root"], "{denied_error:?}");
        trace.borrow_mut().clear();
        let report = if jit {
            let mut backend = kagari_jit_cranelift::CraneliftBackend::for_host().unwrap();
            runtime.execute_with_backend(&loaded, "main", &[], &context, &mut backend)
        } else {
            runtime.execute(&loaded, "main", &[], &context)
        }
        .unwrap();
        assert_eq!(report.return_value, Value::I32(42));
        assert_eq!(*trace.borrow(), ["root", "read"]);
        assert_eq!(runtime.runtime().gc().active_roots(), 0);
    }
}

#[test]
fn source_host_index_paths_capture_index_before_rhs_across_backends() {
    use kagari_common::host_interface::{
        HostIndexSegmentDeclaration, HostPathDeclaration, HostPathSegmentDeclaration, PathAccess,
    };
    use kagari_runtime::{
        AbiFingerprint, HostPathAdapter, TypeKind, TypeRegistration,
        host::{HostError, PreparedHostPathWrite},
    };
    use std::cell::Cell;

    let mut player = HostTypeDeclaration::new("game.Player");
    player.ownership = HostTypeOwnership::HostRoot;
    player.path_access = PathAccess::ReadWrite;
    let make = HostFunctionDeclaration::new(
        "game.make",
        vec![],
        HostValueType::Opaque(player.id.clone()),
    );
    let index = HostFunctionDeclaration::new("game.index", vec![], HostValueType::I32);
    let rhs = HostFunctionDeclaration::new("game.rhs", vec![], HostValueType::I32);
    let path = HostPathDeclaration {
        root: player.id.clone(),
        segments: vec![HostPathSegmentDeclaration::Index(
            HostIndexSegmentDeclaration {
                slot: 0,
                collection: HostValueType::Opaque(player.id.clone()),
                index: HostValueType::I32,
                result: HostValueType::I32,
                access: PathAccess::ReadWrite,
            },
        )],
        access: PathAccess::ReadWrite,
        schema_epoch: 0,
        capabilities: Default::default(),
    };
    let declarations = HostInterface {
        paths: vec![path.clone()],
        types: vec![player.clone()],
        functions: vec![make.clone(), index.clone(), rhs.clone()],
    };
    let engine = KagariEngine::default();
    engine.set_host_interface(declarations).unwrap();
    let profile = LanguageProfile {
        allow_host_calls: true,
        allow_path_mutation: true,
        allow_jit: true,
        ..Default::default()
    };
    let source = SourceFile::new(
        "host-index.kgr",
        "use game as api; fn main() -> i32 { var player = api::make(); player[api::index()] += api::rhs(); player[1] }",
    );
    let artifact = engine
        .compile_to_artifact(
            source,
            CompileOptions {
                language_profile: profile,
            },
            Default::default(),
        )
        .unwrap();
    assert!(
        engine
            .compile_to_artifact(
                SourceFile::new(
                    "invalid-host-index.kgr",
                    "use game as api; fn main() -> i32 { api::make()[true] }",
                ),
                CompileOptions {
                    language_profile: profile,
                },
                Default::default(),
            )
            .is_err()
    );
    for (encoded, jit) in [(false, false), (true, false), (true, true)] {
        let artifact = if encoded {
            kagari_ir::bytecode::KbcArtifact::from_bytes(&artifact.to_bytes().unwrap()).unwrap()
        } else {
            artifact.clone()
        };
        let context = ExecutionContext {
            language_profile: profile,
            capabilities: CapabilitySet {
                host_calls: true,
                path_mutation: true,
                jit: true,
                ..Default::default()
            },
            host_policy: HostExposurePolicy {
                allowed_host_functions: vec![
                    "game.make".into(),
                    "game.index".into(),
                    "game.rhs".into(),
                ],
                allowed_host_types: vec!["game.Player".into()],
                allow_host_path_reads: true,
                allow_host_path_mutation: true,
                ..Default::default()
            },
            jit_policy: if jit {
                kagari_embed::JitPolicy::Enabled
            } else {
                kagari_embed::JitPolicy::Disabled
            },
            ..Default::default()
        };
        let mut runtime = engine.runtime(context.clone());
        let player_id = runtime
            .register_host_type(HostTypeRegistration::new(player.clone(), "Player"))
            .unwrap();
        runtime
            .runtime()
            .types()
            .register(TypeRegistration {
                abi_fingerprint: AbiFingerprint(10),
                ..TypeRegistration::new("i32", TypeKind::Primitive)
            })
            .unwrap();
        let root = runtime
            .runtime_mut()
            .register_host_root(HostObjectId(1), player_id, HostSchemaEpoch::new(0))
            .unwrap();
        let trace = Rc::new(RefCell::new(Vec::new()));
        let state = Rc::new(Cell::new(10));
        let calls = trace.clone();
        runtime
            .register_host_function(HostFunction::new(make.clone(), move |_, _| {
                calls.borrow_mut().push("make");
                Ok(Value::HostRoot(root))
            }))
            .unwrap();
        let calls = trace.clone();
        runtime
            .register_host_function(HostFunction::new(index.clone(), move |_, _| {
                calls.borrow_mut().push("index");
                Ok(Value::I32(1))
            }))
            .unwrap();
        let calls = trace.clone();
        runtime
            .register_host_function(HostFunction::new(rhs.clone(), move |_, _| {
                calls.borrow_mut().push("rhs");
                Ok(Value::I32(2))
            }))
            .unwrap();
        let descriptor = runtime.runtime_mut().register_host_path(&path).unwrap();
        let (reads, value) = (trace.clone(), state.clone());
        let (writes, target) = (trace.clone(), state.clone());
        runtime
            .runtime_mut()
            .register_host_path_adapter(
                descriptor,
                HostPathAdapter::new()
                    .with_read(move |_, path| {
                        if path.dynamic_args.as_slice()[0].value != Value::I32(1) {
                            return Err(HostError::new("unexpected index"));
                        }
                        reads.borrow_mut().push("read");
                        Ok(Value::I32(value.get()))
                    })
                    .with_prepare_write(move |_, _, record| {
                        writes.borrow_mut().push("prepare");
                        let Value::I32(next) = record.new_value else {
                            return Err(HostError::new("expected i32"));
                        };
                        let (writes, target) = (writes.clone(), target.clone());
                        Ok(PreparedHostPathWrite::new(move || {
                            writes.borrow_mut().push("commit");
                            target.set(next);
                        }))
                    }),
            )
            .unwrap();
        let loaded = runtime.load_program(artifact, Default::default()).unwrap();
        let report = if jit {
            let mut backend = kagari_jit_cranelift::CraneliftBackend::for_host().unwrap();
            runtime.execute_with_backend(&loaded, "main", &[], &context, &mut backend)
        } else {
            runtime.execute(&loaded, "main", &[], &context)
        }
        .unwrap();
        assert_eq!(report.return_value, Value::I32(12));
        assert_eq!(state.get(), 12);
        assert_eq!(
            *trace.borrow(),
            ["make", "index", "rhs", "read", "prepare", "commit", "read"]
        );
    }
}

#[test]
fn source_host_writes_commit_after_rhs_and_preserve_completed_rhs_effects_on_failure() {
    use kagari_common::host_interface::{HostPathDeclaration, PathAccess};
    use kagari_runtime::host::{HostError, PreparedHostPathWrite};
    use std::cell::Cell;
    let mut declarations = interface();
    declarations.types[0].path_access = PathAccess::ReadWrite;
    declarations.types[0].fields[0].path_access = PathAccess::ReadWrite;
    declarations.types[0].fields[0].writable = true;
    let mut count =
        HostFieldDeclaration::new(&declarations.types[1].id, "count", HostValueType::I32);
    count.path_access = PathAccess::ReadWrite;
    count.writable = true;
    let path = HostPathDeclaration {
        root: declarations.types[0].id.clone(),
        segments: vec![
            kagari_common::host_interface::HostPathSegmentDeclaration::Field(
                declarations.types[0].fields[0].id.clone(),
            ),
            kagari_common::host_interface::HostPathSegmentDeclaration::Field(count.id.clone()),
        ],
        access: PathAccess::ReadWrite,
        schema_epoch: 0,
        capabilities: Default::default(),
    };
    declarations.types[1].fields.push(count);
    declarations.paths.push(path.clone());
    let rhs = HostFunctionDeclaration::new("left.rhs", vec![], HostValueType::I32);
    declarations.functions.push(rhs.clone());
    let engine = KagariEngine::default();
    engine.set_host_interface(declarations.clone()).unwrap();
    let profile = LanguageProfile {
        allow_host_calls: true,
        allow_path_mutation: true,
        allow_jit: true,
        ..Default::default()
    };
    for rebind in [false, true] {
        for (op, rhs_value, after_rhs, deleted, expected) in [
            ("=", 2, 100, false, Some(2)),
            ("+=", 2, 100, false, Some(102)),
            ("/=", 0, 100, false, None),
            ("+=", 1, i32::MAX, false, None),
            ("+=", 2, 100, true, None),
        ] {
            let text = if rebind {
                format!(
                    "use left as api; fn main() {{ var target = api::make(); target.related.count {op} if true {{ target = api::make(); api::rhs() }} else {{ 0 }}; }}"
                )
            } else {
                format!(
                    "use left as api; fn main() {{ api::make().related.count {op} api::rhs(); }}"
                )
            };
            let artifact = engine
                .compile_to_artifact(
                    SourceFile::new("write.kgr", text.clone()),
                    CompileOptions {
                        language_profile: profile,
                    },
                    Default::default(),
                )
                .unwrap();
            assert!(
                engine
                    .compile_to_artifact(
                        SourceFile::new("denied.kgr", text),
                        CompileOptions {
                            language_profile: LanguageProfile {
                                allow_path_mutation: false,
                                ..profile
                            }
                        },
                        Default::default()
                    )
                    .is_err()
            );
            for (encoded, jit) in [(false, false), (true, false), (true, true)] {
                let artifact = if encoded {
                    kagari_ir::bytecode::KbcArtifact::from_bytes(&artifact.to_bytes().unwrap())
                        .unwrap()
                } else {
                    artifact.clone()
                };
                let context = ExecutionContext {
                    language_profile: profile,
                    capabilities: CapabilitySet {
                        host_calls: true,
                        path_mutation: true,
                        jit: true,
                        ..Default::default()
                    },
                    host_policy: HostExposurePolicy {
                        allowed_host_functions: vec!["left.make".into(), "left.rhs".into()],
                        allowed_host_types: vec!["left.Item".into()],
                        allow_host_path_reads: true,
                        allow_host_path_mutation: true,
                        ..Default::default()
                    },
                    jit_policy: if jit {
                        kagari_embed::JitPolicy::Enabled
                    } else {
                        kagari_embed::JitPolicy::Disabled
                    },
                    ..Default::default()
                };
                let mut runtime = engine.runtime(context.clone());
                let ids = runtime
                    .register_host_types(
                        declarations.types[..2]
                            .iter()
                            .cloned()
                            .map(|ty| HostTypeRegistration::new(ty, "Object"))
                            .collect(),
                    )
                    .unwrap();
                let root = runtime
                    .runtime_mut()
                    .register_host_root(HostObjectId(7), ids[0], HostSchemaEpoch::new(0))
                    .unwrap();
                let replacement = runtime
                    .runtime_mut()
                    .register_host_root(HostObjectId(8), ids[0], HostSchemaEpoch::new(0))
                    .unwrap();
                let trace = Rc::new(RefCell::new(Vec::new()));
                let state = Rc::new(Cell::new(10));
                let removed = Rc::new(Cell::new(false));
                let calls = trace.clone();
                let root_calls = Cell::new(0);
                runtime
                    .register_host_function(HostFunction::new(
                        declarations.functions[0].clone(),
                        move |_, _| {
                            calls.borrow_mut().push("root");
                            let first = root_calls.replace(root_calls.get() + 1) == 0;
                            Ok(Value::HostRoot(if first { root } else { replacement }))
                        },
                    ))
                    .unwrap();
                let (calls, value, absent) = (trace.clone(), state.clone(), removed.clone());
                runtime
                    .register_host_function(HostFunction::new(rhs.clone(), move |_, _| {
                        calls.borrow_mut().push("rhs");
                        value.set(after_rhs);
                        absent.set(deleted);
                        Ok(Value::I32(rhs_value))
                    }))
                    .unwrap();
                let descriptor = runtime.runtime_mut().register_host_path(&path).unwrap();
                let (reads, value, absent) = (trace.clone(), state.clone(), removed.clone());
                let (writes, target) = (trace.clone(), state.clone());
                runtime
                    .runtime_mut()
                    .register_host_path_adapter(
                        descriptor,
                        kagari_runtime::HostPathAdapter::new()
                            .with_read(move |_, path| {
                                assert_eq!(path.root, root);
                                reads.borrow_mut().push("read");
                                if absent.get() {
                                    return Err(HostError::new("target removed by RHS"));
                                }
                                Ok(Value::I32(value.get()))
                            })
                            .with_prepare_write(move |context, _, record| {
                                writes.borrow_mut().push("prepare");
                                context.runtime().collect_garbage().unwrap();
                                let Value::I32(next) = record.new_value else {
                                    return Err(HostError::new("expected i32"));
                                };
                                let (writes, target) = (writes.clone(), target.clone());
                                Ok(PreparedHostPathWrite::new(move || {
                                    writes.borrow_mut().push("commit");
                                    target.set(next);
                                }))
                            }),
                    )
                    .unwrap();
                let loaded = runtime.load_program(artifact, Default::default()).unwrap();
                let result = if jit {
                    let mut backend = kagari_jit_cranelift::CraneliftBackend::for_host().unwrap();
                    runtime.execute_with_backend(&loaded, "main", &[], &context, &mut backend)
                } else {
                    runtime.execute(&loaded, "main", &[], &context)
                };
                if let Some(expected) = expected {
                    result.unwrap();
                    assert_eq!(state.get(), expected);
                    assert_eq!(
                        *trace.borrow(),
                        if rebind {
                            vec!["root", "root", "rhs", "read", "prepare", "commit"]
                        } else {
                            vec!["root", "rhs", "read", "prepare", "commit"]
                        }
                    );
                    let records = runtime.runtime().host_dirty_paths();
                    assert_eq!(records.len(), 1);
                    assert_eq!(records[0].old_value, Some(Value::I32(after_rhs)));
                    assert_eq!(records[0].new_value, Value::I32(expected));
                } else {
                    assert!(result.is_err());
                    assert_eq!(state.get(), after_rhs);
                    assert_eq!(
                        *trace.borrow(),
                        if rebind {
                            vec!["root", "root", "rhs", "read"]
                        } else {
                            vec!["root", "rhs", "read"]
                        }
                    );
                    assert!(runtime.runtime().host_dirty_paths().is_empty());
                }
                assert_eq!(runtime.runtime().gc().active_roots(), 0);
            }
        }
    }
}
