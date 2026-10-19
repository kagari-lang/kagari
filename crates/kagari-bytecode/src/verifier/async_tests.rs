use crate::{
    artifact::KbcArtifact,
    instruction::{BytecodeInstruction as I, CallTarget, JumpTarget, Slot},
    module::{
        BytecodeFunction, BytecodeModule, FunctionMetadata, FunctionRecord, LocalLiveRange,
        RootSlotLayout,
    },
    program::{BytecodeProgram, ModuleRef, verified::VerifiedBytecodeProgram, verify_program},
    verifier::BytecodeVerificationError,
};
use kagari_abi::representation::ValueType;
use kagari_common::identity::{DefinitionKind, ModuleIdentity, map::DefinitionContext};
use kagari_contract::{ids::FunctionRef, types::PublicItem};
use kagari_types::{
    declaration::{TypeDef, TypeDefKind, module::ModuleDecl, native::NativeStorageLayout},
    scalar::BuiltinType,
    ty::{GenericParam, NominalTy, Ty},
};

fn program(context: &DefinitionContext) -> BytecodeProgram {
    let owner = ModuleDecl::new(ModuleIdentity::single_file("await-contract"));
    let root = context.intern_root(&owner.identity).unwrap();
    let id = context
        .intern_child(root, DefinitionKind::AssociatedType, "Future", 0)
        .unwrap();
    let output = Ty::Array(Box::new(Ty::Builtin(BuiltinType::I32)));
    let future = Ty::NativeObject(NominalTy {
        declaration: id,
        arguments: vec![output.clone()],
        associated_types: Default::default(),
    });
    let mut metadata = FunctionMetadata {
        params: vec![ValueType::HeapObject],
        return_type: ValueType::HeapObject,
        roots: RootSlotLayout::from_types(
            &[
                [ValueType::HeapObject; 2].to_vec(),
                [ValueType::HeapObject].to_vec(),
            ]
            .concat(),
        ),
        slots: [vec![ValueType::HeapObject; 2], vec![ValueType::HeapObject]].concat(),
        ..Default::default()
    };
    metadata.effects.may_suspend = true;
    metadata.semantic.params.insert(0, future.clone());
    metadata.semantic.slots.insert(2, future.clone());
    metadata.semantic.slots.insert(0, future.clone());
    metadata.semantic.slots.insert(1, output.clone());
    metadata.semantic.result = Some(output);
    let record = FunctionRecord {
        id: FunctionRef::new(0),
        identity: None,
        name: "wait".into(),
        params: metadata.params.clone(),
        return_type: metadata.return_type,
        effects: metadata.effects,
    };
    BytecodeProgram {
        exports: Default::default(),
        root: ModuleRef::new(0),
        modules: vec![BytecodeModule {
            identity: owner.identity,
            types: vec![ValueType::Unit, ValueType::HeapObject],
            public_items: vec![PublicItem::Type(TypeDef {
                name: "Future".into(),
                kind: TypeDefKind::NativeStorage(NativeStorageLayout::Future),
                generic_params: vec![GenericParam {
                    owner: id,
                    position: 0,
                }],
                bounds: vec![],
                fields: vec![],
                variants: vec![],
            })],
            function_table: vec![record],
            functions: vec![{
                let parameter_base: usize = 2;
                let fixture_instructions = vec![
                    I::Move {
                        dst: Slot::new(0),
                        src: Slot::new(parameter_base),
                    },
                    I::Await {
                        dst: Slot::new(1),
                        value: Slot::new(0),
                        future,
                    },
                    I::Return(Some(Slot::new(1))),
                ];
                let mut fixture_metadata: crate::module::FunctionMetadata = metadata;
                if fixture_metadata.parameter_slots.is_empty() {
                    fixture_metadata.parameter_slots =
                        (0..1).map(|i| Slot::new(parameter_base + i)).collect();
                }
                if fixture_metadata.origins.is_empty() {
                    fixture_metadata.origins = (0..fixture_instructions.len())
                        .map(|instruction| crate::module::InstructionOrigin {
                            span: Default::default(),
                            line: None,
                            column: None,
                            instruction,
                        })
                        .collect();
                }
                BytecodeFunction {
                    id: FunctionRef::new(0),
                    identity: None,
                    name: "wait".into(),
                    parameter_count: 1,
                    slot_count: u32::try_from(fixture_metadata.slots.len()).unwrap(),
                    metadata: fixture_metadata,
                    instructions: fixture_instructions,
                }
            }],
            ..Default::default()
        }],
    }
}

fn loop_program(context: &DefinitionContext) -> BytecodeProgram {
    let mut program = program(context);
    let module = &mut program.modules[0];
    module
        .types
        .extend([ValueType::Bool, ValueType::HostHandle]);
    let function = &mut module.functions[0];
    let array = function.metadata.semantic.result.clone().unwrap();
    function.parameter_count = 4;
    function.slot_count = 11;
    function.metadata.params.extend([
        ValueType::HeapObject,
        ValueType::Bool,
        ValueType::HostHandle,
    ]);
    function.metadata.slots = vec![
        ValueType::HeapObject,
        ValueType::HeapObject,
        ValueType::Bool,
        ValueType::HeapObject,
        ValueType::HostHandle,
        ValueType::HostHandle,
        ValueType::HeapObject,
        ValueType::HeapObject,
        ValueType::Bool,
        ValueType::HostHandle,
        ValueType::HeapObject,
    ];
    function.metadata.parameter_slots = (6..10).map(Slot::new).collect();
    function.metadata.semantic.params.insert(1, array.clone());
    function
        .metadata
        .semantic
        .params
        .insert(2, Ty::Builtin(BuiltinType::Bool));
    function.metadata.semantic.slots.retain(|&slot, _| slot < 2);
    for (&index, ty) in &function.metadata.semantic.params {
        function
            .metadata
            .semantic
            .slots
            .insert(6 + index, ty.clone());
    }
    function.metadata.semantic.slots.insert(10, array.clone());
    function
        .metadata
        .semantic
        .slots
        .insert(2, Ty::Builtin(BuiltinType::Bool));
    function.metadata.semantic.slots.insert(3, array);
    function.metadata.roots = RootSlotLayout::from_types(&function.metadata.slots);
    let await_instruction = function.instructions[1].clone();
    function.instructions = vec![
        I::Move {
            dst: Slot::new(3),
            src: Slot::new(7),
        },
        I::Move {
            dst: Slot::new(2),
            src: Slot::new(8),
        },
        I::Branch {
            cond: Slot::new(2),
            then_target: JumpTarget::new(3),
            else_target: JumpTarget::new(8),
        },
        I::Move {
            dst: Slot::new(4),
            src: Slot::new(9),
        },
        I::Move {
            dst: Slot::new(0),
            src: Slot::new(6),
        },
        await_instruction,
        I::Move {
            dst: Slot::new(7),
            src: Slot::new(1),
        },
        I::Jump {
            target: JumpTarget::new(1),
        },
        I::Move {
            dst: Slot::new(1),
            src: Slot::new(7),
        },
        I::Return(Some(Slot::new(1))),
    ];
    function.metadata.origins = (0..function.instructions.len())
        .map(|instruction| crate::module::InstructionOrigin {
            span: Default::default(),
            line: None,
            column: None,
            instruction,
        })
        .collect();
    // The host parameter is dead after its pre-await read, including the next
    // iteration. Overwrite that local with another host value before a backedge
    // would still make it live, so use a single iteration's terminal path here.
    function.instructions[7] = I::Jump {
        target: JumpTarget::new(8),
    };
    module.function_table[0].params = function.metadata.params.clone();
    program
}

#[test]
fn async_flow_validation_contract() {
    let context = DefinitionContext::new().unwrap();
    let valid = loop_program(&context);
    let verified = VerifiedBytecodeProgram::new(valid.clone(), &context).unwrap();
    let points = verified.suspensions(ModuleRef::new(0)).unwrap();
    assert_eq!(points[0].len(), 1);
    let live: Vec<_> = points[0][0].live_slots().collect();
    assert_eq!(points[0][0].instruction(), 5);
    assert!(!live.contains(&4), "dead host temporary must be discarded");
    assert!(!live.contains(&9), "dead host parameter must be discarded");
    for mutation in 0..5 {
        let mut invalid = valid.clone();
        let function = &mut invalid.modules[0].functions[0];
        let reason = match mutation {
            0 => {
                function.instructions[1] = I::Jump {
                    target: JumpTarget::new(2),
                };
                "uninitialized executable slot"
            }
            1 => {
                function.instructions[4] = I::Move {
                    dst: Slot::new(7),
                    src: Slot::new(1),
                };
                "uninitialized executable slot"
            }
            2 => {
                function.instructions[8] = I::Move {
                    dst: Slot::new(1),
                    src: Slot::new(10),
                };
                "uninitialized executable slot"
            }
            3 => {
                function.instructions[6] = I::Move {
                    dst: Slot::new(5),
                    src: Slot::new(4),
                };
                "host capability live across await"
            }
            4 => {
                function
                    .metadata
                    .debug
                    .local_live_ranges
                    .push(LocalLiveRange {
                        slot: Slot::new(9),
                        name: "host".into(),
                        span: Default::default(),
                        start: 0,
                        end: 7,
                        ty: ValueType::HostHandle,
                        is_parameter: true,
                        binding: 0,
                    });
                "host capability live across await"
            }
            _ => unreachable!(),
        };
        assert_eq!(
            verify_program(&invalid, &context.snapshot()),
            Err(BytecodeVerificationError::InvalidOperation {
                function: FunctionRef::new(0),
                reason,
            }),
            "flow mutation {mutation}"
        );
    }
    let mut backedge = valid;
    let function = &mut backedge.modules[0].functions[0];
    function.instructions[3] = I::Jump {
        target: JumpTarget::new(4),
    };
    function.instructions[7] = I::Jump {
        target: JumpTarget::new(1),
    };
    verify_program(&backedge, &context.snapshot()).unwrap();
}

#[test]
fn async_artifact_validation_contract() {
    let context = DefinitionContext::new().unwrap();
    for layout in [NativeStorageLayout::Future, NativeStorageLayout::Task] {
        let mut valid = program(&context);
        let PublicItem::Type(ty) = &mut valid.modules[0].public_items[0] else {
            unreachable!()
        };
        ty.kind = TypeDefKind::NativeStorage(layout);
        let artifact =
            KbcArtifact::from_program(valid.clone(), Default::default(), &context).unwrap();
        let decoded = KbcArtifact::from_bytes(&artifact.to_bytes(&context.snapshot()).unwrap())
            .map(|metadata| {
                metadata
                    .import_into(&context, &Default::default())
                    .unwrap()
                    .into_records()
            })
            .unwrap();
        decoded
            .validate_for_loader(&Default::default(), &context)
            .unwrap();
        // Every case keeps the physical heap-object representation unchanged.
        for mutation in 0..6 {
            let mut invalid = valid.clone();
            let module = &mut invalid.modules[0];
            let function = &mut module.functions[0];
            match mutation {
                0 => {
                    let PublicItem::Type(ty) = &mut module.public_items[0] else {
                        unreachable!()
                    };
                    ty.kind = TypeDefKind::NativeStorage(NativeStorageLayout::Opaque);
                }
                1 => {
                    function.metadata.effects.may_suspend = false;
                    module.function_table[0].effects.may_suspend = false;
                }
                2 => {
                    function.metadata.semantic.slots.remove(&0);
                }
                3 => {
                    function
                        .metadata
                        .semantic
                        .slots
                        .insert(1, Ty::Array(Box::new(Ty::Builtin(BuiltinType::Bool))));
                }
                4 => {
                    let I::Await { value, .. } = &mut function.instructions[1] else {
                        unreachable!()
                    };
                    *value = Slot::new(1);
                }
                5 => {
                    function.instructions[1] = I::Call {
                        dst: Some(Slot::new(1)),
                        callee: CallTarget::Function(FunctionRef::new(0)),
                        args: vec![Slot::new(0)],
                    };
                }
                _ => unreachable!(),
            }
            assert!(
                verify_program(&invalid, &context.snapshot()).is_err(),
                "invalid {layout:?} await contract {mutation}"
            );
        }
    }
}

#[test]
fn async_factory_artifact_contract() {
    let context = DefinitionContext::new().unwrap();
    let mut valid = program(&context);
    let module = &mut valid.modules[0];
    let mut factory = module.functions[0].clone();
    let future = factory.metadata.semantic.params[&0].clone();
    factory.id = FunctionRef::new(1);
    factory.name = "factory".into();
    factory.metadata.effects.may_suspend = false;
    factory.metadata.semantic.result = Some(future.clone());
    factory.metadata.semantic.slots.insert(1, future.clone());
    factory.instructions[1] = I::MakeFuture {
        dst: Slot::new(1),
        function: FunctionRef::new(0),
        arguments: vec![Slot::new(0)],
        future,
    };
    module.function_table.push(FunctionRecord {
        id: factory.id,
        identity: None,
        name: factory.name.clone(),
        params: factory.metadata.params.clone(),
        return_type: factory.metadata.return_type,
        effects: factory.metadata.effects,
    });
    module.functions.push(factory);
    let artifact = KbcArtifact::from_program(valid.clone(), Default::default(), &context).unwrap();
    KbcArtifact::from_bytes(&artifact.to_bytes(&context.snapshot()).unwrap())
        .map(|metadata| {
            metadata
                .import_into(&context, &Default::default())
                .unwrap()
                .into_records()
        })
        .unwrap()
        .validate_for_loader(&Default::default(), &context)
        .unwrap();
    for mutation in 0..6 {
        let mut invalid = valid.clone();
        let factory = &mut invalid.modules[0].functions[1];
        if mutation == 5 {
            let PublicItem::Type(ty) = &mut invalid.modules[0].public_items[0] else {
                unreachable!()
            };
            ty.kind = TypeDefKind::NativeStorage(NativeStorageLayout::Task);
        } else if mutation == 4 {
            factory.instructions[0] = I::Jump {
                target: JumpTarget::new(1),
            };
        } else {
            let I::MakeFuture {
                function,
                arguments,
                future,
                ..
            } = &mut factory.instructions[1]
            else {
                unreachable!();
            };
            match mutation {
                0 => *function = FunctionRef::new(99),
                1 => *function = FunctionRef::new(1),
                2 => arguments.clear(),
                3 => {
                    let Ty::NativeObject(nominal) = future else {
                        unreachable!();
                    };
                    nominal.arguments[0] = Ty::Builtin(BuiltinType::Bool);
                }
                _ => unreachable!(),
            }
        }
        assert!(
            verify_program(&invalid, &context.snapshot()).is_err(),
            "factory mutation {mutation}"
        );
    }
}
