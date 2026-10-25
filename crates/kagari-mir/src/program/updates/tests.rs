use super::*;
use crate::{
    codec::{decode_program, encode_program},
    debug::{MirFunctionDebugMetadata, MirLexicalScope},
    function::{BasicBlock, MirFunction, MirTemp},
    ids::{BlockId, InstanceId, TempId},
    instruction::{Constant, ConstructionTarget, Instruction, MirValue, Terminator},
    program::{records::MirProgram, verify_program},
};
use kagari_abi::representation::ValueType;
use kagari_common::identity::{DefinitionKind, ModuleIdentity, map::DefinitionContext};
use kagari_contract::{
    callable::{
        construction::evidence::{
            ConstructionRequirement, ConstructionSelection, ConstructionWitness,
        },
        generic::GenericBody,
        witness::OperationWitness,
    },
    types::{
        InterfaceBridge, InterfaceTable, ModuleContract, PublicItem,
        named::{ImplementationContract, NamedContract, NamedDeclaration},
        proofs::{ProofCatalog, implementation::Implementation},
    },
};
use kagari_types::{
    callable::CallableImplementation,
    declaration::{FnDecl, Param, TraitDef, requirement::NativeCallableRequirement},
    scalar::BuiltinType,
    ty::{GenericBound, GenericParam, NominalTy, Ty},
    visibility::Visibility,
};

fn fixture() -> (DefinitionContext, MirProgram) {
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
    let function = MirFunction {
        semantic: kagari_contract::slots::SemanticSlots {
            result: Some(result.clone()),
            registers: [(0, result.clone())].into(),
            ..Default::default()
        },
        id: InstanceId::new(0),
        instance: instance.clone(),
        name: "answer".into(),
        params: vec![],
        return_type: ValueType::I32,
        locals: vec![],
        temps: vec![MirTemp { ty: ValueType::I32 }],
        blocks: vec![BasicBlock {
            instructions: vec![Instruction::LoadConst {
                dst: MirValue {
                    temp: TempId::new(0),
                    ty: ValueType::I32,
                },
                constant: Constant::I32(42),
            }],
            instruction_spans: vec![Default::default()],
            instruction_scopes: vec![0],
            terminator: Some(Terminator::Return(Some(MirValue {
                temp: TempId::new(0),
                ty: ValueType::I32,
            }))),
            terminator_span: Some(Default::default()),
            terminator_scope: Some(0),
        }],
        entry: BlockId::new(0),
        effects: Default::default(),
        debug: MirFunctionDebugMetadata {
            lexical_scopes: vec![MirLexicalScope {
                parent: None,
                local: None,
            }],
            ..Default::default()
        },
    };
    let module = MirModule {
        identity: identity.clone(),
        source_name: String::new(),
        native_targets: vec![],
        interface_instances: vec![],
        external_types: vec![],
        dependencies: vec![],
        structures: vec![],
        enumerations: vec![],
        module_slots: vec![],
        functions: vec![function],
        abi: ModuleContract {
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
        },
    };
    let modules = vec![module];
    let evidence = build(
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
        MirProgram {
            root: identity,
            modules,
            exports: Default::default(),
            update_evidence: evidence,
        },
    )
}

#[test]
fn update_evidence_mir_is_mandatory_and_checks_the_complete_body() {
    let (context, program) = fixture();
    let verified = verify_program(program.clone(), &context, &Default::default()).unwrap();
    let bytes = encode_program(&verified, &Default::default()).unwrap();
    let decoded = decode_program(&bytes, &Default::default()).unwrap();
    assert_eq!(
        encode_program(&decoded, &Default::default()).unwrap(),
        bytes
    );
    let mut missing = program.clone();
    missing.update_evidence = empty();
    assert!(verify_program(missing, &context, &Default::default()).is_err());
    let mut changed = program.clone();
    let Instruction::LoadConst { constant, .. } =
        &mut changed.modules[0].functions[0].blocks[0].instructions[0]
    else {
        unreachable!()
    };
    *constant = Constant::I32(43);
    assert!(verify_program(changed.clone(), &context, &Default::default()).is_err());
    changed.update_evidence = refresh(
        &changed.modules,
        &program.update_evidence,
        &context.snapshot(),
        &Default::default(),
    )
    .unwrap();
    verify_program(changed.clone(), &context, &Default::default()).unwrap();
    assert_eq!(
        program.update_evidence.bodies[0].instructions,
        changed.update_evidence.bodies[0].instructions
    );
    assert_ne!(
        program.update_evidence.bodies[0].actual_instructions,
        changed.update_evidence.bodies[0].actual_instructions
    );
    let mut forged = program;
    forged.update_evidence.bodies[0].classification =
        BodyClassification::Closure(forged.modules[0].functions[0].instance.clone());
    assert!(verify_program(forged, &context, &Default::default()).is_err());
}

#[test]
fn generated_parent_requires_immediate_identity_and_inherited_arguments() {
    let (context, mut program) = fixture();
    let module = &mut program.modules[0];
    let declaration = module.functions[0].instance.declaration;
    let NamedContract::Function(contract) = &mut module.abi.named_declarations[0].contract else {
        unreachable!()
    };
    contract.generic_params.push(GenericParam {
        owner: declaration,
        position: 0,
    });
    module.functions[0].instance.arguments = vec![Ty::Builtin(BuiltinType::I32)];
    let parent = module.functions[0].instance.clone();
    let mut other = module.functions[0].clone();
    other.id = InstanceId::new(1);
    other.instance.arguments = vec![Ty::Builtin(BuiltinType::I64)];
    let other_parent = other.instance.clone();
    let mut child = module.functions[0].clone();
    child.id = InstanceId::new(2);
    child.instance.declaration = context
        .intern_child(declaration, DefinitionKind::Function, "closure_0", 0)
        .unwrap();
    let child_identity = child.instance.clone();
    let mut nested = child.clone();
    nested.id = InstanceId::new(3);
    nested.instance.declaration = context
        .intern_child(
            child.instance.declaration,
            DefinitionKind::Function,
            "$resume",
            0,
        )
        .unwrap();
    let nested_identity = nested.instance.clone();
    module.functions.extend([other, child, nested]);
    let mut sources = program.update_evidence.sources.clone();
    sources[0].bodies = vec![child_identity.clone(), nested_identity.clone()];
    program.update_evidence = build(
        &program.modules,
        EvidenceInputs {
            sources,
            initializers: vec![],
            selections: vec![],
            classifications: [
                (parent.clone(), BodyClassification::Named(declaration)),
                (other_parent.clone(), BodyClassification::Named(declaration)),
                (
                    child_identity.clone(),
                    BodyClassification::Closure(parent.clone()),
                ),
                (nested_identity, BodyClassification::Resume(child_identity)),
            ]
            .into(),
        },
        &context.snapshot(),
        &Default::default(),
    )
    .unwrap();
    verify_program(program.clone(), &context, &Default::default()).unwrap();
    // No creation survives. An existing specialization of the same declaration
    // still cannot become the owner of a child with different concrete arguments.
    for classification in [
        BodyClassification::Closure(other_parent.clone()),
        BodyClassification::Async(other_parent.clone()),
        BodyClassification::Resume(other_parent),
    ] {
        let mut forged = program.clone();
        forged.update_evidence.bodies[2].classification = classification;
        assert!(verify_program(forged, &context, &Default::default()).is_err());
    }
    // Skipping an immediate generated parent is not lexical ownership, even if
    // the proposed ancestor has the same arguments and authored provenance.
    for classification in [
        BodyClassification::Closure(parent.clone()),
        BodyClassification::Async(parent.clone()),
        BodyClassification::Resume(parent.clone()),
        BodyClassification::Adapter(parent),
    ] {
        let mut forged = program.clone();
        forged.update_evidence.bodies[3].classification = classification;
        assert!(verify_program(forged, &context, &Default::default()).is_err());
    }
}

#[test]
fn update_evidence_default_selection_has_impl_identity_and_trait_source() {
    let (context, mut program) = fixture();
    let root = context.snapshot().lookup_root(&program.root).unwrap();
    let interface = context
        .intern_child(root, DefinitionKind::Trait, "Answer", 0)
        .unwrap();
    let source = context
        .intern_child(interface, DefinitionKind::Method, "answer", 0)
        .unwrap();
    let implementation = context
        .intern_child(root, DefinitionKind::Impl, "", 0)
        .unwrap();
    let selected = context
        .intern_child(implementation, DefinitionKind::Method, "answer", 0)
        .unwrap();
    let interface_type = Ty::Trait(NominalTy {
        declaration: interface,
        arguments: vec![],
        associated_types: Default::default(),
    });
    let module = &mut program.modules[0];
    module.abi.named_declarations[0].declaration = source;
    let NamedContract::Function(method) = module.abi.named_declarations[0].contract.clone() else {
        unreachable!()
    };
    module.abi.named_declarations.push(NamedDeclaration {
        declaration: implementation,
        visibility: Visibility::Private,
        ambiguous_local_body: None,
        contract: NamedContract::Implementation(ImplementationContract {
            bridge: None,
            generic_params: vec![],
            bounds: vec![],
            receiver: Ty::Builtin(BuiltinType::I32),
            trait_type: Some(interface_type.clone()),
        }),
    });
    module.abi.public_items = vec![
        PublicItem::Trait(TraitDef {
            name: "Answer".into(),
            associated_consts: vec![],
            supertraits: vec![],
            generic_params: vec![],
            bounds: vec![],
            methods: vec![method.clone()],
            associated_types: vec![],
            conversion_adapter: None,
        }),
        PublicItem::InterfaceTable(Box::new(InterfaceTable {
            declaration: implementation,
            name: "Answer for i32".into(),
            bridge: None,
            associated_type_families: vec![],
            associated_consts: vec![],
            generic_params: vec![],
            bounds: vec![],
            trait_type: interface_type,
            for_type: Ty::Builtin(BuiltinType::I32),
            methods: vec![method],
        })),
    ];
    module.functions[0].instance.declaration = selected;
    let instance = module.functions[0].instance.clone();
    let mut sources = program.update_evidence.sources.clone();
    sources[0].declaration = source;
    sources[0].bodies.clear();
    let mut owner = sources[0].clone();
    owner.declaration = implementation;
    sources.push(owner);
    program.update_evidence = build(
        &program.modules,
        EvidenceInputs {
            sources,
            initializers: vec![],
            selections: vec![],
            classifications: [(instance.clone(), BodyClassification::Named(selected))].into(),
        },
        &context.snapshot(),
        &Default::default(),
    )
    .unwrap();
    assert_eq!(program.update_evidence.targets[0].application, instance);
    assert!(
        program.update_evidence.sources[0]
            .bodies
            .contains(&instance)
    );
    assert!(
        program
            .update_evidence
            .sources
            .iter()
            .all(|source| source.declaration != selected)
    );
    let verified = verify_program(program.clone(), &context, &Default::default()).unwrap();
    let bytes = encode_program(&verified, &Default::default()).unwrap();
    let decoded = decode_program(&bytes, &Default::default()).unwrap();
    assert_eq!(
        encode_program(&decoded, &Default::default()).unwrap(),
        bytes
    );
    let mut detached = program.clone();
    detached.update_evidence.sources[0].bodies.clear();
    assert!(verify_program(detached, &context, &Default::default()).is_err());
    let mut closure = program.clone();
    closure.update_evidence.bodies[0].classification = BodyClassification::Closure(instance);
    assert!(
        refresh(
            &closure.modules,
            &closure.update_evidence,
            &context.snapshot(),
            &Default::default()
        )
        .is_err()
    );
    let mut required = program.clone();
    let PublicItem::Trait(contract) = &mut required.modules[0].abi.public_items[0] else {
        unreachable!()
    };
    contract.methods[0].implementation = CallableImplementation::Required;
    assert!(
        refresh(
            &required.modules,
            &required.update_evidence,
            &context.snapshot(),
            &Default::default()
        )
        .is_err()
    );
    let mut absent = program;
    absent.modules[0].abi.public_items.pop();
    assert!(
        refresh(
            &absent.modules,
            &absent.update_evidence,
            &context.snapshot(),
            &Default::default()
        )
        .is_err()
    );
}

#[test]
fn bridge_classification_requires_its_local_table_method_and_exact_body() {
    let (context, mut program) = fixture();
    let root = context.snapshot().lookup_root(&program.root).unwrap();
    let interface = context
        .intern_child(root, DefinitionKind::Trait, "Answer", 0)
        .unwrap();
    let implementation = context
        .intern_child(root, DefinitionKind::Impl, "", 0)
        .unwrap();
    let declaration = context
        .intern_child(implementation, DefinitionKind::Method, "answer", 0)
        .unwrap();
    let module = &mut program.modules[0];
    let NamedContract::Function(method) = module.abi.named_declarations.remove(0).contract else {
        unreachable!()
    };
    module.functions[0].instance.declaration = declaration;
    let instance = module.functions[0].instance.clone();
    module
        .abi
        .public_items
        .push(PublicItem::InterfaceTable(Box::new(InterfaceTable {
            declaration: implementation,
            name: "bridge".into(),
            bridge: Some(InterfaceBridge::Intrinsic),
            associated_type_families: vec![],
            associated_consts: vec![],
            generic_params: vec![],
            bounds: vec![],
            trait_type: Ty::Trait(NominalTy {
                declaration: interface,
                arguments: vec![],
                associated_types: Default::default(),
            }),
            for_type: Ty::Builtin(BuiltinType::I32),
            methods: vec![method],
        })));
    let extract = |module: &MirModule, classification| {
        build(
            std::slice::from_ref(module),
            EvidenceInputs {
                sources: vec![],
                initializers: vec![],
                selections: vec![],
                classifications: [(instance.clone(), classification)].into(),
            },
            &context.snapshot(),
            &Default::default(),
        )
    };
    // Extraction recognizes table authority; it does not grant a verifier seal.
    let evidence = extract(
        module,
        BodyClassification::IntrinsicAdapter(instance.clone()),
    )
    .unwrap();
    assert!(evidence.sources.is_empty());
    assert!(evidence.targets.is_empty());
    let mut forged_identity = instance.clone();
    forged_identity
        .arguments
        .push(Ty::Builtin(BuiltinType::I32));
    assert_eq!(
        extract(
            module,
            BodyClassification::IntrinsicAdapter(forged_identity)
        ),
        Err(UpdateError::InvalidEvidence)
    );
    assert_eq!(
        extract(module, BodyClassification::Named(declaration)),
        Err(UpdateError::InvalidEvidence)
    );
    let mut missing = module.clone();
    missing.abi.public_items.clear();
    assert_eq!(
        extract(
            &missing,
            BodyClassification::IntrinsicAdapter(instance.clone())
        ),
        Err(UpdateError::InvalidEvidence)
    );
    let mut wrong_method = module.clone();
    let PublicItem::InterfaceTable(table) = &mut wrong_method.abi.public_items[0] else {
        unreachable!()
    };
    table.methods[0].implementation = CallableImplementation::Required;
    assert_eq!(
        extract(
            &wrong_method,
            BodyClassification::IntrinsicAdapter(instance.clone())
        ),
        Err(UpdateError::InvalidEvidence)
    );
    program.update_evidence = evidence;
    // The constant-returning test body is not a valid protocol-forwarding bridge.
    assert!(verify_program(program, &context, &Default::default()).is_err());
}

#[test]
fn selected_construction_preserves_supplied_proof_and_checks_it_at_admission() {
    let (definitions, mut program) = fixture();
    let module = &mut program.modules[0];
    let root = definitions.intern_root(&module.identity).unwrap();
    let read = definitions
        .intern_child(root, DefinitionKind::Trait, "Read", 0)
        .unwrap();
    definitions
        .intern_child(read, DefinitionKind::Method, "read", 0)
        .unwrap();
    let view = definitions
        .intern_child(root, DefinitionKind::Trait, "View", 0)
        .unwrap();
    let selected = definitions
        .intern_child(root, DefinitionKind::Impl, "", 0)
        .unwrap();
    let nominal = |declaration| NominalTy {
        declaration,
        arguments: vec![],
        associated_types: Default::default(),
    };
    let contract = |name: &str| TraitDef {
        name: name.into(),
        conversion_adapter: None,
        generic_params: vec![],
        bounds: vec![],
        supertraits: vec![],
        associated_types: vec![],
        associated_consts: vec![],
        methods: vec![],
    };
    let mut read_contract = contract("Read");
    read_contract.methods.push(FnDecl {
        name: "read".into(),
        host_policy: None,
        method_policy: Default::default(),
        implementation: CallableImplementation::Required,
        generic_params: vec![],
        bounds: vec![],
        params: vec![Param {
            name: "self".into(),
            ty: Ty::SelfType(read),
            mutable: false,
        }],
        return_type: Ty::Builtin(BuiltinType::I32),
    });
    let applied = GenericParam {
        owner: selected,
        position: 0,
    };
    module.abi.public_items.extend([
        PublicItem::Trait(read_contract),
        PublicItem::Trait(contract("View")),
        PublicItem::InterfaceTable(Box::new(InterfaceTable {
            declaration: selected,
            name: "View for tuple".into(),
            bridge: None,
            generic_params: vec![applied.clone()],
            bounds: vec![GenericBound {
                ty: applied.as_type(),
                constraints: vec![nominal(read)],
            }],
            trait_type: Ty::Trait(nominal(view)),
            for_type: Ty::Tuple(vec![applied.as_type()]),
            methods: vec![],
            associated_consts: vec![],
            associated_type_families: vec![],
        })),
    ]);
    let function = &mut module.functions[0];
    let parameter = GenericParam {
        owner: function.instance.declaration,
        position: 0,
    };
    let generic = GenericBody {
        parameters: vec![parameter.clone()],
        bounds: vec![GenericBound {
            ty: parameter.as_type(),
            constraints: vec![nominal(read)],
        }],
    };
    let requirement = ConstructionRequirement {
        receiver: Ty::Tuple(vec![parameter.as_type()]),
        interface: nominal(view),
    };
    let application = ConcreteFunctionIdentity {
        declaration: selected,
        arguments: vec![parameter.as_type()],
    };
    function.semantic.generic = Some(generic.clone());
    function
        .semantic
        .registers
        .insert(0, requirement.receiver.clone());
    function
        .semantic
        .registers
        .insert(1, Ty::Trait(requirement.interface.clone()));
    function.blocks[0].instructions = vec![Instruction::MakeInterface {
        value: MirValue {
            temp: TempId::new(0),
            ty: ValueType::HeapObject,
        },
        dst: MirValue {
            temp: TempId::new(1),
            ty: ValueType::HeapObject,
        },
        target: ConstructionTarget::Declared {
            implementation: selected,
            arguments: application.arguments.clone(),
            operations: vec![OperationWitness::Forward(Box::new(
                NativeCallableRequirement {
                    receiver: parameter.as_type(),
                    interface: nominal(read),
                    member: definitions
                        .snapshot()
                        .lookup_child(read, DefinitionKind::Method, "read", 0)
                        .unwrap(),
                    arguments: vec![],
                },
            ))],
            constructions: vec![ConstructionWitness {
                requirement: ConstructionRequirement {
                    receiver: parameter.as_type(),
                    interface: nominal(read),
                },
                selection: ConstructionSelection::Forward,
            }],
        },
    }];
    let table = definitions.snapshot();
    let modules = [&*module];
    let cancel = CancellationToken::default();
    let mut budget = Budget::new(Limits::default(), &cancel).unwrap();
    let context = Context::new(&modules, &table, &cancel, &mut budget).unwrap();
    // Extraction retains the exact supplied selection and derives requirements
    // from operand types. Linked admission independently checks that evidence.
    let body = canonical::extract(
        &context,
        module,
        &module.functions[0],
        BodyClassification::Named(module.functions[0].instance.declaration),
        None,
        &mut budget,
    )
    .unwrap();
    let CallRequirement::Construction(witness) = &body.requirement.calls[0] else {
        panic!()
    };
    assert_eq!(witness.requirement, requirement);
    let ConstructionSelection::Declared {
        implementation,
        operations,
        constructions,
    } = &witness.selection
    else {
        panic!()
    };
    assert_eq!(implementation, &application);
    assert!(matches!(
        operations.as_slice(),
        [OperationWitness::Forward(_)]
    ));
    assert!(matches!(
        constructions.as_slice(),
        [ConstructionWitness {
            selection: ConstructionSelection::Forward,
            ..
        }]
    ));
    let catalog = ProofCatalog::new(
        module
            .abi
            .public_items
            .iter()
            .filter_map(|item| match item {
                PublicItem::InterfaceTable(table) => Some(Implementation::Interface(table)),
                _ => None,
            })
            .collect(),
        [],
        module
            .abi
            .public_items
            .iter()
            .filter_map(|item| match item {
                PublicItem::Trait(contract) => {
                    Some((if contract.name == "Read" { read } else { view }, contract))
                }
                _ => None,
            }),
        [],
        &table,
        &cancel,
    )
    .unwrap();
    assert!(
        witness
            .valid(&catalog, &generic.parameters, &generic.bounds, &cancel)
            .unwrap()
    );
    for change in 0..4 {
        let mut wrong = module.functions[0].clone();
        match change {
            0 => {
                wrong
                    .semantic
                    .registers
                    .insert(0, Ty::Builtin(BuiltinType::I32));
            }
            1 => {
                wrong.semantic.registers.insert(1, Ty::Trait(nominal(read)));
            }
            _ => {
                let Instruction::MakeInterface {
                    target:
                        ConstructionTarget::Declared {
                            operations,
                            constructions,
                            ..
                        },
                    ..
                } = &mut wrong.blocks[0].instructions[0]
                else {
                    unreachable!()
                };
                if change == 2 {
                    operations.clear();
                } else {
                    constructions.clear();
                }
            }
        }
        let body = canonical::extract(
            &context,
            module,
            &wrong,
            BodyClassification::Named(wrong.instance.declaration),
            None,
            &mut budget,
        )
        .unwrap();
        let CallRequirement::Construction(witness) = &body.requirement.calls[0] else {
            unreachable!()
        };
        assert!(
            !witness
                .valid(&catalog, &generic.parameters, &generic.bounds, &cancel)
                .unwrap()
        );
    }
}

#[test]
fn update_evidence_mir_cancellation_is_not_invalid_evidence() {
    let (context, program) = fixture();
    let cancel = CancellationToken::default();
    cancel.cancel();
    assert_eq!(
        refresh(
            &program.modules,
            &program.update_evidence,
            &context.snapshot(),
            &cancel
        ),
        Err(UpdateError::Cancelled)
    );
}
