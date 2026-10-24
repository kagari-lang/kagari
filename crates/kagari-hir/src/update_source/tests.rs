use crate::{
    CheckedAnalysis,
    analysis::AnalysisDatabase,
    analyze_source,
    update_source::{FactKind, SourceBodyKind, SourceBudget, SourceEvidence, collect},
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
    assert_eq!(range.start, 0);
    assert_eq!(range.end, first.tokens.len());
    assert_eq!(
        first
            .tokens
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
    assert_eq!(records.initializers.len(), 2);
    for body in &records.bodies {
        for fact in &body.facts {
            assert!(fact.site.start <= fact.site.end && fact.site.end <= body.tokens.len());
        }
    }
}

#[test]
fn static_evidence_ignores_trivia_and_callee_contents_but_not_its_expression() {
    let source = "const C: i32 = 2; fn helper() -> i32 { C + 1 } fn unused<T>(x: T) -> i32 { 1 } static val N: i32 = ((helper() + C));";
    let left = checked(source);
    let trivia = checked(
        "const C:i32=2; fn helper()->i32{ C+1 } fn unused<T>(x:T)->i32{1} static val N:i32=((helper() /* comment */ + C));",
    );
    let changed_callees = checked(&source.replace("C + 1", "C + 2").replace("{ 1 }", "{ 3 }"));
    let changed_expression = checked(&source.replace("helper() + C", "helper() + 2"));
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
    assert_eq!(import(&left).records(), import(&trivia).records());
    assert_eq!(import(&left).records(), import(&changed_callees).records());
    assert_ne!(
        import(&left).records(),
        import(&changed_expression).records()
    );
    for body in evidence(&left).bodies {
        if body.kind != SourceBodyKind::StaticInitializer {
            assert!(body.tokens.is_empty());
            assert!(body.facts.is_empty());
            assert!(body.initializer.is_none());
        }
    }
}

#[test]
fn key_templates_and_native_calls_do_not_create_protection_roots() {
    let module = checked(
        r#"
use std::hash::Hash;
struct Key<T> { val value: T }
impl<T: PartialEq> PartialEq for Key<T> { fn eq(self, other: Self) -> bool { self.value == other.value } }
impl<T: Eq> Eq for Key<T> {}
impl<T: Eq + Hash> Hash for Key<T> { fn hash(self) -> i64 { self.value.hash() } }
fn native() -> u64 { val xs: Vec<i32> = Vec::new(); xs.len() }
"#,
    );
    let records = evidence(&module);
    assert!(records.initializers.is_empty());
    assert!(!records.bodies.is_empty());
    assert!(
        records
            .bodies
            .iter()
            .all(|body| body.tokens.is_empty() && body.facts.is_empty())
    );
}

#[test]
fn static_dynamic_calls_keep_local_facts_and_collection_limits_fail_closed() {
    let module = checked(
        "fn leaf() -> i32 { 3 } static val N: i32 = { val call: fn() -> i32 = || leaf(); call() }; static val SIZE: u64 = { val xs: Vec<i32> = Vec::new(); xs.len() };",
    );
    let records = evidence(&module);
    let facts = records
        .bodies
        .iter()
        .flat_map(|body| &body.facts)
        .collect::<Vec<_>>();
    assert!(facts.iter().any(|fact| fact.kind == FactKind::Closure));
    assert!(
        facts
            .iter()
            .any(|fact| fact.kind == FactKind::Call && fact.targets.is_empty())
    );
    assert!(facts.iter().any(|fact| fact.kind == FactKind::Signature));
    assert_eq!(records.initializers.len(), 2);
    let cancel = CancellationToken::default();
    let mut budget = SourceBudget::new(&cancel);
    budget.records = 0;
    assert_eq!(
        collect(&module, &mut budget),
        Err(DefinitionMappingError::LimitExceeded)
    );
    let mut budget = SourceBudget::new(&cancel);
    budget.bytes = 0;
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
fn identical_initializer_tokens_retain_direct_import_identity() {
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
        sources.set(
            &format!("{name}.kgr"),
            "pub fn target() -> i32 { 1 } pub const VALUE: i32 = 2; pub static val CELL: i32 = 3;".into(),
            SourceLayer::Base,
        ).unwrap();
    }
    let root = sources.set(
        "app.kgr",
        "use pkg::left::{target as chosen, VALUE, CELL}; static val N: i32 = chosen() + VALUE + CELL;".into(),
        SourceLayer::Base,
    ).unwrap();
    let mut database = AnalysisDatabase::default();
    database.set_native_modules(catalog::shared().unwrap());
    let before = database
        .snapshot(sources.snapshot(), &Default::default())
        .unwrap()
        .check_program(root, &Default::default())
        .unwrap();
    sources.set(
        "app.kgr",
        "use pkg::right::{target as chosen, VALUE, CELL}; static val N: i32 = chosen() + VALUE + CELL;".into(),
        SourceLayer::Base,
    ).unwrap();
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
    for (program, records, path) in [
        (&before, &before_records, "left"),
        (&after, &after_records, "right"),
    ] {
        for (name, kind) in [
            ("target", FactKind::Call),
            ("VALUE", FactKind::Reference),
            ("CELL", FactKind::Reference),
        ] {
            assert!(records.bodies[0].facts.iter().any(|fact| fact.kind == kind
                && fact.targets.iter().any(|target| {
                    let resolved = program.root().definitions().resolve(*target).unwrap();
                    resolved.last().unwrap().name == name && resolved.module().path == [path]
                })));
        }
    }
}
