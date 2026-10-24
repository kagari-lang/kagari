use super::*;
use crate::{
    artifact::KbcArtifact,
    instruction::{BytecodeInstruction, ConstantId, ConstantOperand, Slot},
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
    types::named::{NamedContract, NamedDeclaration},
    updates::demands::{Coverage, check_coverage, publication_demands},
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
                dependencies: vec![],
            }],
            roots: vec![],
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
    detached.update_evidence.sources[0].dependencies.clear();
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
    program.update_evidence = build(
        &program.modules,
        EvidenceInputs {
            sources: program.update_evidence.sources.clone(),
            roots: vec![],
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
