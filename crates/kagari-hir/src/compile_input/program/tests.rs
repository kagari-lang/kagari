use super::*;
use crate::{
    compile_input::body::{BodyRoot, bindings::BindingId},
    hir::ids::{HirOwner, ParamId},
    tests::test_analysis,
    types::TypeId,
};
use kagari_source::source_database::{SourceDatabase, SourceLayer};
use kagari_types::scalar::BuiltinType;
use std::sync::Arc;

#[test]
fn admitted_bodies_keep_owner_arena_and_completion_evidence_together() {
    let text = include_str!("../../../tests/fixtures/analysis/checked_parameters.kgr");
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
    let initializer = module
        .static_initializer_span(headers.statics[0].id)
        .unwrap();
    assert_eq!(
        &module.source().text()[initializer.start..initializer.end],
        "N"
    );
    let evidence = module.provenance();
    assert_eq!(evidence.initializers.len(), 1);
    let initializer = evidence
        .bodies
        .iter()
        .find(|body| body.initializer.is_some())
        .unwrap();
    assert_eq!(initializer.tokens.len(), 1);
    assert_eq!(initializer.tokens[0].text, "N");
    assert!(
        evidence
            .bodies
            .iter()
            .filter(|body| body.initializer.is_none())
            .all(|body| body.tokens.is_empty() && body.facts.is_empty())
    );

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
    assert_eq!(
        module
            .body_for(BodyOwner::Function(first_function.id))
            .unwrap()
            .key(),
        first.key()
    );
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
    assert_eq!(
        first
            .completion()
            .match_arm_can_continue(value, 0, &Default::default()),
        Ok(None),
        "a literal is not a match expression"
    );
    let second = bodies
        .iter()
        .find(|body| {
            body.enclosing_signature()
                .is_some_and(|signature| signature.name == "second")
        })
        .copied()
        .unwrap();
    let parameter = first_function.params[0].id;
    let other_parameter = second.enclosing_signature().unwrap().params[0].id;
    assert_eq!((parameter.index(), other_parameter.index()), (0, 0));
    assert_eq!(
        first.parameter_type(parameter),
        Some(&TypeId::Builtin(BuiltinType::I32))
    );
    assert_eq!(
        second.parameter_type(other_parameter),
        Some(&TypeId::Builtin(BuiltinType::Bool))
    );
    let binding = first.binding(BindingId::Parameter(parameter)).unwrap();
    assert_eq!(binding.span.start, text.find("value: i32").unwrap());
    assert_eq!(&text[binding.span.start..binding.span.end], "value");
    for invalid in [
        other_parameter,
        ParamId::new(parameter.arena(), parameter.owner(), 1),
        ParamId::new(parameter.arena(), HirOwner::Declaration, 0),
    ] {
        assert!(first.parameter_type(invalid).is_none());
        assert!(first.binding(BindingId::Parameter(invalid)).is_none());
    }
    assert!(second.parameter_type(parameter).is_none());
    let BodyRoot::Block(second_root) = second.root() else {
        panic!("function block")
    };
    let matching = second.block(second_root).unwrap().tail_expr.unwrap();
    let mut completion = second.completion();
    assert_eq!(
        completion.match_arm_can_continue(matching, 0, &Default::default()),
        Ok(Some(false))
    );
    assert_eq!(
        completion.match_arm_can_continue(matching, 2, &Default::default()),
        Ok(None),
        "the match has only two source arms"
    );
    assert_eq!(
        first
            .completion()
            .match_arm_can_continue(matching, 0, &Default::default()),
        Ok(None),
        "another body's match is not owned by this query"
    );
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
            text.replace("first(value: i32) -> i32", "first(value: i64) -> i64"),
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
    assert!(newer_first.parameter_type(parameter).is_none());
    assert!(
        newer_first
            .binding(BindingId::Parameter(parameter))
            .is_none()
    );
    let newer_parameter = newer_first.enclosing_signature().unwrap().params[0].id;
    assert_eq!(
        newer_first.parameter_type(newer_parameter),
        Some(&TypeId::Builtin(BuiltinType::I64))
    );
    assert_eq!(
        first.parameter_type(parameter),
        Some(&TypeId::Builtin(BuiltinType::I32))
    );
    for body in newer_module.bodies() {
        assert!(body.expression(value).is_none());
        assert_eq!(
            body.completion()
                .match_arm_can_continue(matching, 0, &Default::default()),
            Ok(None),
            "the match belongs to the older arena"
        );
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
        CheckedModule::admit(
            &incomplete,
            &Default::default(),
            &mut SourceBudget::new(&Default::default())
        ),
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
    let mut inconsistent = source
        .to_unverified(&Default::default())
        .unwrap()
        .into_records();
    Arc::make_mut(&mut inconsistent.lowered).module.functions[native.1.index()].implementation =
        HirImplementation::Required;
    let inconsistent =
        CheckedAnalysis::adopt_scoped(&inconsistent, source.definitions(), &Default::default())
            .unwrap();
    assert!(matches!(
        CheckedModule::admit(
            &inconsistent,
            &Default::default(),
            &mut SourceBudget::new(&Default::default())
        ),
        Err(AdmissionError::InvalidModule(
            "function implementation and checked binding disagree"
        ))
    ));
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
        CheckedModule::admit(
            &missing,
            &Default::default(),
            &mut SourceBudget::new(&Default::default())
        ),
        Err(AdmissionError::InvalidModule(
            "native default has no checked forwarding call"
        ))
    ));
    let cancelled = CancellationToken::default();
    cancelled.cancel();
    assert!(
        completion
            .match_arm_can_continue(matching, 0, &cancelled)
            .is_err()
    );
    assert!(
        completion
            .match_arm_can_continue(matching, 2, &cancelled)
            .is_err()
    );
    assert!(matches!(
        CheckedCompilation::from_program(&program, &cancelled),
        Err(AdmissionError::Cancelled)
    ));
}
