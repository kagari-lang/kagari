use super::foundation::publish;
use crate::tests::standard::standard_modules;
use crate::{
    analysis::{
        AnalysisDatabase, AnalysisSnapshot, FileAnalysis,
        core_source::{change_core, packaged_modules},
        error::AnalysisError,
    },
    compile_input::program::{AdmissionError, CheckedCompilation},
    hir::{item::function::FunctionImplementation, writeability::Writeability},
    imports::environment::{ImportEnvironment, ModuleScope},
    semantic::signatures::FunctionImplementation as CheckedImplementation,
    types::TypeId,
};
use kagari_common::identity::{
    DefinitionKind, ModuleIdentity, PackageId, map::DefinitionContext, metadata::DefinitionMetadata,
};
use kagari_source::{
    diagnostic::{DiagnosticKind, Severity},
    identity::FileId,
    source::SourceFile,
    source_database::{SourceDatabase, SourceLayer, SourceSnapshot},
};
use kagari_types::{
    declaration::{
        FieldDef, TypeDef, TypeDefKind, module::ModuleDecl, native::NativeStorageLayout,
    },
    range::RangeKind,
    scalar::BuiltinType,
    ty::GenericParam,
    visibility::Visibility,
};
use std::sync::Arc;

const LIBRARY: &str = include_str!("../../../tests/fixtures/analysis/external/library.kgr");
const CLIENT: &str = include_str!("../../../tests/fixtures/analysis/external/client.kgr");
const INVALID_CLIENT: &str =
    include_str!("../../../tests/fixtures/analysis/external/invalid_client.kgr");
const CONSTRUCTORS: &str =
    include_str!("../../../tests/fixtures/analysis/external/constructors.kgr");

fn analyze(library: &str, text: &str) -> (AnalysisDatabase, AnalysisSnapshot, FileId, FileId) {
    let mut sources = SourceDatabase::new("/project").unwrap();
    let api = ModuleIdentity {
        package: PackageId("api".into()),
        path: vec![],
    };
    sources.bind_module("api.kgr", api.clone()).unwrap();
    let lib = sources
        .set("api.kgr", library.into(), SourceLayer::Base)
        .unwrap();
    let root = sources
        .set("main.kgr", text.into(), SourceLayer::Base)
        .unwrap();
    let files = sources.snapshot();
    let mut db = AnalysisDatabase::new("/project").unwrap();
    db.set_import_environment(ImportEnvironment {
        modules: [(
            files.file(root).unwrap().module_identity().clone(),
            ModuleScope {
                dependencies: [api].into(),
                ..Default::default()
            },
        )]
        .into(),
    });
    let snapshot = db.snapshot(files, &Default::default()).unwrap();
    (db, snapshot, root, lib)
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
    let (db, snapshot, root, lib) = analyze(LIBRARY, CLIENT);
    let library_source = snapshot.physical_sources().file(lib).unwrap();
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

    // Replay the actual documents with the captured import configuration.
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
        rebuilt.physical_sources().file(lib).unwrap(),
        library_source
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
    let (_, snapshot, root, _) = analyze(LIBRARY, INVALID_CLIENT);
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
    assert!(snapshot.check_program(root, &Default::default()).is_err());
}

#[test]
fn both_routes_recognize_reserved_constructors_without_spelling_or_runtime_authority() {
    let client = include_str!("../../../tests/fixtures/analysis/external/constructor_client.kgr");
    let (_, snapshot, root, id) = analyze(CONSTRUCTORS, client);
    assert_clean(&snapshot);
    let mut direct = AnalysisDatabase::new("/project").unwrap();
    let mut providers = standard_modules();
    providers.push(super::equivalence::constructor_registration());
    direct.set_native_modules(providers);
    let direct_snapshot = direct
        .snapshot(
            SourceSnapshot::single_file(snapshot.physical_sources().file(root).unwrap().clone()),
            &Default::default(),
        )
        .unwrap();
    // Target is an ordinary GC struct. Only the structured provider advertises
    // Rust loans; both routes select its declared methods without storage authority.
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
    let (_, nominal, _, _) = analyze(
        "pub native type String; pub native fn text(value: String) -> String;",
        "",
    );
    assert_clean(&nominal);
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
    let TypeId::NativeObject(returned) = &text.return_type else {
        panic!("name alone must not create builtin identity");
    };
    let definitions = nominal.definitions();
    let root = definitions
        .lookup_root(nominal.source().module_identity())
        .unwrap();
    assert_eq!(
        Some(returned.declaration),
        definitions.lookup_child(root, DefinitionKind::AssociatedType, "String", 0)
    );
}

#[test]
fn reserved_constructors_require_the_declared_shape() {
    // Change only the declaration in the actual canonical module.
    for (package, module, valid, invalid) in [
        (
            "alloc",
            "string",
            "pub native type String;",
            &[
                "pub native type String<T>;",
                "pub native type String { pub val value: i32 }",
                "native type String;",
            ][..],
        ),
        (
            "core",
            "ops",
            "pub native type Range<T>;",
            &[
                "pub native type Range;",
                "pub native type Range<T> { pub val value: i32 }",
                "pub native type Range<T: RangeBounds<T>>;",
            ][..],
        ),
        (
            "core",
            "ops",
            "pub native type RangeFull;",
            &[
                "pub native type RangeFull<T>;",
                "pub native type RangeFull { pub val value: i32 }",
            ][..],
        ),
    ] {
        let mut shapes = AnalysisDatabase::new("/constructor-shapes").unwrap();
        let owner = kagari_stdlib::namespaces::module(package, module);
        let bundled = packaged_modules()
            .iter()
            .find(|input| input.module_identity() == &owner)
            .unwrap();
        for text in invalid {
            let mut declaration = bundled.clone();
            change_core(&mut declaration, |source| {
                assert!(source.contains(valid));
                source.replace(valid, text)
            });
            shapes.replace_core_fixture(declaration);
            let result = shapes.declarations(SourceSnapshot::default(), &Default::default());
            assert!(
                matches!(result, Err(AnalysisError::NativeApi(ref error)) if error.0.contains("invalid language constructor declaration shape")),
                "{text}: {result:?}"
            );
        }
    }
}

#[test]
fn foundation_reference_names_are_absent_in_both_declaration_routes() {
    let mut sources = SourceDatabase::default();
    let root = sources
        .set(
            "main.kgr",
            "fn former_shared(value: Ref<i32>) {} fn former_mutable(value: RefMut<i32>) {}".into(),
            SourceLayer::Base,
        )
        .unwrap();
    let mut direct = AnalysisDatabase::default();
    direct.set_native_modules(standard_modules());
    let structured = direct
        .snapshot(sources.snapshot(), &Default::default())
        .unwrap();
    let mut offline = publish(structured.declaration_snapshot(), &mut sources);
    let parsed = offline
        .snapshot(sources.snapshot(), &Default::default())
        .unwrap();
    for snapshot in [structured, parsed] {
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
    let mut sources = SourceDatabase::default();
    let root = sources
        .set(
            "main.kgr",
            include_str!("../../../tests/fixtures/analysis/external/reference_named_client.kgr")
                .into(),
            SourceLayer::Base,
        )
        .unwrap();
    let mut direct = AnalysisDatabase::default();
    direct.set_native_modules(vec![reference_named_registration(identity.clone())]);
    let registered = direct
        .snapshot(sources.snapshot(), &Default::default())
        .unwrap();
    let mut offline = AnalysisDatabase::default();
    offline.set_import_environment(
        registered
            .declaration_snapshot()
            .import_environment()
            .clone(),
    );
    sources.bind_module("host.kgr", identity).unwrap();
    sources
        .set(
            "host.kgr",
            include_str!("../../../tests/fixtures/analysis/external/reference_named_types.kgr")
                .into(),
            SourceLayer::Base,
        )
        .unwrap();
    let parsed = offline
        .snapshot(sources.snapshot(), &Default::default())
        .unwrap();
    for (structured, snapshot) in [(true, registered), (false, parsed)] {
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
fn missing_source_dependencies_are_not_admitted() {
    let (_, snapshot, _, id) = analyze(
        include_str!("../../../tests/fixtures/analysis/external/missing_dependency.kgr"),
        "",
    );
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

    let mut db = AnalysisDatabase::default();
    db.set_native_modules(vec![super::equivalence::constructor_registration()]);
    assert!(
        matches!(db.declarations(snapshot.physical_sources().clone(), &Default::default()), Err(AnalysisError::NativeApi(error)) if error.0.contains("source conflicts with library"))
    );
}
