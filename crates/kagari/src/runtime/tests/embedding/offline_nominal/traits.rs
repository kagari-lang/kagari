use super::*;
#[cfg(feature = "native")]
use crate::program::native::NativeCompiler;
use crate::test_support::standard::standard_library;
use crate::{context::JitPolicy, program::Compilation};
use kagari_bytecode::{
    artifact::KbcArtifact, program::verified::VerifiedBytecodeProgram,
    verifier::BytecodeVerificationError,
};
use kagari_contract::types::PublicItem;
use kagari_runtime::{
    error::RuntimeErrorKind,
    module::entry::ProgramEntry,
    native::{catalog::DeclarationCatalog, host},
    value::{carriers::RootedValue, kind::ObjectKind},
};
use kagari_source::{diagnostic::DiagnosticKind, identity::FileId};
use kagari_types::callable::CallableImplementation;
use std::sync::{Arc, Mutex};

fn verified_owner(engine: &KagariEngine, file: FileId) -> VerifiedBytecodeProgram {
    let checked = engine
        .compile_snapshot(engine.source_snapshot(), file, &Default::default())
        .unwrap();
    let artifact = engine.emit_artifact(&checked, Default::default()).unwrap();
    let definitions = DefinitionContext::new().unwrap();
    artifact
        .import_into(&definitions, &Default::default())
        .unwrap()
        .into_records()
        .into_verified(&Default::default(), &definitions, &Default::default())
        .unwrap()
        .into_bytecode()
}

fn owner_catalog(baseline: &VerifiedBytecodeProgram) -> DeclarationCatalog {
    let owner = &baseline.program().modules[baseline.program().root.index()].identity;
    standard_library()
        .declarations()
        .catalog()
        .with_verified_modules(baseline, [owner])
        .unwrap()
}

fn trait_path(owner: &ModuleIdentity, name: &str) -> DefinitionPath {
    DefinitionPath {
        module: owner.clone(),
        path: vec![DefinitionPathSegment {
            kind: DefinitionKind::Trait,
            name: name.into(),
            occurrence: 0,
        }],
    }
}

#[test]
fn anonymous_source_trait_qualifiers_remain_scoped_to_native_importers() {
    let uri = "mem://anonymous-trait.kgr";
    let source = "trait Marker {} pub trait Visible {} fn main() {}";
    let author = KagariEngine::with_native_modules(
        Default::default(),
        standard_library().modules().unwrap(),
    )
    .unwrap();
    let file = author
        .set_source(uri, source.into(), SourceLayer::Base)
        .unwrap();
    let baseline = verified_owner(&author, file);
    let owner = &baseline.program().modules[baseline.program().root.index()].identity;
    let mut builder = ModuleBuilder::new("native_scope", &owner_catalog(&baseline)).unwrap();
    assert!(
        builder
            .reference_trait(&trait_path(owner, "Marker"))
            .is_err()
    );
    let visible = builder
        .reference_trait(&trait_path(owner, "Visible"))
        .unwrap();
    let mut counter = builder.define_type("Counter").unwrap();
    counter
        .external(HostTypePolicy {
            ownership: HostTypeOwnership::HostRoot,
            reflection: HostReflectionPolicy::Hidden,
            path_access: PathAccess::None,
        })
        .unwrap();
    let counter = counter.finish().unwrap();
    builder
        .implement(counter, |group| {
            group.trait_impl(visible.apply([])?, |_| Ok(()))
        })
        .unwrap();
    let engine = KagariEngine::with_native_modules(
        Default::default(),
        standard_library()
            .modules()
            .unwrap()
            .into_iter()
            .chain(vec![builder.finish().unwrap()])
            .collect(),
    )
    .unwrap();
    let file = engine
        .set_source(uri, source.into(), SourceLayer::Base)
        .unwrap();
    engine
        .compile_snapshot(engine.source_snapshot(), file, &Default::default())
        .unwrap();

    // Even the public sibling cannot be imported through the generated qualifier
    // by an ordinary source file. Only the explicit native dependency resolves it.
    let family = owner.package.0.strip_prefix("source-file:").unwrap();
    let ordinary = engine
        .set_source(
            "ordinary.kgr",
            format!("use __kagari_source_{family}::Visible; fn main() {{}}"),
            SourceLayer::Base,
        )
        .unwrap();
    let signatures = engine
        .signatures(engine.source_snapshot(), &Default::default())
        .unwrap();
    assert!(
        signatures
            .file(ordinary)
            .unwrap()
            .diagnostics()
            .iter()
            .any(|d| matches!(d.kind, DiagnosticKind::UnknownName { .. }))
    );
}

#[test]
fn source_trait_reference_uses_explicit_binding_and_portable_abi() {
    let trait_source = "pub trait Readable { type Output; fn get(self) -> Self::Output; fn again(self) -> Self::Output { self.get() } }";
    let module_identity = ModuleIdentity {
        package: PackageId("pkg".into()),
        path: vec!["api".into()],
    };
    let trait_path = trait_path(&module_identity, "Readable");
    // Obtain the exact portable expectation from the trait's checked source ABI.
    let author = KagariEngine::with_native_modules(
        Default::default(),
        standard_library().modules().unwrap(),
    )
    .unwrap();
    let file = author
        .bind_module("mem://readable", module_identity.clone())
        .unwrap();
    author
        .set_source("mem://readable", trait_source.into(), SourceLayer::Base)
        .unwrap();
    let baseline = verified_owner(&author, file);
    let build_module = |baseline: &VerifiedBytecodeProgram, inspect_authoring| {
        let mut builder = ModuleBuilder::new("demo::counter", &owner_catalog(baseline)).unwrap();
        let interface = builder.reference_trait(&trait_path).unwrap();
        if inspect_authoring {
            assert!(builder.reference_trait(&trait_path).is_err());
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
                    let method_policy = HostCallPolicy {
                        passing: vec![HostPassingStyle::SharedBorrow],
                        effects: Default::default(),
                    };
                    methods.define_method(
                        FunctionDecl::new("get")
                            .parameter("self", counter.apply([])?)
                            .returns(Type::i32())
                            .host_policy(method_policy.clone()),
                        host::callback(method_policy, |context, values| {
                            context.runtime_mut().collect_garbage().unwrap();
                            assert_eq!(values.len(), 1);
                            assert_eq!(values[0].kind(), Some(ObjectKind::HostRoot));
                            let root = context.runtime().host().root(HostObjectId(7)).unwrap();
                            assert_eq!(
                                context
                                    .borrows()
                                    .borrow_unique(root.object_id(), root.type_id())
                                    .unwrap_err()
                                    .kind(),
                                RuntimeErrorKind::HostBorrowConflict
                            );
                            Ok(ValueView::from(42))
                        }),
                    )
                })
            })
            .unwrap();
        let make_policy = HostCallPolicy {
            passing: vec![],
            effects: HostFunctionEffects {
                may_allocate: true,
                ..Default::default()
            },
        };
        let make = builder
            .define_function(
                FunctionDecl::new("make")
                    .returns(counter.apply([]).unwrap())
                    .host_policy(make_policy.clone()),
            )
            .unwrap();
        builder
            .bind(
                make,
                host::callback(make_policy, |context, _| {
                    let root = context.runtime().host().root(HostObjectId(7)).unwrap();
                    {
                        let root = context.runtime_mut().host_root_value(root).unwrap();
                        context.import(&root)
                    }
                    .map_err(|error| HostError::new(error.to_string()))
                }),
            )
            .unwrap();
        builder.finish().unwrap()
    };
    let module = build_module(&baseline, true);
    assert_eq!(
        module.required_source_traits().iter().collect::<Vec<_>>(),
        [&trait_path]
    );
    let importer = module.declaration().identity.clone();
    assert!(module.declaration().traits.is_empty());
    let engine = KagariEngine::with_native_modules(
        Default::default(),
        standard_library()
            .modules()
            .unwrap()
            .into_iter()
            .chain(vec![module])
            .collect(),
    )
    .unwrap();
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
        .emit_artifact(&checked, ArtifactOptions::default())
        .unwrap();
    let artifact =
        KbcArtifact::from_bytes(&artifact.records().to_bytes(artifact.definitions()).unwrap())
            .unwrap();
    let prepared =
        Compilation::from_artifact(artifact.clone(), &Default::default(), &Default::default())
            .unwrap();
    let context = ExecutionContext {
        jit_policy: JitPolicy::Disabled,
        ..Default::default()
    };
    let mut runtime = engine.runtime(context.clone());
    let ty = runtime
        .types()
        .get_by_name("demo::counter::Counter")
        .unwrap()
        .id;
    runtime
        .register_host_root(HostObjectId(7), ty, HostSchemaEpoch::new(0))
        .unwrap();
    let loaded = runtime
        .load_program(prepared.program(), Default::default())
        .unwrap();
    let report = runtime
        .execute(&loaded, ProgramEntry::Main, &[], &context)
        .unwrap();
    assert_eq!(report.return_value.as_i32(), Some(42));
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
        .find(|module| module.identity == importer)
        .unwrap();
    let required = definitions.snapshot().lookup(&trait_path).unwrap();
    assert!(
        provider
            .trait_references
            .iter()
            .any(|r| r.declaration == required)
    );
    provider
        .trait_references
        .retain(|r| r.declaration != required);
    let omitted = KbcArtifact::from_program(
        omitted,
        Default::default(),
        &definitions,
        &Default::default(),
    )
    .unwrap();
    let omitted =
        DefinitionMetadata::checked(definitions.snapshot(), omitted, &Default::default()).unwrap();
    let omitted =
        Compilation::from_artifact(omitted, &Default::default(), &Default::default()).unwrap();
    let error = runtime
        .load_program(omitted.program(), Default::default())
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
    // The compiler derives the requirement from the changed owner. SDK export
    // independently checks that product against its captured installation pin.
    let error = engine
        .emit_artifact(&changed, Default::default())
        .unwrap_err();
    assert!(
        format!("{error:?}").contains("source trait references differ"),
        "{error:?}"
    );

    // Compile a complete new provider configuration, then reject it against the
    // previous Runtime's installed native reference requirements.
    author
        .set_source("mem://readable", changed_source.into(), SourceLayer::Base)
        .unwrap();
    let changed_owner = verified_owner(&author, file);
    let changed_engine = KagariEngine::with_native_modules(
        Default::default(),
        standard_library()
            .modules()
            .unwrap()
            .into_iter()
            .chain(vec![build_module(&changed_owner, false)])
            .collect(),
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
        .emit_artifact(
            &changed_engine
                .compile_snapshot(changed_engine.source_snapshot(), main, &Default::default())
                .unwrap(),
            ArtifactOptions::default(),
        )
        .unwrap();
    // Even before checking installation pins, verification rejects a carried
    // requirement that omits the new default from its own reachable owner's ABI.
    let mut inconsistent = changed.records().program.clone();
    let required = changed.definitions().lookup(&trait_path).unwrap();
    let requirement = inconsistent
        .modules
        .iter_mut()
        .find(|m| m.identity == importer)
        .unwrap()
        .trait_references
        .iter_mut()
        .find(|r| r.declaration == required)
        .unwrap();
    let extra = requirement
        .abi
        .methods
        .iter()
        .position(|m| m.name == "extra")
        .unwrap();
    requirement.abi.methods.remove(extra);
    assert!(matches!(
        verify_program(&inconsistent, changed.definitions(), &Default::default()),
        Err(BytecodeVerificationError::InvalidInterfaceTable)
    ));
    let changed =
        KbcArtifact::from_bytes(&changed.records().to_bytes(changed.definitions()).unwrap())
            .unwrap();
    let changed =
        Compilation::from_artifact(changed, &Default::default(), &Default::default()).unwrap();
    let error = runtime
        .load_program(changed.program(), Default::default())
        .unwrap_err();
    assert!(
        format!("{error:?}").contains("source trait references differ"),
        "{error:?}"
    );
}

#[test]
fn host_trait_table_is_checked_against_script_trait_signatures() {
    let source =
        "use std::hash::Hash; pub trait Readable<T: Eq + Hash> { fn get(self, amount: T) -> T; }";
    let identity = ModuleIdentity {
        package: PackageId("pkg".into()),
        path: vec!["contract".into()],
    };
    let trait_path = trait_path(&identity, "Readable");
    let author = KagariEngine::with_native_modules(
        Default::default(),
        standard_library().modules().unwrap(),
    )
    .unwrap();
    let file = author
        .bind_module("mem://host-trait-contract", identity.clone())
        .unwrap();
    author
        .set_source(
            "mem://host-trait-contract",
            source.into(),
            SourceLayer::Base,
        )
        .unwrap();
    let providers = owner_catalog(&verified_owner(&author, file));
    let build_module = |ty: Type| {
        let mut builder = ModuleBuilder::new("demo", &providers).unwrap();
        let readable = builder.reference_trait(&trait_path).unwrap();
        let mut counter = builder.define_type("Counter").unwrap();
        counter
            .external(HostTypePolicy {
                ownership: HostTypeOwnership::HostRoot,
                reflection: HostReflectionPolicy::Metadata,
                path_access: PathAccess::ReadOnly,
            })
            .unwrap();
        let counter = counter.finish().unwrap();
        assert!(
            builder
                .implement(counter.clone(), |group| {
                    group.trait_impl(readable.apply([ty.clone()])?, |_| Ok(()))
                })
                .unwrap_err()
                .to_string()
                .contains("missing method")
        );
        assert!(
            builder
                .implement(counter.clone(), |group| {
                    group.trait_impl(readable.apply([ty.clone()])?, |methods| {
                        methods.define_method(
                            FunctionDecl::new("extra").parameter("self", counter.apply([])?),
                            host::callback(
                                HostCallPolicy {
                                    passing: vec![HostPassingStyle::SharedBorrow],
                                    effects: Default::default(),
                                },
                                |_, _| Ok(ValueView::unit()),
                            ),
                        )
                    })
                })
                .unwrap_err()
                .to_string()
                .contains("unknown implementation member")
        );
        builder
            .implement(counter, |group| {
                let receiver = group.receiver();
                group.trait_impl(readable.apply([ty.clone()])?, |methods| {
                    let method_policy = HostCallPolicy {
                        passing: vec![HostPassingStyle::SharedBorrow, HostPassingStyle::Owned],
                        effects: Default::default(),
                    };
                    methods.define_method(
                        FunctionDecl::new("get")
                            .parameter("self", receiver)
                            .parameter("amount", ty.clone())
                            .returns(ty)
                            .host_policy(method_policy.clone()),
                        host::callback(method_policy, |_, args| Ok(args[1])),
                    )
                })
            })
            .unwrap();
        builder.finish().unwrap()
    };
    let module = build_module(Type::i32());
    let declaration = module.to_declaration();
    // Keep the real binding and verified core evidence fixed; vary only the candidate ABI.
    let [entry] = declaration
        .records()
        .native_declarations(declaration.definitions())
        .unwrap()
        .try_into()
        .unwrap();
    let CallableImplementation::Native(id) = entry.function.implementation else {
        unreachable!()
    };
    let binding = host::callback(
        HostCallPolicy {
            passing: vec![HostPassingStyle::SharedBorrow, HostPassingStyle::Owned],
            effects: Default::default(),
        },
        |_, args| Ok(args[1]),
    );
    let validate = |candidate| {
        let candidate = DefinitionMetadata::checked(
            declaration.definitions().clone(),
            candidate,
            &Default::default(),
        )
        .unwrap();
        ModuleBuilder::from_declaration(candidate, &providers, [(id, binding.clone())].into())?
            .finish()
    };
    validate(declaration.records().clone()).unwrap();
    let mut wrong_result = declaration.records().clone();
    wrong_result.implementations[0].methods[0].return_type = Ty::Builtin(BuiltinType::Bool);
    assert!(
        validate(wrong_result)
            .unwrap_err()
            .to_string()
            .contains("trait implementation differs")
    );
    let mut missing = declaration.records().clone();
    missing.implementations[0].methods.clear();
    assert!(
        validate(missing)
            .unwrap_err()
            .to_string()
            .contains("missing method")
    );
    let mut wrong_argument = declaration.records().clone();
    wrong_argument.implementations[0].methods[0].params[1].ty = Ty::Builtin(BuiltinType::Bool);
    assert!(
        validate(wrong_argument)
            .unwrap_err()
            .to_string()
            .contains("trait implementation differs")
    );
    let mut wrong_arity = declaration.records().clone();
    wrong_arity.implementations[0]
        .trait_type
        .as_mut()
        .unwrap()
        .arguments
        .clear();
    assert!(validate(wrong_arity).is_err());
    let mut extra = declaration.records().clone();
    let mut method = extra.implementations[0].methods[0].clone();
    method.name = "extra".into();
    extra.implementations[0].methods.push(method);
    assert!(validate(extra).is_err());

    let install = |module| {
        let engine = KagariEngine::with_native_modules(
            Default::default(),
            standard_library()
                .modules()
                .unwrap()
                .into_iter()
                .chain(vec![module])
                .collect(),
        )
        .unwrap();
        let file = engine
            .bind_module("mem://host-trait-contract", identity.clone())
            .unwrap();
        engine
            .set_source(
                "mem://host-trait-contract",
                format!("{source} use demo::Counter; fn accept(value: Counter) {{}}"),
                SourceLayer::Base,
            )
            .unwrap();
        (engine, file)
    };
    let (engine, file) = install(module);
    assert!(
        engine
            .compile_snapshot(engine.source_snapshot(), file, &Default::default())
            .is_ok()
    );
    let (unbounded, invalid_file) = install(build_module(Type::scalar(BuiltinType::F32)));
    let error = unbounded
        .compile_snapshot(
            unbounded.source_snapshot(),
            invalid_file,
            &Default::default(),
        )
        .unwrap_err();
    let diagnostic = format!("{error:?}");
    assert!(
        diagnostic.contains("KG_TYPE_GENERIC_BOUND_NOT_SATISFIED")
            && diagnostic.contains("does not satisfy `Eq`")
            && diagnostic.contains("does not satisfy `Hash`"),
        "{diagnostic}"
    );
    engine
        .set_source(
            "mem://host-trait-contract",
            format!(
                "{} use demo::Counter; fn accept(value: Counter) {{}}",
                source.replace("-> T;", "-> bool;")
            ),
            SourceLayer::Base,
        )
        .unwrap();
    let changed = engine
        .compile_snapshot(engine.source_snapshot(), file, &Default::default())
        .unwrap_err();
    let diagnostic = format!("{changed:?}");
    assert!(
        diagnostic.contains("KG_TYPE_TRAIT_METHOD_MISMATCH")
            && diagnostic.contains("return type expected `bool`, found `i32`"),
        "{diagnostic}"
    );
}

#[test]
fn host_trait_bound_calls_use_bound_methods_across_execution_routes() {
    // The example's local trait must be public for a foreign native implementation.
    let source = include_str!("../../../../../../../examples/host-trait-bound.kgr").to_owned();
    let private_source = source.replace("pub trait Readable", "trait Readable");
    let identity = ModuleIdentity {
        package: PackageId("pkg".into()),
        path: vec!["host_trait".into()],
    };
    let trait_path = trait_path(&identity, "Readable");
    let author = KagariEngine::with_native_modules(
        Default::default(),
        standard_library().modules().unwrap(),
    )
    .unwrap();
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
    let providers = owner_catalog(&verified_owner(&author, file));
    let calls = Arc::new(Mutex::new(Vec::new()));
    let build_module = |implemented| {
        let mut builder = ModuleBuilder::new("demo", &providers).unwrap();
        let readable = builder.reference_trait(&trait_path).unwrap();
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
                        (Type::i32(), "number", RootedValue::from(42_i32)),
                        (Type::bool(), "flag", RootedValue::from(true)),
                    ] {
                        let trace = calls.clone();
                        let receiver = group.receiver();
                        group.trait_impl(readable.apply([ty.clone()])?, |methods| {
                            let method_policy = HostCallPolicy {
                                passing: vec![HostPassingStyle::SharedBorrow],
                                effects: Default::default(),
                            };
                            methods.define_method(
                                FunctionDecl::new("get")
                                    .parameter("self", receiver)
                                    .returns(ty)
                                    .host_policy(method_policy.clone()),
                                host::callback(method_policy, move |cx, args| {
                                    trace.lock().unwrap().push(name);
                                    assert_eq!(args.len(), 1);
                                    assert_eq!(args[0].kind(), Some(ObjectKind::HostRoot));
                                    cx.import(&result)
                                        .map_err(|error| HostError::new(error.to_string()))
                                }),
                            )
                        })?;
                    }
                    Ok(())
                })
                .unwrap();
        }
        let make_policy = HostCallPolicy {
            passing: vec![],
            effects: HostFunctionEffects {
                may_allocate: true,
                ..Default::default()
            },
        };
        let make = builder
            .define_function(
                FunctionDecl::new("make")
                    .returns(counter.apply([]).unwrap())
                    .host_policy(make_policy.clone()),
            )
            .unwrap();
        let trace = calls.clone();
        builder
            .bind(
                make,
                host::callback(make_policy, move |context, _| {
                    trace.lock().unwrap().push("make");
                    let root = context.runtime().host().root(HostObjectId(7)).unwrap();
                    {
                        let root = context.runtime_mut().host_root_value(root).unwrap();
                        context.import(&root)
                    }
                    .map_err(|error| HostError::new(error.to_string()))
                }),
            )
            .unwrap();
        builder.finish().unwrap()
    };
    let module = build_module(true);
    let engine = KagariEngine::with_native_modules(
        Default::default(),
        standard_library()
            .modules()
            .unwrap()
            .into_iter()
            .chain(vec![module])
            .collect(),
    )
    .unwrap();
    let file = engine
        .bind_module("mem://host-trait-bound", identity.clone())
        .unwrap();
    engine
        .set_source("mem://host-trait-bound", source.clone(), SourceLayer::Base)
        .unwrap();
    // A pin from the public baseline cannot grant access after the target turns private.
    engine
        .set_source(
            "mem://host-trait-bound",
            private_source,
            SourceLayer::Overlay,
        )
        .unwrap();
    let error = engine
        .compile_snapshot(engine.source_snapshot(), file, &Default::default())
        .unwrap_err();
    assert!(
        format!("{error:?}").contains("KG_RESOLVE_IMPORT_NOT_PUBLIC"),
        "{error:?}"
    );
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
    let diagnostics = signatures.file(foreign).unwrap().diagnostics();
    assert!(
        matches!(diagnostics, [d] if matches!(&d.kind, DiagnosticKind::ImportNotPublic { path } if path == "pkg::host_trait::Readable")),
        "{diagnostics:?}"
    );
    engine.close_overlay("mem://private-trait-user").unwrap();
    engine.close_overlay("mem://host-trait-bound").unwrap();
    let checked = engine
        .compile_snapshot(engine.source_snapshot(), file, &Default::default())
        .unwrap();
    let artifact = engine.emit_artifact(&checked, Default::default()).unwrap();
    let executable = &artifact.records().program.modules[artifact.records().program.root.index()];
    assert!(executable.trait_contracts.is_empty());
    assert!(
        executable
            .public_items
            .iter()
            .any(|item| matches!(item, PublicItem::Trait(abi) if abi.name == "Readable"))
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
        let counter_id = runtime.types().get_by_name("demo::Counter").unwrap().id;
        runtime
            .register_host_root(HostObjectId(7), counter_id, HostSchemaEpoch::new(0))
            .unwrap();
        let loaded_program =
            Compilation::from_artifact(artifact, &Default::default(), &Default::default()).unwrap();
        let loaded = runtime
            .load_program(loaded_program.program(), Default::default())
            .unwrap();
        let report = if jit {
            let mut backend = CraneliftBackend::for_host().unwrap();
            let prepared = NativeCompiler::default()
                .prepare(
                    &loaded_program,
                    ProgramEntry::Main,
                    &mut backend,
                    &context.cancellation,
                )
                .and_then(|native_code| (native_code).install(&mut runtime, &loaded))
                .unwrap();
            runtime.execute_prepared(&loaded, ProgramEntry::Main, &[], &context, &prepared)
        } else {
            runtime.execute(&loaded, ProgramEntry::Main, &[], &context)
        }
        .unwrap();
        assert_eq!(report.return_value.as_i32().unwrap(), 42);
        assert_eq!(*calls.lock().unwrap(), ["make", "flag", "make", "number"]);
    }
    engine.set_source("mem://host-trait-bound", format!(
        "{source}\nuse demo::Counter; impl Readable<i32> for Counter {{ fn get(self) -> i32 {{ 7 }} }}"
    ), SourceLayer::Base).unwrap();
    let error = engine
        .compile_snapshot(engine.source_snapshot(), file, &Default::default())
        .unwrap_err();
    assert!(format!("{error:?}").contains("overlap"), "{error:?}");
    let unimplemented = KagariEngine::with_native_modules(
        Default::default(),
        standard_library()
            .modules()
            .unwrap()
            .into_iter()
            .chain(vec![build_module(false)])
            .collect(),
    )
    .unwrap();
    let file = unimplemented
        .bind_module("mem://host-trait-bound", identity)
        .unwrap();
    unimplemented
        .set_source("mem://host-trait-bound", source, SourceLayer::Base)
        .unwrap();
    let error = unimplemented
        .compile_snapshot(unimplemented.source_snapshot(), file, &Default::default())
        .unwrap_err();
    assert!(
        format!("{error:?}").contains("KG_TYPE_GENERIC_BOUND_NOT_SATISFIED"),
        "{error:?}"
    );
}
