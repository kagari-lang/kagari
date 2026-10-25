use super::*;
use crate::{
    bytecode::lower_program_to_bytecode,
    source::lower::{
        ModuleLowering, ModuleLoweringInputs,
        instances::{MirLoweringOptions, bridge_identities::BridgeIdentities},
        optimize_module,
    },
};
use kagari_bytecode::program::verified::VerifiedBytecodeProgram;
use kagari_contract::{
    types::named::correspondence::DeclarationCorrespondence,
    updates::{
        BodyClassification,
        correspondence::import_verified_evidence,
        initializers::compare_initializers,
        limits::{Budget, UpdateError},
    },
};
use kagari_hir::{analysis::AnalysisDatabase, analyze_source};
use kagari_mir::{
    codec::{decode_program, encode_program},
    instruction::Instruction,
    verify::VerifiedMirModule,
};
use kagari_source::{
    source::SourceFile,
    source_database::{SourceDatabase, SourceLayer},
};
use kagari_stdlib::catalog;
use std::slice;

#[test]
fn only_static_expressions_retain_computation_and_all_bodies_keep_authored_owners() {
    let mut sources = SourceDatabase::default();
    let root = sources.set("updates.kgr", r#"
use std::hash::Hash;
struct Key<T> { val value: T }
impl<T: PartialEq> PartialEq for Key<T> { fn eq(self, other: Self) -> bool { self.value == other.value } }
impl<T: Eq> Eq for Key<T> {}
impl<T: Eq + Hash> Hash for Key<T> { fn hash(self) -> i64 { self.value.hash() } }
const C: i32 = 2;
fn unused<T>(x: T) -> i32 { C + 1 }
fn helper() -> i32 { C + 1 }
static val N: i32 = ((helper() + C));
static val CALLBACK: fn() -> i32 = || helper();
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
    let initializers = inputs
        .initializers
        .iter()
        .filter(|initializer| local(initializer.declaration))
        .collect::<Vec<_>>();
    assert_eq!(initializers.len(), 2);
    for source in &inputs.sources {
        if !inputs
            .initializers
            .iter()
            .any(|initializer| initializer.declaration == source.declaration)
        {
            assert!(source.generic.is_none());
            assert!(source.tokens.is_empty());
            assert!(source.inputs.is_empty());
            assert!(source.calls.is_empty());
        }
        assert!(source.bodies.is_empty());
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
    assert!(unused.generic.is_none());
    assert!(unused.inputs.is_empty());
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
    assert!(
        initializer
            .tokens
            .iter()
            .map(|token| token.text.as_str())
            .eq(["(", "(", "helper", "(", ")", "+", "C", ")", ")",])
    );
    assert!(initializer.inputs.iter().any(|input| matches!(input,
        CanonicalOperand::Declaration(id) if definitions.resolve(*id).unwrap().last().unwrap().name == "helper"
    )));
    let published = published.update_evidence();
    for body in &published.bodies {
        let owners = published
            .sources
            .iter()
            .filter(|source| source.bodies.contains(&body.requirement.origin))
            .collect::<Vec<_>>();
        if matches!(body.classification, BodyClassification::IntrinsicAdapter(_)) {
            assert!(owners.is_empty());
        } else {
            assert_eq!(owners.len(), 1);
        }
    }
    let callback = published
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
                    == "CALLBACK"
        })
        .unwrap();
    assert!(
        callback
            .bodies
            .iter()
            .any(|identity| published.bodies.iter().any(|body| {
                body.requirement.origin == *identity
                    && matches!(body.classification, BodyClassification::Closure(_))
            }))
    );
    let body = callback.bodies[0].clone();
    let attached = attach_bodies(
        &published.sources,
        vec![(callback.declaration, body.clone())],
        &Default::default(),
    )
    .unwrap();
    assert_eq!(attached, published.sources);
    assert_eq!(
        attach_bodies(
            &published.sources,
            vec![(unused.declaration, body)],
            &Default::default()
        ),
        Err(DefinitionMappingError::InvalidContract)
    );
}

fn initializer_program(text: &str) -> VerifiedBytecodeProgram {
    let mut sources = SourceDatabase::default();
    let root = sources
        .set("lexical.kgr", text.into(), SourceLayer::Base)
        .unwrap();
    let mut database = AnalysisDatabase::default();
    database.set_native_modules(catalog::shared().unwrap());
    let checked = database
        .snapshot(sources.snapshot(), &Default::default())
        .unwrap()
        .check_program(root, &Default::default())
        .unwrap();
    let mir = crate::source::program::lower_program_to_mir(&checked, &Default::default()).unwrap();
    // Source-free projection carries the preserved MIR and independently checked
    // bytecode recipes, so comparison exercises both executable vocabularies.
    let bytes = encode_program(&mir, &Default::default()).unwrap();
    let decoded = decode_program(&bytes, &Default::default()).unwrap();
    lower_program_to_bytecode(&decoded, &Default::default()).unwrap()
}

fn compare_static_programs(
    previous: &VerifiedBytecodeProgram,
    candidate: &VerifiedBytecodeProgram,
) -> Result<(), UpdateError> {
    let cancel = CancellationToken::default();
    let mut budget = Budget::new(Default::default(), &cancel).unwrap();
    let mut correspondence = DeclarationCorrespondence::new().unwrap();
    let mut import = |program: &VerifiedBytecodeProgram| {
        let inventory = program
            .program()
            .modules
            .iter()
            .flat_map(|module| module.named_declarations.iter().cloned())
            .collect::<Vec<_>>();
        import_verified_evidence(
            &mut correspondence,
            &program.program().update_evidence,
            &inventory,
            program.definitions(),
            &mut budget,
        )
        .unwrap()
    };
    let old = import(previous);
    let new = import(candidate);
    compare_initializers(old.records(), new.records(), &mut budget)
}

#[test]
fn static_closure_identity_ignores_preceding_expressions_and_trivia() {
    let source = r#"
fn helper() -> i32 { 1 }
static val CALLBACK: fn() -> i32 = || helper();
static val NESTED: fn() -> fn() -> i32 = || (|| helper());
static val SIBLINGS: fn() -> i32 = {
    val first = || helper();
    val second = || helper();
    || first() + second()
};
static val ASYNC: fn() -> Future<i32> = async || { (async { helper() }).await };
"#;
    let previous = initializer_program(source);
    for candidate in [
        source.replace("{ 1 }", "{ 1 + 1 }"),
        format!("\n/* preceding trivia */\n{source}"),
        source
            .replace("||", "|| /* π inline trivia */")
            .replace("helper()", "helper ( )"),
        source.replace(
            "fn helper()",
            "fn unrelated() -> i32 { val extra = 1 + 2; extra } fn helper()",
        ),
    ] {
        let candidate = initializer_program(&candidate);
        assert_eq!(compare_static_programs(&previous, &candidate), Ok(()));
    }
    let changed = initializer_program(&source.replace("(|| helper())", "(|| helper() + 0)"));
    assert!(matches!(
        compare_static_programs(&previous, &changed),
        Err(UpdateError::InitializerChange { .. })
    ));
}

#[test]
fn static_callable_adapter_identity_ignores_preceding_trivia() {
    let source = r#"
struct Number { val value: i32 }
impl Fn<(i32,)> for Number {
    type Output = i32;
    fn call(self, args: (i32,)) -> i32 { self.value + args[0] }
}
static val CALLBACK: fn(i32) -> i32 = (Number { value: 2 });
static val FACTORY: fn() -> fn(i32) -> i32 = || (Number { value: 2 });
static val SIBLINGS: fn(i32) -> i32 = {
    val first: fn(i32) -> i32 = (Number { value: 2 });
    val second: fn(i32) -> i32 = Number { value: 3 };
    |value: i32| first(value) + second(value)
};
"#;
    let previous = initializer_program(source);
    for candidate in [
        format!("/* preceding trivia */{source}"),
        source.replace("self.value + args[0]", "self.value + args[0] + 1"),
        source
            .replace("(Number", "( /* π adapter trivia */ Number")
            .replace("value: 3", "value: /* local trivia */ 3"),
    ] {
        let candidate = initializer_program(&candidate);
        assert_eq!(compare_static_programs(&previous, &candidate), Ok(()));
    }
    let changed = initializer_program(&source.replace("value: 3", "value: 4"));
    assert!(matches!(
        compare_static_programs(&previous, &changed),
        Err(UpdateError::InitializerChange { .. })
    ));
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
    let inputs = ModuleLoweringInputs::new(slice::from_ref(&module), &module.aggregates, &context);
    let mut bridges = BridgeIdentities::default();
    let mut state = ModuleLowering::initialize(&module, &inputs, &options, &mut bridges).unwrap();
    state
        .process(&Default::default(), &inputs, &options, &mut bridges)
        .unwrap();
    let lowered = state.snapshot(&inputs, &options).unwrap();
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
