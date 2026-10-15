use super::*;
use kagari::{context::JitPolicy, program::PreparedProgram};
use kagari_bytecode::artifact::KbcArtifact;
use kagari_contract::types::PublicItem;
use kagari_runtime::module::entry::ProgramEntry;
use kagari_source::diagnostic::DiagnosticKind;
use kagari_types::declaration::{TraitDef, host::PathAccess};
use std::sync::{Arc, Mutex};

#[test]
fn source_trait_reference_uses_explicit_binding_and_portable_abi() {
    use kagari_runtime::{
        host::HostCallContext,
        native::{builder::ModuleBuilder, declarations::FunctionDecl, types::Type},
    };
    use kagari_types::declaration::host::{
        HostCallPolicy, HostFunctionEffects, HostReflectionPolicy, HostTypePolicy,
    };

    let trait_source = "pub trait Readable { type Output; fn get(self) -> Self::Output; fn again(self) -> Self::Output { self.get() } }";
    let module_identity = ModuleIdentity {
        package: PackageId("pkg".into()),
        path: vec!["api".into()],
    };
    let trait_path = DefinitionPath {
        module: module_identity.clone(),
        path: vec![DefinitionPathSegment {
            kind: DefinitionKind::Trait,
            name: "Readable".into(),
            occurrence: 0,
        }],
    };
    // Obtain the exact portable expectation from the trait's checked source ABI.
    let author = KagariEngine::default();
    let file = author
        .bind_module("mem://readable", module_identity.clone())
        .unwrap();
    author
        .set_source("mem://readable", trait_source.into(), SourceLayer::Base)
        .unwrap();
    let checked = author
        .compile_snapshot(author.source_snapshot(), file, &Default::default())
        .unwrap();
    let contract = author
        .emit_bytecode(&checked, ArtifactOptions::default())
        .unwrap();
    let definition = contract
        .records()
        .program
        .modules
        .iter()
        .find(|module| module.identity == module_identity)
        .unwrap()
        .public_items
        .iter()
        .find_map(|item| match item {
            PublicItem::Trait(trait_) => Some(trait_.clone()),
            _ => None,
        })
        .unwrap();
    let definition = DefinitionMetadata::checked(
        contract.definitions().clone(),
        definition,
        &Default::default(),
    )
    .unwrap();
    let catalog = kagari_stdlib::declarations::StandardDeclarations::default()
        .catalog()
        .unwrap();
    let build_module = |definition: DefinitionMetadata<TraitDef>, inspect_authoring| {
        let mut builder = ModuleBuilder::new("demo::counter", &catalog).unwrap();
        if inspect_authoring {
            let mut native_contract = definition.records().clone();
            let trait_id = definition.definitions().lookup(&trait_path).unwrap();
            let method_id = definition
                .definitions()
                .lookup_child(trait_id, DefinitionKind::Method, "get", 0)
                .unwrap();
            native_contract.methods[0].implementation =
                kagari_types::callable::CallableImplementation::Native(method_id);
            assert!(
                builder
                    .reference_trait(
                        &trait_path,
                        DefinitionMetadata::checked(
                            definition.definitions().clone(),
                            native_contract,
                            &Default::default()
                        )
                        .unwrap()
                    )
                    .is_err()
            );
        }
        let interface = builder
            .reference_trait(&trait_path, definition.clone())
            .unwrap();
        if inspect_authoring {
            assert!(builder.reference_trait(&trait_path, definition).is_err());
        }
        let mut counter = builder.define_type("Counter").unwrap();
        counter
            .external(HostTypePolicy {
                ownership: HostTypeOwnership::HostRoot,
                reflection: HostReflectionPolicy::Metadata,
                path_access: PathAccess::ReadOnly,
            })
            .unwrap();
        let counter = counter.finish().unwrap();
        if inspect_authoring {
            // Required members must bind; the source default remains owned by the script.
            assert!(
                builder
                    .implement(counter.clone(), |group| group
                        .trait_impl(interface.apply([])?, |methods| {
                            methods.associated_type("Output", Type::i32())
                        }))
                    .is_err()
            );
        }
        builder
            .implement(counter.clone(), |group| {
                group.trait_impl(interface.apply([])?, |methods| {
                    methods.associated_type("Output", Type::i32())?;
                    methods.host_policy(
                        "get",
                        HostCallPolicy {
                            passing: vec![HostPassingStyle::SharedBorrow],
                            effects: Default::default(),
                        },
                    )?;
                    methods.bind("get", |context: &HostCallContext<'_>, values: &[Value]| {
                        context.runtime().collect_garbage().unwrap();
                        let [Value::HostRoot(id)] = values else {
                            panic!("root")
                        };
                        assert_eq!(
                            context.runtime().gc().host_root(*id).unwrap().object_id(),
                            HostObjectId(7)
                        );
                        Ok(Value::I32(42))
                    })
                })
            })
            .unwrap();
        let make = builder
            .define_function(
                FunctionDecl::new("make")
                    .returns(counter.apply([]).unwrap())
                    .host_policy(HostCallPolicy {
                        passing: vec![],
                        effects: HostFunctionEffects {
                            may_allocate: true,
                            ..Default::default()
                        },
                    }),
            )
            .unwrap();
        builder
            .bind(make, |context: &HostCallContext<'_>, _: &[Value]| {
                let root = context.runtime().host().root(HostObjectId(7)).unwrap();
                context
                    .runtime()
                    .gc()
                    .alloc_host_root(root)
                    .map_err(|error| HostError::new(error.to_string()))
            })
            .unwrap();
        builder.finish().unwrap()
    };
    let module = build_module(definition, true);
    assert_eq!(module.declaration().trait_references.len(), 1);
    assert!(module.declaration().traits.is_empty());
    let engine = KagariEngine::with_native_modules(Default::default(), vec![module]).unwrap();
    engine
        .bind_module(
            "mem://counter-main",
            ModuleIdentity {
                package: PackageId("pkg".into()),
                path: vec!["main".into()],
            },
        )
        .unwrap();
    let main = engine.set_source("mem://counter-main", "use demo::counter::make; use pkg::api::Readable; pub fn main() -> i32 { make().again() }".into(), SourceLayer::Base).unwrap();
    assert!(
        engine
            .compile_snapshot(engine.source_snapshot(), main, &Default::default())
            .is_err()
    );
    engine
        .bind_module("mem://readable", module_identity.clone())
        .unwrap();
    engine
        .set_source("mem://readable", trait_source.into(), SourceLayer::Base)
        .unwrap();
    let foreign = engine
        .set_source(
            "mem://foreign-trait-user",
            "use pkg::api::Readable;".into(),
            SourceLayer::Overlay,
        )
        .unwrap();
    let signatures = engine
        .signatures(engine.source_snapshot(), &Default::default())
        .unwrap();
    assert!(
        signatures
            .file(foreign)
            .unwrap()
            .diagnostics()
            .iter()
            .any(|diagnostic| matches!(diagnostic.kind, DiagnosticKind::UnknownName { .. }))
    );
    engine.close_overlay("mem://foreign-trait-user").unwrap();
    let checked = engine
        .compile_snapshot(engine.source_snapshot(), main, &Default::default())
        .unwrap();
    let artifact = engine
        .emit_bytecode(&checked, ArtifactOptions::default())
        .unwrap();
    let artifact =
        KbcArtifact::from_bytes(&artifact.records().to_bytes(artifact.definitions()).unwrap())
            .unwrap();
    let prepared =
        PreparedProgram::from_artifact(artifact.clone(), &Default::default(), &Default::default())
            .unwrap();
    let context = ExecutionContext {
        jit_policy: JitPolicy::Disabled,
        ..Default::default()
    };
    let mut runtime = engine.runtime(context.clone());
    let ty = runtime
        .runtime()
        .types()
        .get_by_name("demo::counter::Counter")
        .unwrap()
        .id;
    runtime
        .runtime_mut()
        .register_host_root(HostObjectId(7), ty, HostSchemaEpoch::new(0))
        .unwrap();
    let loaded = runtime.load_program(&prepared, Default::default()).unwrap();
    let report = runtime
        .execute(&loaded, ProgramEntry::Main, &[], &context)
        .unwrap();
    assert_eq!(
        report.return_value.value(runtime.runtime().gc()),
        Some(Value::I32(42))
    );
    // Public lookup still works without a reference record, but the installed
    // module must not accept omission of its declared requirement.
    let definitions = DefinitionContext::new().unwrap();
    let imported = artifact
        .clone()
        .import_into(&definitions, &Default::default())
        .unwrap();
    let mut omitted = imported.records().program.clone();
    let provider = omitted
        .modules
        .iter_mut()
        .find(|module| !module.trait_references.is_empty())
        .unwrap();
    provider.trait_references.clear();
    let omitted = KbcArtifact::from_program(omitted, Default::default(), &definitions).unwrap();
    let omitted =
        DefinitionMetadata::checked(definitions.snapshot(), omitted, &Default::default()).unwrap();
    let omitted =
        PreparedProgram::from_artifact(omitted, &Default::default(), &Default::default()).unwrap();
    let error = runtime
        .load_program(&omitted, Default::default())
        .unwrap_err();
    assert!(
        format!("{error:?}").contains("source trait references differ"),
        "{error:?}"
    );
    // An old expectation cannot authorize a coherently changed source contract.
    let changed_source = "pub trait Readable { type Output; fn get(self) -> Self::Output; fn again(self) -> Self::Output { self.get() } fn extra(self) -> i32 { 0 } }";
    engine
        .set_source("mem://readable", changed_source.into(), SourceLayer::Base)
        .unwrap();
    let changed = engine
        .compile_snapshot(engine.source_snapshot(), main, &Default::default())
        .unwrap();
    assert!(
        engine
            .emit_bytecode(&changed, ArtifactOptions::default())
            .is_err()
    );

    // Compile a complete new provider configuration, then reject it against the
    // previous Runtime's installed native reference requirements.
    author
        .set_source("mem://readable", changed_source.into(), SourceLayer::Base)
        .unwrap();
    let source_contract = author
        .emit_bytecode(
            &author
                .compile_snapshot(author.source_snapshot(), file, &Default::default())
                .unwrap(),
            Default::default(),
        )
        .unwrap();
    let abi = source_contract
        .records()
        .program
        .modules
        .iter()
        .find(|module| module.identity == module_identity)
        .unwrap()
        .public_items
        .iter()
        .find_map(|item| match item {
            PublicItem::Trait(abi) => Some(abi.clone()),
            _ => None,
        })
        .unwrap();
    let definition = DefinitionMetadata::checked(
        source_contract.definitions().clone(),
        abi,
        &Default::default(),
    )
    .unwrap();
    let changed_engine = KagariEngine::with_native_modules(
        Default::default(),
        vec![build_module(definition, false)],
    )
    .unwrap();
    changed_engine
        .bind_module("mem://readable", module_identity)
        .unwrap();
    changed_engine
        .set_source("mem://readable", changed_source.into(), SourceLayer::Base)
        .unwrap();
    let main = changed_engine
        .bind_module(
            "mem://counter-main",
            ModuleIdentity {
                package: PackageId("pkg".into()),
                path: vec!["main".into()],
            },
        )
        .unwrap();
    changed_engine.set_source("mem://counter-main", "use demo::counter::make; use pkg::api::Readable; pub fn main() -> i32 { make().again() }".into(), SourceLayer::Base).unwrap();
    let changed = changed_engine
        .emit_bytecode(
            &changed_engine
                .compile_snapshot(changed_engine.source_snapshot(), main, &Default::default())
                .unwrap(),
            ArtifactOptions::default(),
        )
        .unwrap();
    let changed =
        KbcArtifact::from_bytes(&changed.records().to_bytes(changed.definitions()).unwrap())
            .unwrap();
    let changed =
        PreparedProgram::from_artifact(changed, &Default::default(), &Default::default()).unwrap();
    let error = runtime
        .load_program(&changed, Default::default())
        .unwrap_err();
    assert!(
        format!("{error:?}").contains("source trait references differ"),
        "{error:?}"
    );
}

#[test]
fn host_trait_table_is_checked_against_script_trait_signatures() {
    let definitions = DefinitionContext::new().unwrap();
    let engine = KagariEngine::default();
    let path = "mem://host-trait-contract";
    let source = "use std::hash::{Hash};\ntrait Readable<T: Eq + Hash> { fn get(self, amount: T) -> T; } use demo::Counter; fn accept(value: Counter) {}";
    let file = engine
        .set_source(path, source.into(), SourceLayer::Base)
        .unwrap();
    let module = engine
        .source_snapshot()
        .file(file)
        .unwrap()
        .module_identity()
        .clone();
    let trait_id = DefinitionPath {
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
    let trait_id = definitions.intern(&trait_id).unwrap();
    let trait_method = definitions.intern(&trait_method).unwrap();
    let mut host = HostTypeDeclaration::new(&definitions, "demo.Counter").unwrap();
    let method = HostMethodDeclaration::new(
        &definitions,
        host.id,
        "read",
        vec![HostParameter {
            name: "amount".into(),
            ty: HostValueType::I32,
            passing: HostPassingStyle::Owned,
        }],
        HostValueType::I32,
    )
    .unwrap();
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
            .set_host_interface(scoped(
                &definitions,
                HostInterface {
                    paths: vec![],
                    types: vec![host],
                    functions: vec![],
                },
            ))
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
        DiagnosticKind::InvalidTraitImpl { reason, .. }
            if reason.contains("return type")
    )));

    let mut missing = host.clone();
    missing.trait_implementations[0].methods.clear();
    assert!(install(missing).iter().any(|diagnostic| matches!(
        &diagnostic.kind,
        DiagnosticKind::InvalidTraitImpl { reason, .. }
            if reason.contains("missing method")
    )));
    let mut wrong_argument = host.clone();
    wrong_argument.methods[0].params[0].ty = HostValueType::Bool;
    assert!(install(wrong_argument).iter().any(|diagnostic| matches!(
        &diagnostic.kind,
        DiagnosticKind::InvalidTraitImpl { reason, .. }
            if reason.contains("parameter 1")
    )));

    let mut wrong_arity = host.clone();
    wrong_arity.trait_implementations[0].trait_arguments.clear();
    assert!(install(wrong_arity).iter().any(|diagnostic| matches!(
        &diagnostic.kind,
        DiagnosticKind::InvalidTraitImpl { reason, .. }
            if reason.contains("type argument count")
    )));
    let mut unsatisfied_bound = host.clone();
    unsatisfied_bound.trait_implementations[0].trait_arguments = vec![HostValueType::F32];
    assert!(install(unsatisfied_bound).iter().any(|diagnostic| matches!(
        &diagnostic.kind,
        DiagnosticKind::InvalidTraitImpl { reason, .. }
            if reason.contains("does not satisfy its bound")
    )));

    let mut extra = host.clone();
    let mut extra_method = extra.trait_implementations[0].methods[0].clone();
    let table = definitions.snapshot();
    let parent = table.parent(extra_method.trait_method).unwrap().unwrap();
    let segment = table
        .resolve(extra_method.trait_method)
        .unwrap()
        .last()
        .unwrap();
    extra_method.trait_method = definitions
        .intern_child(parent, segment.kind, "extra", segment.occurrence)
        .unwrap();
    extra.trait_implementations[0].methods.push(extra_method);
    assert!(install(extra).iter().any(|diagnostic| matches!(
        &diagnostic.kind,
        DiagnosticKind::InvalidTraitImpl { reason, .. }
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
                DiagnosticKind::InvalidTraitImpl { reason, .. }
                    if reason.contains("return type")
            ))
    );
}

#[test]
fn host_trait_bound_calls_use_bound_methods_across_execution_routes() {
    use kagari_runtime::{
        host::HostCallContext,
        native::{builder::ModuleBuilder, declarations::FunctionDecl, types::Type},
    };
    use kagari_types::declaration::host::{
        HostCallPolicy, HostFunctionEffects, HostReflectionPolicy, HostTypePolicy,
    };
    let source = include_str!("../../../../examples/host-trait-bound.kgr");
    let identity = ModuleIdentity {
        package: PackageId("pkg".into()),
        path: vec!["host_trait".into()],
    };
    let trait_path = DefinitionPath {
        module: identity.clone(),
        path: vec![DefinitionPathSegment {
            kind: DefinitionKind::Trait,
            name: "Readable".into(),
            occurrence: 0,
        }],
    };
    let author = KagariEngine::default();
    let file = author
        .bind_module("mem://host-trait-bound", identity.clone())
        .unwrap();
    let contract_source = source
        .split("fn main()")
        .next()
        .unwrap()
        .replace("use demo::make;", "");
    author
        .set_source("mem://host-trait-bound", contract_source, SourceLayer::Base)
        .unwrap();
    let contract = author
        .emit_bytecode(
            &author
                .compile_snapshot(author.source_snapshot(), file, &Default::default())
                .unwrap(),
            Default::default(),
        )
        .unwrap();
    let abi = contract.records().program.modules[contract.records().program.root.index()]
        .trait_contracts
        .iter()
        .find(|contract| contract.abi.name == "Readable")
        .unwrap()
        .abi
        .clone();
    let expected =
        DefinitionMetadata::checked(contract.definitions().clone(), abi, &Default::default())
            .unwrap();
    let calls = Arc::new(Mutex::new(Vec::new()));
    let build_module = |implemented| {
        let mut builder = ModuleBuilder::new(
            "demo",
            &kagari_stdlib::declarations::StandardDeclarations::default()
                .catalog()
                .unwrap(),
        )
        .unwrap();
        let readable = builder
            .reference_trait(&trait_path, expected.clone())
            .unwrap();
        let mut counter = builder.define_type("Counter").unwrap();
        counter
            .external(HostTypePolicy {
                ownership: HostTypeOwnership::HostRoot,
                reflection: HostReflectionPolicy::Metadata,
                path_access: PathAccess::ReadOnly,
            })
            .unwrap();
        let counter = counter.finish().unwrap();
        if implemented {
            builder
                .implement(counter.clone(), |group| {
                    for (ty, name, result) in [
                        (Type::i32(), "number", Value::I32(42)),
                        (Type::bool(), "flag", Value::Bool(true)),
                    ] {
                        let trace = calls.clone();
                        group.trait_impl(readable.apply([ty])?, |methods| {
                            methods.host_policy(
                                "get",
                                HostCallPolicy {
                                    passing: vec![HostPassingStyle::SharedBorrow],
                                    effects: Default::default(),
                                },
                            )?;
                            methods.bind("get", move |_: &HostCallContext<'_>, args: &[Value]| {
                                trace.lock().unwrap().push(name);
                                assert!(matches!(args, [Value::HostRoot(_)]));
                                Ok(result)
                            })
                        })?;
                    }
                    Ok(())
                })
                .unwrap();
        }
        let make = builder
            .define_function(
                FunctionDecl::new("make")
                    .returns(counter.apply([]).unwrap())
                    .host_policy(HostCallPolicy {
                        passing: vec![],
                        effects: HostFunctionEffects {
                            may_allocate: true,
                            ..Default::default()
                        },
                    }),
            )
            .unwrap();
        let trace = calls.clone();
        builder
            .bind(make, move |context: &HostCallContext<'_>, _: &[Value]| {
                trace.lock().unwrap().push("make");
                let root = context.runtime().host().root(HostObjectId(7)).unwrap();
                context
                    .runtime()
                    .gc()
                    .alloc_host_root(root)
                    .map_err(|error| HostError::new(error.to_string()))
            })
            .unwrap();
        builder.finish().unwrap()
    };
    let module = build_module(true);
    let engine = KagariEngine::with_native_modules(Default::default(), vec![module]).unwrap();
    let file = engine
        .bind_module("mem://host-trait-bound", identity.clone())
        .unwrap();
    engine
        .set_source("mem://host-trait-bound", source.into(), SourceLayer::Base)
        .unwrap();
    let foreign = engine
        .bind_module(
            "mem://private-trait-user",
            ModuleIdentity {
                package: identity.package.clone(),
                path: vec!["foreign".into()],
            },
        )
        .unwrap();
    engine
        .set_source(
            "mem://private-trait-user",
            "use pkg::host_trait::Readable;".into(),
            SourceLayer::Overlay,
        )
        .unwrap();
    let signatures = engine
        .signatures(engine.source_snapshot(), &Default::default())
        .unwrap();
    assert!(
        signatures
            .file(foreign)
            .unwrap()
            .diagnostics()
            .iter()
            .any(|diagnostic| matches!(diagnostic.kind, DiagnosticKind::ImportNotPublic { .. }))
    );
    engine.close_overlay("mem://private-trait-user").unwrap();
    let checked = engine
        .compile_snapshot(engine.source_snapshot(), file, &Default::default())
        .unwrap();
    let artifact = engine.emit_bytecode(&checked, Default::default()).unwrap();
    let executable = &artifact.records().program.modules[artifact.records().program.root.index()];
    assert_eq!(executable.trait_contracts.len(), 1);
    assert_eq!(executable.trait_contracts[0].abi.name, "Readable");
    assert!(
        !executable
            .public_items
            .iter()
            .any(|item| matches!(item, PublicItem::Trait(_)))
    );
    for (encoded, jit) in [(false, false), (true, false), (true, true)] {
        calls.lock().unwrap().clear();
        let artifact = if encoded {
            KbcArtifact::from_bytes(&artifact.records().to_bytes(artifact.definitions()).unwrap())
                .unwrap()
        } else {
            artifact.clone()
        };
        let context = ExecutionContext {
            jit_policy: if jit {
                JitPolicy::Enabled
            } else {
                JitPolicy::Disabled
            },
            ..Default::default()
        };
        let mut runtime = engine.runtime(context.clone());
        let counter_id = runtime
            .runtime()
            .types()
            .get_by_name("demo::Counter")
            .unwrap()
            .id;
        runtime
            .runtime_mut()
            .register_host_root(HostObjectId(7), counter_id, HostSchemaEpoch::new(0))
            .unwrap();
        let loaded_program =
            PreparedProgram::from_artifact(artifact, &Default::default(), &Default::default())
                .unwrap();
        let loaded = runtime
            .load_program(&loaded_program, Default::default())
            .unwrap();
        let report = if jit {
            let mut backend = CraneliftBackend::for_host().unwrap();
            let prepared = runtime
                .prepare_native(
                    &loaded_program,
                    &loaded,
                    ProgramEntry::Main,
                    &mut backend,
                    &context.cancellation,
                )
                .unwrap();
            runtime.execute_prepared(&loaded, ProgramEntry::Main, &[], &context, &prepared)
        } else {
            runtime.execute(&loaded, ProgramEntry::Main, &[], &context)
        }
        .unwrap();
        assert_eq!(
            report.return_value.value(runtime.runtime().gc()).unwrap(),
            Value::I32(42)
        );
        assert_eq!(*calls.lock().unwrap(), ["make", "flag", "make", "number"]);
    }
    engine.set_source("mem://host-trait-bound", format!(
        "{source}\nuse demo::Counter; impl Readable<i32> for Counter {{ fn get(self) -> i32 {{ 7 }} }}"
    ), SourceLayer::Base).unwrap();
    let error = engine
        .compile_snapshot(engine.source_snapshot(), file, &Default::default())
        .unwrap_err();
    assert!(format!("{error:?}").contains("overlap"), "{error:?}");
    let unimplemented =
        KagariEngine::with_native_modules(Default::default(), vec![build_module(false)]).unwrap();
    let file = unimplemented
        .bind_module("mem://host-trait-bound", identity)
        .unwrap();
    unimplemented
        .set_source("mem://host-trait-bound", source.into(), SourceLayer::Base)
        .unwrap();
    let error = unimplemented
        .compile_snapshot(unimplemented.source_snapshot(), file, &Default::default())
        .unwrap_err();
    assert!(
        format!("{error:?}").contains("KG_TYPE_GENERIC_BOUND_NOT_SATISFIED"),
        "{error:?}"
    );
}
