use super::*;

#[test]
fn artifact_host_trait_table_requires_callback_before_publication() {
    let mut counter = HostTypeDeclaration::new("demo.Counter");
    let method = HostMethodDeclaration::new(&counter.id, "read", vec![], HostValueType::I32);
    counter.methods.push(method.clone());
    let trait_id = DefinitionId {
        module: ModuleIdentity {
            package: PackageId("pkg".into()),
            path: vec!["api".into()],
        },
        path: vec![DefinitionPathSegment {
            kind: DefinitionKind::Trait,
            name: "Readable".into(),
            occurrence: 0,
        }],
    };
    let mut trait_method = trait_id.clone();
    trait_method.path.push(DefinitionPathSegment {
        kind: DefinitionKind::Method,
        name: "get".into(),
        occurrence: 0,
    });
    counter
        .trait_implementations
        .push(HostTraitImplementationDeclaration::new(
            trait_id,
            vec![],
            vec![HostTraitMethodBinding {
                trait_method,
                host_method: method.id.clone(),
            }],
        ));
    let engine = KagariEngine::default();
    engine
        .set_host_interface(HostInterface {
            paths: vec![],
            types: vec![counter.clone()],
            functions: vec![],
        })
        .unwrap();
    let artifact = engine
        .compile_to_artifact(
            SourceFile::new(
                "host-trait.kgr",
                "use demo::Counter; pub fn identity(value: Counter) -> Counter { value } fn main() {}",
            ),
            CompileOptions::default(),
            ArtifactOptions::default(),
        )
        .unwrap();
    let encoded = kagari_bytecode::KbcArtifact::from_bytes(&artifact.to_bytes().unwrap()).unwrap();
    let required = &encoded.program.modules[encoded.program.root.index()].host_interface;
    assert_eq!(
        required.types[0].trait_implementations,
        counter.trait_implementations
    );
    let context = ExecutionContext {
        host_policy: HostExposurePolicy {
            allowed_host_types: vec![counter.symbol.clone()],
            allowed_host_functions: vec!["demo.Counter.read".into()],
            ..Default::default()
        },
        ..Default::default()
    };
    let mut runtime = engine.runtime(context);
    runtime
        .register_host_type(HostTypeRegistration::new(counter.clone(), "Counter"))
        .unwrap();
    assert!(
        runtime
            .load_program(encoded.clone(), Default::default())
            .is_err()
    );
    runtime
        .register_host_function(
            HostFunction::method(&counter, &method.id, |_, _| Ok(Value::I32(42))).unwrap(),
        )
        .unwrap();
    runtime.load_program(encoded, Default::default()).unwrap();
}

#[test]
fn host_trait_table_is_checked_against_script_trait_signatures() {
    let engine = KagariEngine::default();
    let path = "mem://host-trait-contract";
    let source = "trait Readable<T: Eq + Hash> { fn get(self, amount: T) -> T; } use demo::Counter; fn accept(value: Counter) {}";
    let file = engine
        .set_source(path, source.into(), SourceLayer::Base)
        .unwrap();
    let module = engine
        .source_snapshot()
        .file(file)
        .unwrap()
        .module_identity()
        .clone();
    let trait_id = DefinitionId {
        module,
        path: vec![DefinitionPathSegment {
            kind: DefinitionKind::Trait,
            name: "Readable".into(),
            occurrence: 0,
        }],
    };
    let mut trait_method = trait_id.clone();
    trait_method.path.push(DefinitionPathSegment {
        kind: DefinitionKind::Method,
        name: "get".into(),
        occurrence: 0,
    });
    let mut host = HostTypeDeclaration::new("demo.Counter");
    let method = HostMethodDeclaration::new(
        &host.id,
        "read",
        vec![HostParameter {
            name: "amount".into(),
            ty: HostValueType::I32,
            passing: HostPassingStyle::Owned,
        }],
        HostValueType::I32,
    );
    host.methods.push(method.clone());
    host.trait_implementations
        .push(HostTraitImplementationDeclaration::new(
            trait_id,
            vec![HostValueType::I32],
            vec![HostTraitMethodBinding {
                trait_method,
                host_method: method.id,
            }],
        ));
    let install = |host: HostTypeDeclaration| {
        engine
            .set_host_interface(HostInterface {
                paths: vec![],
                types: vec![host],
                functions: vec![],
            })
            .unwrap();
        engine
            .signatures(engine.source_snapshot(), &Default::default())
            .unwrap()
            .file(file)
            .unwrap()
            .diagnostics()
            .to_vec()
    };
    assert!(install(host.clone()).is_empty());

    let mut wrong_result = host.clone();
    wrong_result.methods[0].return_type = HostValueType::Bool;
    assert!(install(wrong_result).iter().any(|diagnostic| matches!(
        &diagnostic.kind,
        kagari_common::DiagnosticKind::InvalidTraitImpl { reason, .. }
            if reason.contains("return type")
    )));

    let mut missing = host.clone();
    missing.trait_implementations[0].methods.clear();
    assert!(install(missing).iter().any(|diagnostic| matches!(
        &diagnostic.kind,
        kagari_common::DiagnosticKind::InvalidTraitImpl { reason, .. }
            if reason.contains("missing method")
    )));
    let mut wrong_argument = host.clone();
    wrong_argument.methods[0].params[0].ty = HostValueType::Bool;
    assert!(install(wrong_argument).iter().any(|diagnostic| matches!(
        &diagnostic.kind,
        kagari_common::DiagnosticKind::InvalidTraitImpl { reason, .. }
            if reason.contains("parameter 1")
    )));

    let mut wrong_arity = host.clone();
    wrong_arity.trait_implementations[0].trait_arguments.clear();
    assert!(install(wrong_arity).iter().any(|diagnostic| matches!(
        &diagnostic.kind,
        kagari_common::DiagnosticKind::InvalidTraitImpl { reason, .. }
            if reason.contains("type argument count")
    )));
    let mut unsatisfied_bound = host.clone();
    unsatisfied_bound.trait_implementations[0].trait_arguments = vec![HostValueType::F32];
    assert!(install(unsatisfied_bound).iter().any(|diagnostic| matches!(
        &diagnostic.kind,
        kagari_common::DiagnosticKind::InvalidTraitImpl { reason, .. }
            if reason.contains("does not satisfy its bound")
    )));

    let mut extra = host.clone();
    let mut extra_method = extra.trait_implementations[0].methods[0].clone();
    extra_method.trait_method.path.last_mut().unwrap().name = "extra".into();
    extra.trait_implementations[0].methods.push(extra_method);
    assert!(install(extra).iter().any(|diagnostic| matches!(
        &diagnostic.kind,
        kagari_common::DiagnosticKind::InvalidTraitImpl { reason, .. }
            if reason.contains("extra trait method")
    )));

    assert!(install(host).is_empty());
    engine
        .set_source(path, source.replace("-> T;", "-> bool;"), SourceLayer::Base)
        .unwrap();
    let changed = engine
        .signatures(engine.source_snapshot(), &Default::default())
        .unwrap();
    assert!(
        changed
            .file(file)
            .unwrap()
            .diagnostics()
            .iter()
            .any(|diagnostic| matches!(
                &diagnostic.kind,
                kagari_common::DiagnosticKind::InvalidTraitImpl { reason, .. }
                    if reason.contains("return type")
            ))
    );
}

#[test]
fn host_trait_bound_calls_use_bound_methods_across_execution_routes() {
    let engine = KagariEngine::default();
    let file = engine
        .set_source(
            "mem://host-trait-bound",
            include_str!("../../../../examples/host-trait-bound.kgr").into(),
            SourceLayer::Base,
        )
        .unwrap();
    let trait_id = DefinitionId {
        module: engine
            .source_snapshot()
            .file(file)
            .unwrap()
            .module_identity()
            .clone(),
        path: vec![DefinitionPathSegment {
            kind: DefinitionKind::Trait,
            name: "Readable".into(),
            occurrence: 0,
        }],
    };
    let mut trait_method = trait_id.clone();
    trait_method.path.push(DefinitionPathSegment {
        kind: DefinitionKind::Method,
        name: "get".into(),
        occurrence: 0,
    });
    let mut host = HostTypeDeclaration::new("demo.Counter");
    host.ownership = HostTypeOwnership::HostRoot;
    host.path_access = kagari_common::host_interface::PathAccess::ReadOnly;
    let number = HostMethodDeclaration::new(&host.id, "number", vec![], HostValueType::I32);
    let flag = HostMethodDeclaration::new(&host.id, "flag", vec![], HostValueType::Bool);
    host.methods.extend([number.clone(), flag.clone()]);
    host.trait_implementations
        .push(HostTraitImplementationDeclaration::new(
            trait_id.clone(),
            vec![HostValueType::I32],
            vec![HostTraitMethodBinding {
                trait_method: trait_method.clone(),
                host_method: number.id.clone(),
            }],
        ));
    host.trait_implementations
        .push(HostTraitImplementationDeclaration::new(
            trait_id,
            vec![HostValueType::Bool],
            vec![HostTraitMethodBinding {
                trait_method,
                host_method: flag.id.clone(),
            }],
        ));
    let make =
        HostFunctionDeclaration::new("demo.make", vec![], HostValueType::Opaque(host.id.clone()));
    let interface = HostInterface {
        paths: vec![],
        types: vec![host.clone()],
        functions: vec![make.clone()],
    };
    engine.set_host_interface(interface).unwrap();
    let profile = LanguageProfile {
        allow_host_calls: true,
        allow_jit: true,
        ..Default::default()
    };
    let checked = engine
        .compile_snapshot(
            engine.source_snapshot(),
            file,
            CompileOptions {
                language_profile: profile,
            },
            &Default::default(),
        )
        .unwrap();
    let artifact = engine.emit_bytecode(&checked, Default::default()).unwrap();
    let executable = &artifact.program.modules[artifact.program.root.index()];
    assert_eq!(executable.trait_contracts.len(), 1);
    assert_eq!(executable.trait_contracts[0].abi.name, "Readable");
    assert!(
        !executable
            .public_items
            .iter()
            .any(|item| matches!(item, kagari_abi::types::PublicAbiItem::Trait(_)))
    );
    for (encoded, jit) in [(false, false), (true, false), (true, true)] {
        let artifact = if encoded {
            kagari_bytecode::KbcArtifact::from_bytes(&artifact.to_bytes().unwrap()).unwrap()
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
                allowed_host_functions: vec![
                    "demo.make".into(),
                    "demo.Counter.number".into(),
                    "demo.Counter.flag".into(),
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
        let host_id = runtime
            .register_host_type(HostTypeRegistration::new(host.clone(), "Counter"))
            .unwrap();
        let root = runtime
            .runtime_mut()
            .register_host_root(HostObjectId(7), host_id, HostSchemaEpoch::new(0))
            .unwrap();
        let calls = Rc::new(RefCell::new(Vec::new()));
        let trace = calls.clone();
        runtime
            .register_host_function(HostFunction::new(make.clone(), move |_, _| {
                trace.borrow_mut().push("make");
                Ok(Value::HostRoot(root))
            }))
            .unwrap();
        let trace = calls.clone();
        runtime
            .register_host_function(
                HostFunction::method(&host, &number.id, move |_, args| {
                    trace.borrow_mut().push("number");
                    assert!(matches!(args, [Value::HostRoot(_)]));
                    Ok(Value::I32(42))
                })
                .unwrap(),
            )
            .unwrap();
        let trace = calls.clone();
        runtime
            .register_host_function(
                HostFunction::method(&host, &flag.id, move |_, args| {
                    trace.borrow_mut().push("flag");
                    assert!(matches!(args, [Value::HostRoot(_)]));
                    Ok(Value::Bool(true))
                })
                .unwrap(),
            )
            .unwrap();
        let loaded = runtime.load_program(artifact, Default::default()).unwrap();
        let report = if jit {
            let mut backend = kagari_codegen_cranelift::CraneliftBackend::for_host().unwrap();
            runtime.execute_with_backend(&loaded, "main", &[], &context, &mut backend)
        } else {
            runtime.execute(&loaded, "main", &[], &context)
        }
        .unwrap();
        assert_eq!(report.return_value, Value::I32(42));
        assert_eq!(*calls.borrow(), ["make", "flag", "make", "number"]);
    }
    let source = include_str!("../../../../examples/host-trait-bound.kgr");
    engine
        .set_source(
            "mem://host-trait-bound",
            format!(
                "{source}\nuse demo::Counter; impl Readable<i32> for Counter {{ fn get(self) -> i32 {{ 7 }} }}"
            ),
            SourceLayer::Base,
        )
        .unwrap();
    let signatures = engine
        .signatures(engine.source_snapshot(), &Default::default())
        .unwrap();
    assert!(
        signatures
            .file(file)
            .unwrap()
            .diagnostics()
            .iter()
            .any(|diagnostic| matches!(
                &diagnostic.kind,
                kagari_common::DiagnosticKind::InvalidTraitImpl { reason, .. }
                    if reason.contains("host and script implementations overlap")
            ))
    );
    engine
        .set_source("mem://host-trait-bound", source.into(), SourceLayer::Base)
        .unwrap();
    let mut unimplemented = host;
    unimplemented.trait_implementations.clear();
    engine
        .set_host_interface(HostInterface {
            paths: vec![],
            types: vec![unimplemented],
            functions: vec![make],
        })
        .unwrap();
    let error = engine
        .compile_snapshot(
            engine.source_snapshot(),
            file,
            CompileOptions {
                language_profile: profile,
            },
            &Default::default(),
        )
        .unwrap_err();
    assert!(format!("{error:?}").contains("KG_TYPE_GENERIC_BOUND_NOT_SATISFIED"));
}
