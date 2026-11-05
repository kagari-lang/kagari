use super::*;
use crate::{
    compile_input::body::BodyRoot, hir::ids::HirOwner, tests::test_analysis, types::TypeId,
};
use kagari_source::source_database::{SourceDatabase, SourceLayer};
use kagari_types::scalar::BuiltinType;

#[test]
fn admitted_bodies_keep_owner_arena_and_completion_evidence_together() {
    let text = "const N: i32 = 2; static val S: i32 = N; trait Read { fn read(self) -> i32; } struct Box<T> { val value: T } type Alias<T> = Box<T>; impl<T> Box<T> where T: Read {} fn first() -> i32 { 3 } fn second() -> i32 { S }";
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
    let headers = module.headers();
    let constant = module
        .environment()
        .local_constant(headers.constants[0].id)
        .unwrap();
    assert_eq!(constant.ty, &TypeId::Builtin(BuiltinType::I32));
    assert!(module.registration().is_none());
    let header = &headers.implementations[0];
    assert!(header.methods.is_empty());
    let implementation = module.implementation(header.id).unwrap();
    assert!(implementation.interface.is_none());
    assert_eq!(implementation.generic_params.len(), 1);
    let parameter = TypeId::Generic(implementation.generic_params[0].clone());
    assert_eq!(implementation.bounds[&parameter].len(), 1);
    let TypeId::Struct(receiver) = implementation.receiver else {
        panic!("inherent receiver");
    };
    assert_eq!(receiver.arguments, [parameter]);
    let alias = &headers.aliases[0];
    let declaration = module
        .environment()
        .definition(ResolvedName::TypeAlias(alias.id))
        .unwrap();
    let template = module
        .environment()
        .declarations()
        .imported_types()
        .alias(&declaration)
        .unwrap();
    assert!(matches!(template.target, TypeId::Struct(_)));
    let span = module.declaration_span(Item::TypeAlias(alias.id)).unwrap();
    assert_eq!(
        &module.source().text()[span.start..span.end],
        "type Alias<T> = Box<T>;"
    );
    assert!(
        module
            .declaration_span(Item::Function(FunctionId::new(u32::MAX as usize)))
            .is_none()
    );
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
    let native = compilation
        .modules()
        .iter()
        .find_map(|module| {
            module.headers().functions.iter().find_map(|function| {
                module
                    .native_default(function.id)
                    .map(|call| (module, function.id, call))
            })
        })
        .expect("foundation supplies checked default recipes");
    assert!(native.0.registration().is_some());
    assert!(
        native
            .0
            .environment()
            .definitions()
            .resolve(native.2.declaration)
            .is_ok()
    );
    assert!(!native.2.signature.return_type.is_unresolved());
    let source = program
        .modules()
        .iter()
        .find(|module| {
            module.lowered.source.module_identity() == native.0.source().module_identity()
        })
        .unwrap();
    let mut missing = source
        .to_unverified(&Default::default())
        .unwrap()
        .into_records();
    missing
        .typed
        .type_table
        .native_default_calls
        .remove(&native.1);
    let missing =
        CheckedAnalysis::adopt_scoped(&missing, source.definitions(), &Default::default()).unwrap();
    assert!(matches!(
        CheckedModule::admit(&missing, &Default::default()),
        Err(AdmissionError::InvalidModule(
            "native default has no checked forwarding call"
        ))
    ));
    let cancelled = CancellationToken::default();
    cancelled.cancel();
    assert!(matches!(
        CheckedCompilation::from_program(&program, &cancelled),
        Err(AdmissionError::Cancelled)
    ));
}
