use super::*;
use crate::{
    artifact::KbcArtifact,
    instruction::{BytecodeInstruction, CallTarget, ConstantId, ConstantOperand, Slot},
    module::{
        BytecodeFunction, BytecodeSemanticSlots, FunctionMetadata, FunctionRecord,
        InstructionOrigin,
    },
    program::{BytecodeProgram, ModuleRef, verified::VerifiedBytecodeProgram, verify_program},
    trait_bounds::preparation::VerificationResources,
};
use kagari_abi::representation::ValueType;
use kagari_common::identity::{DefinitionKind, ModuleIdentity, map::DefinitionContext};
use kagari_contract::{
    ids::FunctionRef,
    statics::{StaticContract, StaticDescriptor},
    types::named::{NamedContract, NamedDeclaration},
    updates::{
        demands::{Coverage, check_coverage, publication_demands},
        initializers::compare_initializers,
    },
};
use kagari_types::{
    callable::CallableImplementation, declaration::FnDecl, scalar::BuiltinType, ty::Ty,
    visibility::Visibility,
};

fn fixture() -> (DefinitionContext, BytecodeProgram) {
    let context = DefinitionContext::new().unwrap();
    let identity = ModuleIdentity::single_file("evidence");
    let root = context.intern_root(&identity).unwrap();
    let declaration = context
        .intern_child(root, DefinitionKind::Function, "answer", 0)
        .unwrap();
    let instance = ConcreteFunctionIdentity {
        declaration,
        arguments: vec![],
    };
    let result = Ty::Builtin(BuiltinType::I32);
    let function = BytecodeFunction {
        id: FunctionRef::new(0),
        identity: Some(instance.clone()),
        name: "answer".into(),
        parameter_count: 0,
        slot_count: 1,
        metadata: FunctionMetadata {
            semantic: BytecodeSemanticSlots {
                result: Some(result.clone()),
                slots: [(0, result.clone())].into(),
                ..Default::default()
            },
            return_type: ValueType::I32,
            slots: vec![ValueType::I32],
            origins: (0..2)
                .map(|instruction| InstructionOrigin {
                    instruction,
                    span: Default::default(),
                    line: None,
                    column: None,
                })
                .collect(),
            ..Default::default()
        },
        instructions: vec![
            BytecodeInstruction::LoadConst {
                dst: Slot::new(0),
                constant: ConstantId::new(0),
            },
            BytecodeInstruction::Return(Some(Slot::new(0))),
        ],
    };
    let record = FunctionRecord {
        id: function.id,
        identity: function.identity.clone(),
        name: function.name.clone(),
        params: vec![],
        return_type: ValueType::I32,
        effects: Default::default(),
    };
    let module = BytecodeModule {
        identity,
        constants: vec![ConstantOperand::I32(42)],
        types: vec![ValueType::I32],
        functions: vec![function],
        function_table: vec![record],
        named_declarations: vec![NamedDeclaration {
            declaration,
            visibility: Visibility::Private,
            ambiguous_local_body: None,
            contract: NamedContract::Function(FnDecl {
                name: "answer".into(),
                host_policy: None,
                method_policy: Default::default(),
                implementation: CallableImplementation::Script,
                generic_params: vec![],
                bounds: vec![],
                params: vec![],
                return_type: result,
            }),
        }],
        ..Default::default()
    };
    let modules = vec![module];
    let update_evidence = build(
        &modules,
        EvidenceInputs {
            sources: vec![SourceRecord {
                declaration,
                generic: None,
                tokens: vec![],
                inputs: vec![],
                calls: vec![],
                bodies: vec![],
            }],
            initializers: vec![],
            selections: vec![],
            classifications: [(instance, BodyClassification::Named(declaration))].into(),
        },
        &context.snapshot(),
        &Default::default(),
    )
    .unwrap();
    (
        context,
        BytecodeProgram {
            root: ModuleRef::new(0),
            modules,
            exports: Default::default(),
            update_evidence,
        },
    )
}

#[test]
fn update_evidence_bytecode_requires_complete_actual_body_and_portable_certificate() {
    let (context, program) = fixture();
    let verified =
        VerifiedBytecodeProgram::new(program.clone(), &context, &Default::default()).unwrap();
    assert_eq!(verified.update_coverage().evidence.bodies.len(), 1);
    let artifact = KbcArtifact::from_program(
        program.clone(),
        Default::default(),
        &context,
        &Default::default(),
    )
    .unwrap();
    let bytes = artifact.to_bytes(&context.snapshot()).unwrap();
    let imported = DefinitionContext::new().unwrap();
    let decoded = KbcArtifact::from_bytes(&bytes)
        .unwrap()
        .import_into(&imported, &Default::default())
        .unwrap()
        .into_records();
    decoded
        .validate_for_loader(&Default::default(), &imported, &Default::default())
        .unwrap();
    assert_eq!(decoded.to_bytes(&imported.snapshot()).unwrap(), bytes);
    let mut missing = program.clone();
    missing.update_evidence = empty();
    assert!(verify_program(&missing, &context.snapshot(), &Default::default()).is_err());
    let mut detached = program.clone();
    detached.update_evidence.sources[0].bodies.clear();
    assert!(verify_program(&detached, &context.snapshot(), &Default::default()).is_err());
    let mut missing_source = program.clone();
    missing_source.update_evidence.sources.clear();
    assert!(verify_program(&missing_source, &context.snapshot(), &Default::default()).is_err());
    let mut wrong_target = program.clone();
    wrong_target.update_evidence.targets[0].function = FunctionRef::new(1);
    assert!(verify_program(&wrong_target, &context.snapshot(), &Default::default()).is_err());
    let mut changed = program.clone();
    changed.modules[0].constants[0] = ConstantOperand::I32(43);
    assert!(verify_program(&changed, &context.snapshot(), &Default::default()).is_err());
    changed.update_evidence = project(
        &changed.modules,
        &program.update_evidence,
        &context.snapshot(),
        &Default::default(),
    )
    .unwrap();
    verify_program(&changed, &context.snapshot(), &Default::default()).unwrap();
    assert_eq!(
        changed.update_evidence.bodies[0].instructions,
        program.update_evidence.bodies[0].instructions
    );
    assert_ne!(
        changed.update_evidence.bodies[0].actual_instructions,
        program.update_evidence.bodies[0].actual_instructions
    );
    let mut classified = program.clone();
    classified.update_evidence.bodies[0].classification = BodyClassification::Closure(
        classified.update_evidence.bodies[0]
            .requirement
            .origin
            .clone(),
    );
    assert!(verify_program(&classified, &context.snapshot(), &Default::default()).is_err());
    let mut moved = program;
    moved.modules[0]
        .constants
        .insert(0, ConstantOperand::I32(99));
    let BytecodeInstruction::LoadConst { constant, .. } =
        &mut moved.modules[0].functions[0].instructions[0]
    else {
        unreachable!()
    };
    *constant = ConstantId::new(1);
    verify_program(&moved, &context.snapshot(), &Default::default()).unwrap();
}

fn static_fixture() -> (DefinitionContext, BytecodeProgram) {
    let (context, mut program) = fixture();
    let module = &mut program.modules[0];
    let root = context.intern_root(&module.identity).unwrap();
    let declaration = context
        .intern_child(root, DefinitionKind::Static, "STATE", 0)
        .unwrap();
    let mut initializer = module.functions[0].clone();
    initializer.id = FunctionRef::new(1);
    initializer.name = "initialize_state".into();
    initializer.identity = Some(ConcreteFunctionIdentity {
        declaration: context
            .intern_child(root, DefinitionKind::Function, "initialize_state", 0)
            .unwrap(),
        arguments: vec![],
    });
    initializer.instructions[0] = BytecodeInstruction::Call {
        dst: Some(Slot::new(0)),
        callee: CallTarget::Function(FunctionRef::new(0)),
        args: vec![],
    };
    let mut record = module.function_table[0].clone();
    record.id = initializer.id;
    record.identity = initializer.identity.clone();
    record.name = initializer.name.clone();
    module.function_table.push(record);
    module.module_slots.push(StaticDescriptor {
        contract: StaticContract {
            declaration,
            supplier: module.identity.clone(),
            name: "STATE".into(),
            ty: Ty::Builtin(BuiltinType::I32),
            mutable: true,
            visibility: Visibility::Private,
        },
        initializer: initializer.id,
    });
    module.functions.push(initializer);
    let program = fixtures::certify(program, &context);
    (context, program)
}

#[test]
fn static_inventory_and_local_recipe_survive_source_free_admission() {
    let (context, program) = static_fixture();
    let declaration = program.modules[0].module_slots[0].contract.declaration;
    verify_program(&program, &context.snapshot(), &Default::default()).unwrap();
    assert_eq!(
        program.update_evidence.initializers,
        vec![StaticInitializer { declaration }]
    );
    assert!(matches!(
        program.update_evidence.bodies[1]
            .requirement
            .calls
            .as_slice(),
        [CallRequirement::Direct(_)]
    ));
    let mut missing = program.clone();
    missing.update_evidence.initializers.clear();
    assert!(verify_program(&missing, &context.snapshot(), &Default::default()).is_err());
    let mut duplicate = program.clone();
    duplicate
        .update_evidence
        .initializers
        .push(StaticInitializer { declaration });
    assert!(verify_program(&duplicate, &context.snapshot(), &Default::default()).is_err());
    let mut detached = program.clone();
    detached
        .update_evidence
        .sources
        .iter_mut()
        .find(|source| source.declaration == declaration)
        .unwrap()
        .bodies
        .clear();
    assert!(verify_program(&detached, &context.snapshot(), &Default::default()).is_err());

    let mut candidate = program.clone();
    candidate.modules[0].constants[0] = ConstantOperand::I32(43);
    let candidate = fixtures::certify(candidate, &context);
    verify_program(&candidate, &context.snapshot(), &Default::default()).unwrap();
    let cancel = CancellationToken::default();
    let mut budget = Budget::new(Limits::default(), &cancel).unwrap();
    compare_initializers(
        &program.update_evidence,
        &candidate.update_evidence,
        &mut budget,
    )
    .unwrap();

    let mut changed = candidate;
    changed.modules[0].functions[1].instructions[0] = BytecodeInstruction::LoadConst {
        dst: Slot::new(0),
        constant: ConstantId::new(0),
    };
    let changed = fixtures::certify(changed, &context);
    verify_program(&changed, &context.snapshot(), &Default::default()).unwrap();
    assert_eq!(
        compare_initializers(
            &program.update_evidence,
            &changed.update_evidence,
            &mut budget
        ),
        Err(UpdateError::InitializerChange { declaration })
    );
}

fn static_child_without_creation() -> (DefinitionContext, BytecodeProgram) {
    let (context, mut program) = static_fixture();
    let module = &mut program.modules[0];
    let parent = module.functions[1].identity.clone().unwrap();
    let child = ConcreteFunctionIdentity {
        declaration: context
            .intern_child(parent.declaration, DefinitionKind::Function, "closure_0", 0)
            .unwrap(),
        arguments: parent.arguments.clone(),
    };
    let mut function = module.functions[0].clone();
    function.id = FunctionRef::new(2);
    function.identity = Some(child.clone());
    function.name = "closure_0".into();
    let mut record = module.function_table[0].clone();
    record.id = function.id;
    record.identity = function.identity.clone();
    record.name = function.name.clone();
    module.functions[1].instructions[0] = BytecodeInstruction::Call {
        dst: Some(Slot::new(0)),
        callee: CallTarget::Function(function.id),
        args: vec![],
    };
    module.functions.push(function);
    module.function_table.push(record);
    let mut sources = program.update_evidence.sources.clone();
    sources
        .iter_mut()
        .find(|source| source.declaration == module.module_slots[0].contract.declaration)
        .unwrap()
        .bodies
        .push(child.clone());
    let mut classifications = program
        .update_evidence
        .bodies
        .iter()
        .map(|body| (body.requirement.origin.clone(), body.classification.clone()))
        .collect::<HashMap<_, _>>();
    classifications.insert(child, BodyClassification::Closure(parent));
    program.update_evidence = build(
        &program.modules,
        EvidenceInputs {
            sources,
            initializers: program.update_evidence.initializers.clone(),
            selections: vec![],
            classifications,
        },
        &context.snapshot(),
        &Default::default(),
    )
    .unwrap();
    (context, program)
}

#[test]
fn static_child_cannot_forge_lexical_owner_without_a_creation_instruction() {
    let (context, program) = static_child_without_creation();
    verify_program(&program, &context.snapshot(), &Default::default()).unwrap();
    let child = program.modules[0].functions[2].identity.clone().unwrap();
    assert!(matches!(
        program.update_evidence.bodies[1].requirement.calls.as_slice(),
        [CallRequirement::RetainedBody(application)] if application.body == child
    ));
    let unrelated = program.modules[0].functions[0].identity.clone().unwrap();
    for classification in [
        BodyClassification::Closure(unrelated.clone()),
        BodyClassification::Async(unrelated.clone()),
        BodyClassification::Resume(unrelated.clone()),
        BodyClassification::Adapter(unrelated.clone()),
    ] {
        let mut forged = program.clone();
        forged.update_evidence.bodies[2].classification = classification;
        for source in &mut forged.update_evidence.sources {
            source.bodies.retain(|body| body != &child);
            if source.declaration == unrelated.declaration {
                source.bodies.push(child.clone());
            }
        }
        assert!(verify_program(&forged, &context.snapshot(), &Default::default()).is_err());
        assert_eq!(
            project(
                &forged.modules,
                &forged.update_evidence,
                &context.snapshot(),
                &Default::default()
            ),
            Err(UpdateError::InvalidEvidence)
        );
    }
    let mut changed = program.clone();
    changed.modules[0].constants.push(ConstantOperand::I32(43));
    changed.modules[0].functions[2].instructions[0] = BytecodeInstruction::LoadConst {
        dst: Slot::new(0),
        constant: ConstantId::new(1),
    };
    changed.update_evidence = project(
        &changed.modules,
        &program.update_evidence,
        &context.snapshot(),
        &Default::default(),
    )
    .unwrap();
    verify_program(&changed, &context.snapshot(), &Default::default()).unwrap();
    let cancel = CancellationToken::default();
    let mut budget = Budget::new(Limits::default(), &cancel).unwrap();
    assert_eq!(
        compare_initializers(
            &program.update_evidence,
            &changed.update_evidence,
            &mut budget
        ),
        Err(UpdateError::InitializerChange {
            declaration: program.modules[0].module_slots[0].contract.declaration,
        })
    );
}

#[test]
fn update_evidence_retained_edge_needs_a_current_target_not_an_old_body() {
    let (context, mut program) = fixture();
    let target = program.update_evidence.targets[0].application.clone();
    let mut historical = program.update_evidence.clone();
    historical.bodies[0]
        .requirement
        .calls
        .push(CallRequirement::Direct(target.clone()));
    let cancel = CancellationToken::default();
    let mut budget = Budget::new(Limits::default(), &cancel).unwrap();
    let envelope = publication_demands(&historical, &mut budget).unwrap();
    // Retained historical bodies do not provide an ordinary current-call route.
    program.update_evidence = historical;
    program.update_evidence.targets.clear();
    let definitions = context.snapshot();
    let mut resources =
        VerificationResources::new(program.modules.iter().collect(), &definitions, &cancel)
            .unwrap();
    let scope = resources
        .scope(&(0..program.modules.len()).collect::<Vec<_>>(), &cancel)
        .unwrap();
    let coverage = Coverage {
        evidence: &program.update_evidence,
        methods: &[],
        constructions: &[],
        layouts: &[],
        natives: &[],
    };
    assert_eq!(
        check_coverage(&envelope, &coverage, &scope.catalog, &mut budget),
        Err(UpdateError::MissingTarget(target))
    );
}

#[test]
fn update_evidence_capture_free_closure_remains_body_owned_and_cannot_drop_its_edge() {
    let (context, mut program) = fixture();
    let parent = program.modules[0].functions[0].identity.clone().unwrap();
    let child = ConcreteFunctionIdentity {
        declaration: context
            .intern_child(parent.declaration, DefinitionKind::Function, "closure", 0)
            .unwrap(),
        arguments: vec![],
    };
    let mut body = program.modules[0].functions[0].clone();
    body.id = FunctionRef::new(1);
    body.identity = Some(child.clone());
    body.name = "closure".into();
    let mut record = program.modules[0].function_table[0].clone();
    record.id = body.id;
    record.identity = body.identity.clone();
    record.name = body.name.clone();
    program.modules[0].functions.push(body);
    program.modules[0].function_table.push(record);
    let ty = Ty::Function {
        params: vec![],
        result: Box::new(Ty::Builtin(BuiltinType::I32)),
    };
    let owner = &mut program.modules[0];
    let function = &mut owner.functions[0];
    function.instructions[0] = BytecodeInstruction::MakeClosure {
        dst: Slot::new(0),
        function: FunctionRef::new(1),
        captures: vec![],
    };
    function.metadata.return_type = ValueType::HeapObject;
    function.metadata.slots[0] = ValueType::HeapObject;
    function.metadata.semantic.result = Some(ty.clone());
    function.metadata.semantic.slots.insert(0, ty.clone());
    function.metadata.roots = crate::module::RootSlotLayout::from_types(&function.metadata.slots);
    function.metadata.effects = kagari_contract::effects::EffectSet::allocation();
    owner.function_table[0].return_type = ValueType::HeapObject;
    owner.function_table[0].effects = function.metadata.effects;
    owner.types.push(ValueType::HeapObject);
    let NamedContract::Function(declaration) = &mut owner.named_declarations[0].contract else {
        unreachable!()
    };
    declaration.return_type = ty;
    let mut sources = program.update_evidence.sources.clone();
    sources[0].bodies.push(child.clone());
    program.update_evidence = build(
        &program.modules,
        EvidenceInputs {
            sources,
            initializers: vec![],
            selections: vec![],
            classifications: [
                (
                    parent.clone(),
                    BodyClassification::Named(parent.declaration),
                ),
                (child.clone(), BodyClassification::Closure(parent)),
            ]
            .into(),
        },
        &context.snapshot(),
        &Default::default(),
    )
    .unwrap();
    verify_program(&program, &context.snapshot(), &Default::default()).unwrap();
    assert!(
        matches!(&program.update_evidence.bodies[0].requirement.calls[0], CallRequirement::RetainedBody(application) if application.body == child)
    );
    assert_eq!(program.update_evidence.targets.len(), 1);
    let mut omitted = program.clone();
    omitted.update_evidence.bodies[0].requirement.calls.clear();
    assert!(verify_program(&omitted, &context.snapshot(), &Default::default()).is_err());
    let mut live = program;
    live.update_evidence.bodies[1].classification = BodyClassification::Named(child.declaration);
    assert!(verify_program(&live, &context.snapshot(), &Default::default()).is_err());
}

#[test]
fn update_evidence_bytecode_cancellation_retains_its_error_kind() {
    let (context, program) = fixture();
    let cancel = CancellationToken::default();
    cancel.cancel();
    assert_eq!(
        project(
            &program.modules,
            &program.update_evidence,
            &context.snapshot(),
            &cancel
        ),
        Err(UpdateError::Cancelled)
    );
}
