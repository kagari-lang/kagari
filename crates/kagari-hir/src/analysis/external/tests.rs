use super::*;
use crate::tests::standard::standard_modules;
use crate::{
    analysis::{AnalysisSnapshot, FileAnalysis, core_source::packaged_modules},
    compile_input::program::{AdmissionError, CheckedCompilation},
    hir::{item::function::FunctionImplementation, writeability::Writeability},
    semantic::signatures::FunctionImplementation as CheckedImplementation,
    types::TypeId,
};
use kagari_common::identity::{
    ModuleIdentity, PackageId, map::DefinitionContext, metadata::DefinitionMetadata,
};
use kagari_source::{
    diagnostic::{DiagnosticKind, Severity},
    source_database::{SourceDatabase, SourceLayer},
};
use kagari_types::{
    declaration::{
        FieldDef, TypeDef, TypeDefKind, module::ModuleDecl, native::NativeStorageLayout,
    },
    range::RangeKind,
    reference::ReferenceAccess,
    scalar::BuiltinType,
    ty::GenericParam,
    visibility::Visibility,
};

const LIBRARY: &str = include_str!("../../../tests/fixtures/analysis/external/library.kgr");
const CLIENT: &str = include_str!("../../../tests/fixtures/analysis/external/client.kgr");
const INVALID_CLIENT: &str =
    include_str!("../../../tests/fixtures/analysis/external/invalid_client.kgr");
const CONSTRUCTORS: &str =
    include_str!("../../../tests/fixtures/analysis/external/constructors.kgr");

fn input(text: &str) -> ExternalModule {
    let mut sources = SourceDatabase::new("/exports").unwrap();
    sources
        .bind_module(
            "api.kgr",
            ModuleIdentity {
                package: PackageId("api".into()),
                path: vec![],
            },
        )
        .unwrap();
    let id = sources
        .set("api.kgr", text.into(), SourceLayer::Base)
        .unwrap();
    ExternalModule::new(sources.snapshot().file(id).unwrap().clone())
}

fn analyze(input: ExternalModule, text: &str) -> (AnalysisDatabase, AnalysisSnapshot, FileId) {
    let mut db = AnalysisDatabase::new("/project").unwrap();
    let mut inputs = if input.dependencies.is_empty() {
        Vec::new()
    } else {
        super::foundation::exported(&standard_modules())
    };
    inputs.push(input);
    db.set_external_modules(inputs);
    let source = Arc::new(SourceFile::new("/project/main.kgr", text));
    let id = source.id();
    let snapshot = db
        .snapshot(SourceSnapshot::single_file(source), &Default::default())
        .unwrap();
    (db, snapshot, id)
}

pub(super) fn assert_clean(snapshot: &AnalysisSnapshot) {
    for file in snapshot.files() {
        assert!(
            file.result()
                .diagnostics()
                .iter()
                .all(|diagnostic| diagnostic.severity != Severity::Error),
            "{}: {:?}",
            file.source().name(),
            file.result().diagnostics()
        );
    }
}

#[test]
fn external_source_preserves_fields_members_and_complete_script_definitions_without_registration() {
    let library = input(LIBRARY);
    let library_source = library.source.clone();
    let (db, snapshot, root) = analyze(library, CLIENT);
    assert_clean(&snapshot);
    assert!(snapshot.inputs().native_modules().is_empty());
    let file = snapshot.file(library_source.id()).unwrap();
    assert!(file.syntax().is_some());
    let facts = file.result().facts();
    let counter = &facts.lowered.module.native_types[0];
    assert_eq!(counter.fields.len(), 3);
    assert_eq!(counter.fields[0].writeability, Writeability::Var);
    assert_eq!(counter.fields[1].writeability, Writeability::Val);
    let child_type = facts
        .typed
        .type_table()
        .field_type(counter.fields[1].id)
        .unwrap();
    assert!(matches!(child_type, TypeId::Struct(_)), "{child_type:?}");
    for name in ["defaulted", "embedded", "script_identity"] {
        let function = facts
            .lowered
            .module
            .functions
            .iter()
            .find(|function| function.name == name)
            .unwrap();
        assert!(matches!(
            function.implementation,
            FunctionImplementation::Script(_)
        ));
        assert!(facts.typed.valid_bodies.contains(&function.id));
        assert_eq!(
            facts
                .typed
                .signatures()
                .function(function.id)
                .unwrap()
                .implementation,
            CheckedImplementation::Script
        );
    }
    let native = facts
        .lowered
        .module
        .functions
        .iter()
        .find(|function| function.name == "new")
        .unwrap();
    assert_eq!(
        facts
            .typed
            .signatures()
            .function(native.id)
            .unwrap()
            .implementation,
        CheckedImplementation::Native(Default::default())
    );
    let span = facts.lowered.module.functions[native.id.index()]
        .origin
        .diagnostic_span();
    assert!(library_source.text()[span.start..span.end].contains("native fn new"));
    let program = snapshot.check_program(root, &Default::default()).unwrap();
    assert!(matches!(
        CheckedCompilation::from_program(&program, &Default::default()),
        Err(AdmissionError::InvalidModule(
            "Native interface fields have no executable adapters"
        ))
    ));

    // Capturing/restoring keeps offline source and static metadata, not a provider.
    let captured = db.inputs();
    let mut restored = captured.database().unwrap();
    assert_eq!(
        db.environment_fingerprint(&Default::default()).unwrap(),
        restored
            .environment_fingerprint(&Default::default())
            .unwrap()
    );
    let rebuilt = restored
        .snapshot(snapshot.physical_sources().clone(), &Default::default())
        .unwrap();
    assert_clean(&rebuilt);
    assert!(Arc::ptr_eq(
        &captured.external_modules()[0].source,
        &library_source
    ));
}

#[test]
fn ordinary_native_syntax_is_checked_without_installed_bindings() {
    let source = Arc::new(SourceFile::new("ordinary.kgr", LIBRARY));
    let mut db = AnalysisDatabase::new("/project").unwrap();
    let snapshot = db
        .snapshot(
            SourceSnapshot::single_file(source.clone()),
            &Default::default(),
        )
        .unwrap();
    let file = snapshot.file(source.id()).unwrap();
    assert!(file.parsed.physical_diagnostics().is_empty());
    assert!(
        file.result().diagnostics().is_empty(),
        "{:?}",
        file.result().diagnostics()
    );
    let facts = file.result().facts();
    assert_eq!(facts.lowered.module.native_types.len(), 1);
    assert_eq!(
        facts
            .typed
            .signatures()
            .functions()
            .iter()
            .filter(|function| matches!(
                function.implementation,
                crate::semantic::signatures::FunctionImplementation::Native(_)
            ))
            .count(),
        5
    );
    assert!(
        snapshot
            .check_program(source.id(), &Default::default())
            .is_ok()
    );
}

#[test]
fn external_native_fields_and_calls_enforce_mutability_visibility_bounds_and_arity() {
    let mut library = input(LIBRARY);
    library.methods.insert(
        ExternalFunction::TraitMethod {
            owner: "Read".into(),
            name: "defaulted".into(),
        },
        MethodPolicy {
            override_allowed: false,
            ..Default::default()
        },
    );
    let (_, snapshot, root) = analyze(library, INVALID_CLIENT);
    let file = snapshot.file(root).unwrap();
    let diagnostics = file.result().diagnostics();
    assert!(
        diagnostics
            .iter()
            .any(|d| matches!(d.kind, DiagnosticKind::InvalidAssignmentTarget { .. })),
        "{diagnostics:?}"
    );
    assert!(
        diagnostics
            .iter()
            .any(|d| matches!(d.kind, DiagnosticKind::GenericBoundNotSatisfied { .. })),
        "{diagnostics:?}"
    );
    assert!(
        diagnostics.iter().any(|d| matches!(
            d.kind,
            DiagnosticKind::CallArityMismatch {
                expected: 0,
                found: 1,
                ..
            }
        )),
        "{diagnostics:?}"
    );
    for witness in ["secret", "hidden"] {
        let offset = INVALID_CLIENT.rfind(witness).unwrap();
        assert!(
            diagnostics.iter().any(
                |d| matches!(&d.kind, DiagnosticKind::UnknownName { name } if name == witness)
                    && d.span
                        .is_some_and(|span| span.start <= offset && offset < span.end)
            ),
            "{witness}: {diagnostics:?}"
        );
    }
    assert!(diagnostics.iter().any(|d| matches!(&d.kind, DiagnosticKind::TraitMethodMismatch { method_name, reason, .. } if method_name == "defaulted" && reason == "this method declaration forbids overriding")), "{diagnostics:?}");
    assert!(snapshot.check_program(root, &Default::default()).is_err());
}

fn constructors() -> ExternalModule {
    let mut module = input(CONSTRUCTORS);
    module.dependencies.extend([
        kagari_stdlib::namespaces::module("core", "ops"),
        kagari_stdlib::namespaces::module("alloc", "string"),
    ]);
    for (name, access) in [
        ("shared", ReferenceAccess::Shared),
        ("mutable", ReferenceAccess::Mutable),
    ] {
        module.methods.insert(
            ExternalFunction::ImplMethod {
                owner: 0,
                name: name.into(),
            },
            MethodPolicy {
                receiver_access: Some(access),
                ..Default::default()
            },
        );
    }
    module
}

#[test]
fn external_constructor_metadata_recognizes_identity_without_spelling_or_runtime_authority() {
    let library = constructors();
    let id = library.source.id();
    let client = include_str!("../../../tests/fixtures/analysis/external/constructor_client.kgr");
    let (_, snapshot, root) = analyze(library, client);
    assert_clean(&snapshot);
    let mut direct = AnalysisDatabase::new("/project").unwrap();
    let mut providers = standard_modules();
    providers.push(super::equivalence::constructor_registration());
    direct.set_native_modules(providers);
    let direct_snapshot = direct
        .snapshot(snapshot.physical_sources().clone(), &Default::default())
        .unwrap();
    assert_clean(&direct_snapshot);
    let context = DefinitionContext::new().unwrap();
    let canonical_types = |file: &FileAnalysis| {
        let types = file
            .signatures()
            .facts()
            .functions()
            .iter()
            .flat_map(|function| {
                function
                    .params
                    .iter()
                    .map(|param| param.ty.clone())
                    .chain([function.return_type.clone()])
            })
            .collect::<Vec<_>>();
        DefinitionMetadata::checked(file.definitions().clone(), types, &Default::default())
            .unwrap()
            .import_into(&context, &Default::default())
            .unwrap()
            .into_records()
    };
    assert_eq!(
        canonical_types(snapshot.file(root).unwrap()),
        canonical_types(direct_snapshot.file(root).unwrap())
    );
    let direct_library = direct_snapshot
        .files()
        .find(|file| file.source().module_identity().package.0 == "api")
        .unwrap();
    // Native binding provenance intentionally differs; declaration types do not.
    assert_eq!(
        canonical_types(snapshot.file(id).unwrap()),
        canonical_types(direct_library)
    );
    let signatures = snapshot.file(id).unwrap().signatures().facts();
    let functions = signatures.functions();
    let text = functions
        .iter()
        .find(|function| function.name == "text")
        .unwrap();
    assert_eq!(text.params[0].ty, TypeId::Builtin(BuiltinType::String));
    assert_eq!(text.return_type, text.params[0].ty);
    let range = functions
        .iter()
        .find(|function| function.name == "range")
        .unwrap();
    assert_eq!(
        range.return_type,
        TypeId::Range(
            Box::new(TypeId::Builtin(BuiltinType::I32)),
            RangeKind::Exclusive
        )
    );
    assert_eq!(range.params[0].ty, range.return_type);
    let (_, nominal, _) = analyze(
        input("pub native type String; pub native fn text(value: String) -> String;"),
        "",
    );
    let nominal = nominal
        .files()
        .find(|file| file.source().module_identity().package.0 == "api")
        .unwrap();
    let text = nominal
        .signatures()
        .facts()
        .functions()
        .iter()
        .find(|function| function.name == "text")
        .unwrap();
    assert!(
        matches!(text.return_type, TypeId::NativeObject(_)),
        "name alone must not create builtin identity"
    );
}

#[test]
fn foundation_reference_names_are_absent_in_both_declaration_routes() {
    let providers = standard_modules();
    for structured in [true, false] {
        let mut db = AnalysisDatabase::new("/project").unwrap();
        if structured {
            db.set_native_modules(providers.clone());
        } else {
            db.set_external_modules(super::foundation::exported(&providers));
        }
        let source = Arc::new(SourceFile::new(
            "/project/main.kgr",
            "fn former_shared(value: Ref<i32>) {} fn former_mutable(value: RefMut<i32>) {}",
        ));
        let root = source.id();
        let snapshot = db
            .snapshot(SourceSnapshot::single_file(source), &Default::default())
            .unwrap();
        let diagnostics = snapshot.file(root).unwrap().result().diagnostics();
        for name in ["former_shared", "former_mutable"] {
            assert!(
                diagnostics.iter().any(|d| matches!(
                    &d.kind,
                    DiagnosticKind::UnknownType { function_name, .. } if function_name == name
                )),
                "{name}: {diagnostics:?}"
            );
        }
    }
}

fn reference_named_registration(identity: ModuleIdentity) -> Arc<DefinitionMetadata<ModuleDecl>> {
    let context = DefinitionContext::new().unwrap();
    let root = context.intern_root(&identity).unwrap();
    let mut module = ModuleDecl::new(identity);
    module.package_alias = Some("core".into());
    for (name, arity) in [("Ref", 1), ("RefMut", 2)] {
        let owner = context
            .intern_child(root, DefinitionKind::AssociatedType, name, 0)
            .unwrap();
        let generic_params = (0..arity)
            .map(|position| GenericParam { owner, position })
            .collect::<Vec<_>>();
        module.types.push(TypeDef {
            name: name.into(),
            kind: TypeDefKind::NativeStorage(NativeStorageLayout::Opaque),
            fields: vec![FieldDef {
                name: "value".into(),
                visibility: Visibility::Public,
                ty: generic_params.last().unwrap().as_type(),
                mutable: false,
                host_policy: None,
            }],
            generic_params,
            bounds: vec![],
            variants: vec![],
        });
    }
    let definitions = module.collect_definitions(&context).unwrap();
    Arc::new(DefinitionMetadata::checked(definitions, module, &Default::default()).unwrap())
}

#[test]
fn former_reference_owner_admits_ordinary_natives_in_both_declaration_routes() {
    let identity = kagari_stdlib::namespaces::module("core", "host");
    let mut sources = SourceDatabase::new("/exports").unwrap();
    sources.bind_module("host.kgr", identity.clone()).unwrap();
    let id = sources
        .set(
            "host.kgr",
            include_str!("../../../tests/fixtures/analysis/external/reference_named_types.kgr")
                .into(),
            SourceLayer::Base,
        )
        .unwrap();
    let mut declarations = ExternalModule::new(sources.snapshot().file(id).unwrap().clone());
    declarations.package_alias = Some("core".into());
    let source = Arc::new(SourceFile::new(
        "/project/main.kgr",
        include_str!("../../../tests/fixtures/analysis/external/reference_named_client.kgr"),
    ));
    let root = source.id();
    for structured in [true, false] {
        let mut db = AnalysisDatabase::new("/project").unwrap();
        if structured {
            db.set_native_modules(vec![reference_named_registration(identity.clone())]);
        } else {
            db.set_external_modules(vec![declarations.clone()]);
        }
        let snapshot = db
            .snapshot(
                SourceSnapshot::single_file(source.clone()),
                &Default::default(),
            )
            .unwrap();
        let file = snapshot.file(root).unwrap();
        let diagnostics = file.result().diagnostics();
        let errors = diagnostics
            .iter()
            .filter(|d| d.severity == Severity::Error)
            .collect::<Vec<_>>();
        assert!(
            matches!(errors.as_slice(), [diagnostic] if matches!(
                &diagnostic.kind,
                DiagnosticKind::UnknownType { function_name, .. } if function_name == "invalid"
            )),
            "structured={structured}: {diagnostics:?}"
        );
        let store = file
            .signatures()
            .facts()
            .functions()
            .iter()
            .find(|function| function.name == "store")
            .unwrap();
        let TypeId::NativeObject(outer) = &store.params[0].ty else {
            panic!(
                "expected an ordinary native object: {:?}",
                store.params[0].ty
            );
        };
        let TypeId::NativeObject(inner) = &store.return_type else {
            panic!(
                "expected an ordinary native object: {:?}",
                store.return_type
            );
        };
        assert_ne!(outer.declaration, inner.declaration);
        assert_eq!(
            outer.arguments,
            [
                TypeId::Builtin(BuiltinType::Bool),
                store.return_type.clone()
            ]
        );
        assert_eq!(inner.arguments, [TypeId::Builtin(BuiltinType::I32)]);
    }
}

#[test]
fn missing_dependencies_and_invalid_external_metadata_are_not_admitted() {
    let missing = input(include_str!(
        "../../../tests/fixtures/analysis/external/missing_dependency.kgr"
    ));
    let id = missing.source.id();
    let (_, snapshot, _) = analyze(missing, "");
    let diagnostics = snapshot.file(id).unwrap().result().diagnostics();
    assert!(
        diagnostics
            .iter()
            .any(|d| matches!(d.kind, DiagnosticKind::UnknownTrait { .. })),
        "{diagnostics:?}"
    );
    assert!(
        diagnostics
            .iter()
            .any(|d| matches!(d.kind, DiagnosticKind::UnknownType { .. })),
        "{diagnostics:?}"
    );
    assert!(snapshot.check_program(id, &Default::default()).is_err());

    let mut db = AnalysisDatabase::new("/project").unwrap();
    let library = constructors();
    db.set_external_modules(vec![library.clone(), library.clone()]);
    assert!(matches!(
        db.snapshot(SourceSnapshot::default(), &Default::default()),
        Err(AnalysisError::NativeApi(_))
    ));
    db.set_external_modules(vec![library.clone()]);
    assert!(matches!(
        db.snapshot(SourceSnapshot::single_file(library.source.clone()), &Default::default()),
        Err(AnalysisError::NativeApi(error)) if error.0.contains("source conflicts")
    ));
    let mut providers = standard_modules();
    providers.push(super::equivalence::constructor_registration());
    db.set_native_modules(providers);
    assert!(matches!(
        db.snapshot(SourceSnapshot::default(), &Default::default()),
        Err(AnalysisError::NativeApi(error)) if error.0.contains("conflicts with installed")
    ));
    db.set_native_modules(vec![]);
    let mut owner_sources = SourceDatabase::new("/owners").unwrap();
    let mut owner_inputs = Vec::new();
    for package in ["left", "right"] {
        let uri = format!("{package}.kgr");
        owner_sources
            .bind_module(
                &uri,
                ModuleIdentity {
                    package: PackageId(package.into()),
                    path: vec![],
                },
            )
            .unwrap();
        let id = owner_sources
            .set(
                &uri,
                include_str!("../../../tests/fixtures/analysis/external/constructor_owners.kgr")
                    .into(),
                SourceLayer::Base,
            )
            .unwrap();
        let mut owner = ExternalModule::new(owner_sources.snapshot().file(id).unwrap().clone());
        for name in ["Text", "Alias"] {
            owner.types.insert(
                name.into(),
                ExternalType::Constructor(NativeTypeConstructor::String),
            );
        }
        owner_inputs.push(owner);
    }
    // An arbitrary module cannot grant constructor identity, even for a local alias.
    db.set_external_modules(vec![owner_inputs[0].clone()]);
    assert!(
        matches!(db.declarations(SourceSnapshot::default(), &Default::default()), Err(AnalysisError::NativeApi(error)) if error.0.contains("invalid external constructor shape"))
    );
    db.set_external_modules(owner_inputs);
    assert!(
        matches!(db.declarations(SourceSnapshot::default(), &Default::default()), Err(AnalysisError::NativeApi(error)) if error.0.contains("invalid external constructor shape"))
    );
    db.set_native_modules(vec![]);
    let mut malformed = library;
    malformed.types.insert(
        "Target".into(),
        ExternalType::Constructor(NativeTypeConstructor::String),
    );
    db.set_external_modules(vec![malformed]);
    assert!(matches!(
        db.snapshot(SourceSnapshot::default(), &Default::default()),
        Err(AnalysisError::NativeApi(_))
    ));
    assert!(db.declaration_cache.is_none());
    // Constructor identity itself imposes shape, even when no sidecar is supplied.
    for (package, module, name, constructor, valid, wrong_arity, fields) in [
        (
            "alloc",
            "string",
            "String",
            NativeTypeConstructor::String,
            "pub native type String;",
            "pub native type String<T>;",
            "pub native type String { pub val value: i32 }",
        ),
        (
            "core",
            "ops",
            "Range",
            NativeTypeConstructor::Range(RangeKind::Exclusive),
            "pub native type Range<T>;",
            "pub native type Range;",
            "pub native type Range<T> { pub val value: i32 }",
        ),
        (
            "core",
            "ops",
            "RangeFull",
            NativeTypeConstructor::Range(RangeKind::Full),
            "pub native type RangeFull;",
            "pub native type RangeFull<T>;",
            "pub native type RangeFull { pub val value: i32 }",
        ),
    ] {
        let mut shapes = AnalysisDatabase::new("/constructor-shapes").unwrap();
        let owner = kagari_stdlib::namespaces::module(package, module);
        let bundled = packaged_modules()
            .iter()
            .find(|input| input.source.module_identity() == &owner)
            .unwrap();
        for (text, accepted) in [(valid, true), (wrong_arity, false), (fields, false)] {
            let mut declaration = bundled.clone();
            super::foundation::change_export(&mut declaration, |source| {
                assert!(source.contains(valid));
                source.replace(valid, text)
            });
            for sidecar in [false, true] {
                let mut declaration = declaration.clone();
                if sidecar {
                    declaration
                        .types
                        .insert(name.into(), ExternalType::Constructor(constructor));
                }
                shapes.replace_core_fixture(declaration);
                let result = shapes.declarations(SourceSnapshot::default(), &Default::default());
                if accepted {
                    assert!(result.is_ok(), "{text}, sidecar={sidecar}: {result:?}");
                } else {
                    assert!(
                        matches!(result, Err(AnalysisError::NativeApi(ref error)) if error.0.contains("invalid language constructor declaration shape")),
                        "{text}, sidecar={sidecar}: {result:?}"
                    );
                }
            }
        }
    }

    let locked = MethodPolicy {
        override_allowed: false,
        ..Default::default()
    };
    let loan = MethodPolicy {
        receiver_access: Some(ReferenceAccess::Shared),
        ..Default::default()
    };
    for (selector, policy) in [
        (ExternalFunction::Function("bounded".into()), locked),
        (
            ExternalFunction::TraitMethod {
                owner: "Read".into(),
                name: "read".into(),
            },
            locked,
        ),
        (
            ExternalFunction::TraitMethod {
                owner: "Read".into(),
                name: "defaulted".into(),
            },
            loan,
        ),
        (
            ExternalFunction::ImplMethod {
                owner: 0,
                name: "new".into(),
            },
            loan,
        ),
        (
            ExternalFunction::ImplMethod {
                owner: 0,
                name: "echo".into(),
            },
            locked,
        ),
    ] {
        let mut invalid = input(LIBRARY);
        invalid.methods.insert(selector, policy);
        db.set_external_modules(vec![invalid]);
        assert!(
            matches!(db.snapshot(SourceSnapshot::default(), &Default::default()), Err(AnalysisError::NativeApi(error)) if error.0.contains("invalid external method policy"))
        );
    }
    db.set_external_modules(vec![constructors()]);
    let cancel = CancellationToken::default();
    cancel.cancel();
    assert!(matches!(
        db.snapshot(SourceSnapshot::default(), &cancel),
        Err(AnalysisError::Cancelled)
    ));
    assert!(db.declaration_cache.is_none());
}
