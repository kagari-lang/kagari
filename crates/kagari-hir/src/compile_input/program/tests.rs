use super::*;
use crate::{
    compile_input::body::BodyRoot, hir::ids::HirOwner, tests::test_analysis, types::TypeId,
};
use kagari_source::source_database::{SourceDatabase, SourceLayer};
use kagari_types::scalar::BuiltinType;

#[test]
fn admitted_bodies_keep_owner_arena_and_completion_evidence_together() {
    let text = "const N: i32 = 2; static val S: i32 = N; trait Read { fn read(self) -> i32; } fn first() -> i32 { 3 } fn second() -> i32 { S }";
    let mut sources = SourceDatabase::default();
    let file = sources
        .set("input.kgr", text.into(), SourceLayer::Base)
        .unwrap();
    let mut database = test_analysis();
    let snapshot = database
        .snapshot(sources.snapshot(), &Default::default())
        .unwrap();
    let program = snapshot.check_program(file, &Default::default()).unwrap();
    let compilation = CheckedCompilation::from_program(&program, &Default::default()).unwrap();
    let module = compilation
        .modules()
        .iter()
        .find(|module| module.handle() == compilation.root())
        .unwrap();
    let bodies = module.bodies().collect::<Vec<_>>();
    assert_eq!(bodies.len(), 4, "two functions and two initializer owners");
    let first_function = module
        .environment()
        .signatures()
        .functions()
        .iter()
        .find(|function| function.name == "first")
        .unwrap();
    let first = bodies
        .iter()
        .find(|body| body.key().owner == BodyOwner::Function(first_function.id))
        .copied()
        .unwrap();
    let BodyRoot::Block(root) = first.root() else {
        panic!("function block")
    };
    let value = first.block(root).unwrap().tail_expr.unwrap();
    assert_eq!(
        first.expression_facts(value).unwrap().ty,
        Some(TypeId::Builtin(BuiltinType::I32))
    );
    let span = first.expression_span(value).unwrap();
    assert_eq!(&module.source().text()[span.start..span.end], "3");
    for other in &bodies {
        if other.key() != first.key() {
            assert!(other.expression(value).is_none());
            assert!(other.expression_facts(value).is_none());
        }
    }
    let requirement = module
        .environment()
        .signatures()
        .functions()
        .iter()
        .find(|function| function.name == "read")
        .unwrap();
    assert!(matches!(
        requirement.implementation,
        FunctionImplementation::Required
    ));
    assert!(
        bodies
            .iter()
            .all(|body| body.key().owner != BodyOwner::Function(requirement.id))
    );

    sources
        .set(
            "input.kgr",
            text.replace("first() -> i32 { 3 }", "first() -> i64 { 4 }"),
            SourceLayer::Overlay,
        )
        .unwrap();
    let newer = database
        .snapshot(sources.snapshot(), &Default::default())
        .unwrap();
    let newer_program = newer.check_program(file, &Default::default()).unwrap();
    let newer_compilation =
        CheckedCompilation::from_program(&newer_program, &Default::default()).unwrap();
    let newer_module = newer_compilation
        .modules()
        .iter()
        .find(|module| module.handle() == newer_compilation.root())
        .unwrap();
    assert_eq!(
        first
            .environment()
            .signatures()
            .function(first_function.id)
            .unwrap()
            .return_type,
        TypeId::Builtin(BuiltinType::I32),
    );
    let newer_first = newer_module
        .bodies()
        .find(|body| body.key().owner == first.key().owner)
        .unwrap();
    assert_eq!(
        newer_first
            .environment()
            .signatures()
            .function(first_function.id)
            .unwrap()
            .return_type,
        TypeId::Builtin(BuiltinType::I64),
    );
    assert!(newer_module.body(first.key()).is_none());
    for body in newer_module.bodies() {
        assert!(body.expression(value).is_none());
    }
    assert_eq!(value.owner(), HirOwner::Body(first.key().owner));
    assert_eq!(
        first.expression_facts(value).unwrap().ty,
        Some(TypeId::Builtin(BuiltinType::I32))
    );

    // Simulate an internal producer dropping completion evidence after checking.
    // Admission must reject even though the former checked envelope had no diagnostics.
    let mut incomplete = program
        .root()
        .to_unverified(&Default::default())
        .unwrap()
        .into_records();
    incomplete.typed.valid_bodies.remove(&first_function.id);
    let incomplete = CheckedAnalysis::adopt_scoped(
        &incomplete,
        program.root().definitions(),
        &Default::default(),
    )
    .unwrap();
    assert!(matches!(
        CheckedModule::admit(&incomplete, &Default::default()),
        Err(AdmissionError::InvalidBody { .. })
    ));
    let cancelled = CancellationToken::default();
    cancelled.cancel();
    assert!(matches!(
        CheckedCompilation::from_program(&program, &cancelled),
        Err(AdmissionError::Cancelled)
    ));
}
