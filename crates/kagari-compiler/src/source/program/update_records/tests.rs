use super::*;
use crate::source::lower::{
    instances::MirLoweringOptions, lower_to_mir_with_requests, optimize_module,
};
use kagari_contract::updates::BodyClassification;
use kagari_hir::{analysis::AnalysisDatabase, analyze_source};
use kagari_mir::{instruction::Instruction, verify::VerifiedMirModule};
use kagari_source::{
    source::SourceFile,
    source_database::{SourceDatabase, SourceLayer},
};
use kagari_stdlib::catalog;

#[test]
fn unused_templates_and_exact_initializers_reach_contract_records() {
    let mut sources = SourceDatabase::default();
    let root = sources.set("updates.kgr", r#"
use std::hash::Hash;
struct Key<T> { val value: T }
impl<T: PartialEq> PartialEq for Key<T> { fn eq(self, other: Self) -> bool { self.value == other.value } }
impl<T: Eq> Eq for Key<T> {}
impl<T: Eq + Hash> Hash for Key<T> { fn hash(self) -> i64 { self.value.hash() } }
const C: i32 = 2;
fn unused<T>(x: T) -> i32 { C + 1 }
static val N: i32 = ((C + 1));
"#.into(), SourceLayer::Base).unwrap();
    let mut database = AnalysisDatabase::default();
    database.set_native_modules(catalog::shared().unwrap());
    let checked = database
        .snapshot(sources.snapshot(), &Default::default())
        .unwrap()
        .check_program(root, &Default::default())
        .unwrap();
    let inputs = collect(&checked, &Default::default()).unwrap();
    let published =
        crate::source::program::lower_program_to_mir(&checked, &Default::default()).unwrap();
    assert_eq!(
        published.update_evidence().sources.len(),
        inputs.sources.len()
    );
    let definitions = checked.root().definitions();
    let owner = checked.root().lowered.source.module_identity();
    let local = |id| definitions.resolve(id).unwrap().module() == owner;
    let roots = inputs
        .roots
        .iter()
        .filter(|root| local(root.declaration.declaration))
        .collect::<Vec<_>>();
    for kind in [
        RootKind::KeyHash,
        RootKind::KeyEq,
        RootKind::StaticInitializer,
    ] {
        assert!(roots.iter().any(|root| root.kind == kind));
    }
    let unused = inputs
        .sources
        .iter()
        .find(|source| {
            local(source.declaration)
                && definitions
                    .resolve(source.declaration)
                    .unwrap()
                    .last()
                    .unwrap()
                    .name
                    == "unused"
        })
        .unwrap();
    assert_eq!(unused.generic.as_ref().unwrap().parameters.len(), 1);
    assert!(
        unused
            .dependencies
            .iter()
            .any(|dependency| matches!(dependency, Dependency::Source(id)
        if definitions.resolve(*id).unwrap().last().unwrap().name == "C"))
    );
    let initializer = inputs
        .sources
        .iter()
        .find(|source| {
            local(source.declaration)
                && definitions
                    .resolve(source.declaration)
                    .unwrap()
                    .last()
                    .unwrap()
                    .name
                    == "N"
        })
        .unwrap();
    assert!(initializer.tokens.windows(7).any(|tokens| {
        tokens
            .iter()
            .map(|token| token.text.as_str())
            .eq(["(", "(", "C", "+", "1", ")", ")"])
    }));
    for root in roots {
        assert!(
            root.dependencies
                .iter()
                .all(|dependency| matches!(dependency, Dependency::Source(id)
            if inputs.sources.iter().any(|source| source.declaration == *id)))
        );
    }
}

#[test]
fn body_classification_distinguishes_capture_free_named_closure_and_resume_bodies() {
    let checked = analyze_source(
        &SourceFile::new(
            "classification.kgr",
            r#"
fn named(x: i32) -> i32 { x }
fn folded() -> i32 { 1 + 2 }
fn local() -> fn(i32) -> i32 { fn inner(x: i32) -> i32 { x } inner }
fn closure() -> fn(i32) -> i32 { |x: i32| x }
async fn asynchronous(x: i32) -> i32 { x }
fn cold() { val future = async { 1 }; }
static val VALUE: i32 = 7;
"#,
        ),
        catalog::shared().unwrap(),
    )
    .unwrap()
    .into_codegen()
    .unwrap();
    let context = checked.definition_context();
    let module = checked
        .to_unverified(&Default::default())
        .unwrap()
        .into_records();
    let options = MirLoweringOptions {
        optimization: Some(Default::default()),
        ..Default::default()
    };
    let lowered = lower_to_mir_with_requests(
        &module,
        &options,
        &Default::default(),
        std::slice::from_ref(&module),
        &module.aggregates,
        &context,
    )
    .unwrap();
    let definitions = context.snapshot();
    for name in ["named", "inner", "asynchronous"] {
        assert!(lowered.classifications.iter().any(|(_, classification)| matches!(classification,
            BodyClassification::Named(id) if definitions.resolve(*id).unwrap().last().unwrap().name == name)));
    }
    assert!(
        lowered
            .classifications
            .iter()
            .any(|(_, kind)| matches!(kind, BodyClassification::Closure(_)))
    );
    assert!(
        lowered
            .classifications
            .iter()
            .any(|(_, kind)| matches!(kind, BodyClassification::Async(_)))
    );
    assert!(
        lowered
            .classifications
            .iter()
            .any(|(_, kind)| matches!(kind, BodyClassification::Resume(_)))
    );
    assert!(lowered.classifications.iter().any(
        |(_, kind)| matches!(kind, BodyClassification::StaticInitializer(id)
        if definitions.resolve(*id).unwrap().last().unwrap().name == "VALUE")
    ));
    let has_unfolded_sum = |module: &VerifiedMirModule| {
        module
            .functions
            .iter()
            .find(|function| {
                definitions
                    .resolve(function.instance.declaration)
                    .unwrap()
                    .last()
                    .unwrap()
                    .name
                    == "folded"
            })
            .unwrap()
            .blocks
            .iter()
            .flat_map(|block| &block.instructions)
            .any(|instruction| {
                matches!(
                    instruction,
                    Instruction::Binary { .. } | Instruction::Numeric { .. }
                )
            })
    };
    assert!(has_unfolded_sum(&lowered.module));
    let optimized = optimize_module(lowered.module, &options).unwrap();
    assert!(!has_unfolded_sum(&optimized));
}
