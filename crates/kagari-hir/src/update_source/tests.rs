use crate::{
    CheckedAnalysis,
    analysis::AnalysisDatabase,
    analyze_source,
    update_source::{
        FactKind, SourceBodyKind, SourceBudget, SourceEvidence, SourceRootKind, UnknownDependency,
        collect,
    },
};
use kagari_common::{
    cancellation::CancellationToken,
    identity::{
        ModuleIdentity, PackageId, map::DefinitionContext, mapping::DefinitionMappingError,
        metadata::DefinitionMetadata,
    },
};
use kagari_source::{
    source::SourceFile,
    source_database::{SourceDatabase, SourceLayer},
};
use kagari_stdlib::catalog;

fn checked(text: &str) -> CheckedAnalysis {
    analyze_source(
        &SourceFile::new("update.kgr", text),
        catalog::shared().unwrap(),
    )
    .unwrap()
    .into_codegen()
    .unwrap()
}

fn evidence(module: &CheckedAnalysis) -> SourceEvidence {
    collect(
        module,
        &mut SourceBudget::new(&CancellationToken::default()),
    )
    .unwrap()
}

#[test]
fn initializer_retains_parentheses_and_literal_trivia() {
    let module = checked(
        "static val N: i32 = ((1 + 2)); static val S: String = (r#\" // keep /* this */ \"#);",
    );
    let items = &module.lowered.module.statics;
    let map = &module.lowered.source_map;
    let span = map.static_initializer_span(items[0].id).unwrap();
    assert_eq!(
        &module.lowered.source.text()[span.start..span.end],
        "((1 + 2))"
    );
    assert_ne!(span, map.expr_span(items[0].initializer));
    let records = evidence(&module);
    let first = &records.bodies[0];
    let range = first.initializer.unwrap();
    assert_eq!(
        first.tokens[range.start..range.end]
            .iter()
            .map(|token| token.text.as_str())
            .collect::<Vec<_>>(),
        ["(", "(", "1", "+", "2", ")", ")"]
    );
    assert!(
        records.bodies[1]
            .tokens
            .iter()
            .any(|token| token.text == "r#\" // keep /* this */ \"#")
    );
    assert_eq!(
        records
            .roots
            .iter()
            .filter(|root| root.kind == SourceRootKind::StaticInitializer)
            .count(),
        2
    );
}

#[test]
fn canonical_evidence_ignores_offsets_and_trivia_but_not_computation() {
    let left = checked(
        "const C: i32 = 2; fn unused<T>(x: T) -> i32 { C + 1 } static val N: i32 = ((C + 1));",
    );
    let right = checked(
        "\nconst C:i32=2;\nfn unused<T>(x:T)->i32{/*comment*/ C+1}\nstatic val N:i32=((C /*keep resolution*/+1));",
    );
    let context = DefinitionContext::new().unwrap();
    let import = |module: &CheckedAnalysis| {
        DefinitionMetadata::checked(
            module.definitions().clone(),
            evidence(module),
            &Default::default(),
        )
        .unwrap()
        .import_into(&context, &Default::default())
        .unwrap()
    };
    assert_eq!(import(&left).records(), import(&right).records());
    let changed = checked(
        "const C: i32 = 2; fn unused<T>(x: T) -> i32 { C + 2 } static val N: i32 = ((C + 1));",
    );
    assert_ne!(import(&left).records(), import(&changed).records());
    let records = evidence(&left);
    let body = records
        .bodies
        .iter()
        .find(|body| !body.parameters.is_empty())
        .unwrap();
    assert_eq!(body.parameters.len(), 1);
    assert!(
        body.facts
            .iter()
            .any(|fact| fact.kind == FactKind::Reference)
    );
}

#[test]
fn named_values_and_closures_are_not_classified_by_capture_count() {
    let module = checked(
        "fn outer() -> fn(i32) -> i32 { fn id<T>(x: T) -> T { x } val closure = |x: i32| x; id }",
    );
    let records = evidence(&module);
    assert_eq!(records.bodies.len(), 2);
    assert!(
        records
            .bodies
            .iter()
            .flat_map(|body| &body.facts)
            .any(|fact| fact.kind == FactKind::FunctionValue)
    );
    assert!(
        records
            .bodies
            .iter()
            .flat_map(|body| &body.facts)
            .any(|fact| fact.kind == FactKind::Closure)
    );
    assert!(
        records
            .bodies
            .iter()
            .all(|body| body.kind == SourceBodyKind::Function)
    );
}

#[test]
fn value_dispatch_is_unknown_and_collection_limits_fail_closed() {
    let module = checked("fn invoke(f: fn(i32) -> i32) -> i32 { f(1) }");
    assert!(
        evidence(&module)
            .bodies
            .iter()
            .flat_map(|body| &body.facts)
            .any(|fact| matches!(
                fact.kind,
                FactKind::Unknown(
                    UnknownDependency::InterfaceDispatch | UnknownDependency::FunctionValue
                )
            ))
    );
    let cancel = CancellationToken::default();
    let mut budget = SourceBudget::new(&cancel);
    budget.records = 0;
    assert_eq!(
        collect(&module, &mut budget),
        Err(DefinitionMappingError::LimitExceeded)
    );
    cancel.cancel();
    assert_eq!(
        collect(&module, &mut SourceBudget::new(&cancel)),
        Err(DefinitionMappingError::Cancelled)
    );
}

#[test]
fn unused_key_templates_preserve_inherited_equality_and_default_origins() {
    let module = checked(
        r#"
use std::hash::Hash;
struct Key<T> { val value: T }
impl<T: PartialEq> PartialEq for Key<T> { fn eq(self, other: Self) -> bool { self.value == other.value } }
impl<T: Eq> Eq for Key<T> {}
impl<T: Eq + Hash> Hash for Key<T> { fn hash(self) -> i64 { self.value.hash() } }
fn leaf() -> i32 { 7 }
trait Helper { fn run(self) -> i32 { leaf() } }
impl<T> Helper for Key<T> {}
"#,
    );
    let records = evidence(&module);
    let equality = records
        .roots
        .iter()
        .find(|root| root.kind == SourceRootKind::Equality)
        .unwrap();
    assert!(equality.members.iter().any(|member| {
        module
            .definitions()
            .resolve(*member)
            .unwrap()
            .last()
            .unwrap()
            .name
            == "eq"
    }));
    let hash = records
        .roots
        .iter()
        .find(|root| root.kind == SourceRootKind::Hash)
        .unwrap();
    assert!(hash.receiver.as_ref().is_some_and(|ty| !ty.is_concrete()));
    assert!(hash.members.iter().any(|member| {
        module
            .definitions()
            .resolve(*member)
            .unwrap()
            .last()
            .unwrap()
            .name
            == "hash"
    }));
    assert!(
        records
            .selections
            .iter()
            .any(|fact| fact.kind == FactKind::Selection && fact.targets.len() == 3)
    );
    assert!(
        records
            .bodies
            .iter()
            .flat_map(|body| &body.facts)
            .any(|fact| fact.kind == FactKind::Call
                && fact.targets.iter().any(|target| module
                    .definitions()
                    .resolve(*target)
                    .unwrap()
                    .last()
                    .unwrap()
                    .name
                    == "leaf"))
    );
}

#[test]
fn identical_body_tokens_retain_retargeted_import_identity() {
    let mut sources = SourceDatabase::default();
    for name in ["left", "right", "app"] {
        sources
            .bind_module(
                &format!("{name}.kgr"),
                ModuleIdentity {
                    package: PackageId("pkg".into()),
                    path: vec![name.into()],
                },
            )
            .unwrap();
    }
    for name in ["left", "right"] {
        sources
            .set(
                &format!("{name}.kgr"),
                "pub fn target() -> i32 { 1 } pub const VALUE: i32 = 2; pub static val CELL: i32 = 3;".into(),
                SourceLayer::Base,
            )
            .unwrap();
    }
    let root = sources
        .set(
            "app.kgr",
            "use pkg::left::{target as chosen, VALUE, CELL}; fn untouched<T>() -> i32 { chosen() + VALUE + CELL }".into(),
            SourceLayer::Base,
        )
        .unwrap();
    let mut database = AnalysisDatabase::default();
    database.set_native_modules(catalog::shared().unwrap());
    let before = database
        .snapshot(sources.snapshot(), &Default::default())
        .unwrap()
        .check_program(root, &Default::default())
        .unwrap();
    sources
        .set(
            "app.kgr",
            "use pkg::right::{target as chosen, VALUE, CELL}; fn untouched<T>() -> i32 { chosen() + VALUE + CELL }".into(),
            SourceLayer::Base,
        )
        .unwrap();
    let after = database
        .snapshot(sources.snapshot(), &Default::default())
        .unwrap()
        .check_program(root, &Default::default())
        .unwrap();
    let before_records = evidence(before.root());
    let after_records = evidence(after.root());
    assert_eq!(
        before_records.bodies[0].tokens,
        after_records.bodies[0].tokens
    );
    let before_target = before_records.bodies[0]
        .facts
        .iter()
        .find(|fact| fact.kind == FactKind::Call)
        .unwrap()
        .targets[0];
    let after_target = after_records.bodies[0]
        .facts
        .iter()
        .find(|fact| fact.kind == FactKind::Call)
        .unwrap()
        .targets[0];
    assert_eq!(
        before
            .root()
            .definitions()
            .resolve(before_target)
            .unwrap()
            .module()
            .path,
        ["left"]
    );
    assert_eq!(
        after
            .root()
            .definitions()
            .resolve(after_target)
            .unwrap()
            .module()
            .path,
        ["right"]
    );
    assert_ne!(before_target, after_target);
    for (program, records, path) in [
        (&before, &before_records, "left"),
        (&after, &after_records, "right"),
    ] {
        for name in ["VALUE", "CELL"] {
            assert!(records.bodies[0].facts.iter().any(|fact| {
                fact.kind == FactKind::Reference
                    && fact.targets.iter().any(|target| {
                        let resolved = program.root().definitions().resolve(*target).unwrap();
                        resolved.last().unwrap().name == name && resolved.module().path == [path]
                    })
            }));
        }
    }
}

#[test]
fn native_declaration_requirements_survive_source_collection() {
    let mut sources = SourceDatabase::default();
    let root = sources
        .set(
            "native.kgr",
            "fn use_vector(xs: Vec<i32>) { xs.dedup(); }".into(),
            SourceLayer::Base,
        )
        .unwrap();
    let mut database = AnalysisDatabase::default();
    database.set_native_modules(catalog::shared().unwrap());
    let program = database
        .snapshot(sources.snapshot(), &Default::default())
        .unwrap()
        .check_program(root, &Default::default())
        .unwrap();
    let mut requirements = 0;
    for module in program.modules() {
        if !module
            .lowered
            .registered_native_declarations()
            .iter()
            .any(|native| !native.callable_requirements.is_empty())
        {
            continue;
        }
        let records = evidence(module);
        for native in module.lowered.registered_native_declarations() {
            let body = records
                .bodies
                .iter()
                .find(|body| body.declaration == native.declaration)
                .unwrap();
            for requirement in &native.callable_requirements {
                requirements += 1;
                assert!(
                    body.facts
                        .iter()
                        .any(|fact| fact.kind == FactKind::NativeRequirement
                            && fact.targets == [requirement.member])
                );
            }
            assert!(
                body.facts
                    .iter()
                    .any(|fact| fact.kind == FactKind::Unknown(UnknownDependency::NativeReentry))
            );
        }
    }
    assert!(requirements > 0);
}

#[test]
fn async_creation_captures_and_native_reentry_are_explicit() {
    let module = checked(
        "async fn task(x: i32) -> i32 { val pending = async { x }; pending.await } fn native() -> u64 { val xs: Vec<i32> = Vec::new(); xs.len() }",
    );
    let records = evidence(&module);
    let facts: Vec<_> = records.bodies.iter().flat_map(|body| &body.facts).collect();
    assert!(facts.iter().any(|fact| fact.kind == FactKind::AsyncBlock));
    assert!(facts.iter().any(|fact| fact.kind == FactKind::Await));
    assert!(
        facts
            .iter()
            .any(|fact| fact.kind == FactKind::Unknown(UnknownDependency::FutureResume))
    );
    assert!(
        facts
            .iter()
            .any(|fact| fact.kind == FactKind::Capture && !fact.related.is_empty())
    );
    assert!(
        facts
            .iter()
            .any(|fact| fact.kind == FactKind::Unknown(UnknownDependency::NativeReentry))
    );
}
