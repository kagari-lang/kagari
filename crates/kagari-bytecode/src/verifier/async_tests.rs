use crate::{
    artifact::KbcArtifact,
    instruction::{BytecodeInstruction as I, CallTarget, JumpTarget, LocalSlot, Register},
    module::{
        BytecodeFunction, BytecodeModule, FunctionMetadata, FunctionRecord, LocalLiveRange,
        RootSlotLayout,
    },
    program::{BytecodeProgram, ModuleRef, verified::VerifiedBytecodeProgram, verify_program},
    verifier::BytecodeVerificationError,
};
use kagari_abi::representation::ValueType;
use kagari_common::identity::{DefinitionKind, ModuleIdentity};
use kagari_contract::{ids::FunctionRef, types::PublicItem};
use kagari_types::{
    collection::CollectionAccess,
    declaration::{TypeDef, TypeDefKind, module::ModuleDecl, native::NativeStorageLayout},
    scalar::BuiltinType,
    ty::{GenericParam, NominalTy, Ty},
};

fn program() -> BytecodeProgram {
    let owner = ModuleDecl::new(ModuleIdentity::single_file("await-contract"));
    let id = owner.definition(DefinitionKind::AssociatedType, "Future");
    let output = Ty::Array(
        Box::new(Ty::Builtin(BuiltinType::I32)),
        CollectionAccess::Mutable,
    );
    let future = Ty::NativeObject(NominalTy {
        declaration: id.clone(),
        arguments: vec![output.clone()],
        associated_types: Default::default(),
    });
    let mut metadata = FunctionMetadata {
        params: vec![ValueType::HeapObject],
        locals: vec![ValueType::HeapObject],
        registers: vec![ValueType::HeapObject; 2],
        return_type: ValueType::HeapObject,
        roots: RootSlotLayout::from_types(&[ValueType::HeapObject], &[ValueType::HeapObject; 2]),
        ..Default::default()
    };
    metadata.effects.may_suspend = true;
    metadata.semantic.params.insert(0, future.clone());
    metadata.semantic.locals.insert(0, future.clone());
    metadata.semantic.registers.insert(0, future.clone());
    metadata.semantic.registers.insert(1, output.clone());
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
            functions: vec![BytecodeFunction {
                id: FunctionRef::new(0),
                identity: None,
                name: "wait".into(),
                parameter_count: 1,
                register_count: 2,
                local_count: 1,
                metadata,
                instructions: vec![
                    I::LoadLocal {
                        dst: Register::new(0),
                        local: LocalSlot::new(0),
                    },
                    I::Await {
                        dst: Register::new(1),
                        value: Register::new(0),
                        future,
                    },
                    I::Return(Some(Register::new(1))),
                ],
            }],
            ..Default::default()
        }],
    }
}

fn loop_program() -> BytecodeProgram {
    let mut program = program();
    let module = &mut program.modules[0];
    module
        .types
        .extend([ValueType::Bool, ValueType::HostHandle]);
    let function = &mut module.functions[0];
    let array = function.metadata.semantic.result.clone().unwrap();
    function.parameter_count = 4;
    function.local_count = 5;
    function.register_count = 6;
    function.metadata.params.extend([
        ValueType::HeapObject,
        ValueType::Bool,
        ValueType::HostHandle,
    ]);
    function.metadata.locals = function.metadata.params.clone();
    function.metadata.locals.push(ValueType::HeapObject);
    function.metadata.registers.extend([
        ValueType::Bool,
        ValueType::HeapObject,
        ValueType::HostHandle,
        ValueType::HostHandle,
    ]);
    function.metadata.semantic.params.insert(1, array.clone());
    function
        .metadata
        .semantic
        .params
        .insert(2, Ty::Builtin(BuiltinType::Bool));
    function.metadata.semantic.locals = function.metadata.semantic.params.clone();
    function.metadata.semantic.locals.insert(4, array.clone());
    function
        .metadata
        .semantic
        .registers
        .insert(2, Ty::Builtin(BuiltinType::Bool));
    function.metadata.semantic.registers.insert(3, array);
    function.metadata.roots =
        RootSlotLayout::from_types(&function.metadata.locals, &function.metadata.registers);
    let await_instruction = function.instructions[1].clone();
    function.instructions = vec![
        I::LoadLocal {
            dst: Register::new(3),
            local: LocalSlot::new(1),
        },
        I::BeginIteration {
            collection: Register::new(3),
        },
        I::LoadLocal {
            dst: Register::new(2),
            local: LocalSlot::new(2),
        },
        I::Branch {
            cond: Register::new(2),
            then_target: JumpTarget::new(4),
            else_target: JumpTarget::new(9),
        },
        I::LoadLocal {
            dst: Register::new(4),
            local: LocalSlot::new(3),
        },
        I::LoadLocal {
            dst: Register::new(0),
            local: LocalSlot::new(0),
        },
        await_instruction,
        I::StoreLocal {
            local: LocalSlot::new(1),
            src: Register::new(1),
        },
        I::Jump {
            target: JumpTarget::new(2),
        },
        I::EndIteration,
        I::LoadLocal {
            dst: Register::new(1),
            local: LocalSlot::new(1),
        },
        I::Return(Some(Register::new(1))),
    ];
    // The host parameter is dead after its pre-await read, including the next
    // iteration. Overwrite that local with another host value before a backedge
    // would still make it live, so use a single iteration's terminal path here.
    function.instructions[8] = I::Jump {
        target: JumpTarget::new(9),
    };
    module.function_table[0].params = function.metadata.params.clone();
    program
}

#[test]
fn async_flow_validation_contract() {
    let valid = loop_program();
    let verified = VerifiedBytecodeProgram::new(valid.clone()).unwrap();
    let points = verified.suspensions(ModuleRef::new(0)).unwrap();
    assert_eq!(points[0].len(), 1);
    let live: Vec<_> = points[0][0].live_slots().collect();
    assert_eq!(points[0][0].instruction(), 6);
    assert!(!live.contains(&4), "dead host temporary must be discarded");
    assert!(!live.contains(&9), "dead host parameter must be discarded");
    for mutation in 0..7 {
        let mut invalid = valid.clone();
        let function = &mut invalid.modules[0].functions[0];
        let reason = match mutation {
            0 => {
                function.instructions[0] = I::Jump {
                    target: JumpTarget::new(1),
                };
                "uninitialized slot in resume body"
            }
            1 => {
                function.instructions[5] = I::StoreLocal {
                    local: LocalSlot::new(1),
                    src: Register::new(1),
                };
                "uninitialized slot in resume body"
            }
            2 => {
                function.instructions[10] = I::LoadLocal {
                    dst: Register::new(1),
                    local: LocalSlot::new(4),
                };
                "uninitialized slot in resume body"
            }
            3 => {
                function.instructions[1] = I::EndIteration;
                "iteration resource underflow"
            }
            4 => {
                function.instructions[8] = I::Jump {
                    target: JumpTarget::new(1),
                };
                "inconsistent iteration resource stack"
            }
            5 => {
                function.instructions[7] = I::Move {
                    dst: Register::new(5),
                    src: Register::new(4),
                };
                "host capability live across await"
            }
            6 => {
                function
                    .metadata
                    .debug
                    .local_live_ranges
                    .push(LocalLiveRange {
                        local: LocalSlot::new(3),
                        name: "host".into(),
                        span: Default::default(),
                        start: 0,
                        end: 7,
                        ty: ValueType::HostHandle,
                        is_parameter: true,
                    });
                "host capability live across await"
            }
            _ => unreachable!(),
        };
        assert_eq!(
            verify_program(&invalid),
            Err(BytecodeVerificationError::InvalidOperation {
                function: FunctionRef::new(0),
                reason,
            }),
            "flow mutation {mutation}"
        );
    }
    let mut backedge = valid;
    let function = &mut backedge.modules[0].functions[0];
    function.instructions[4] = I::Jump {
        target: JumpTarget::new(5),
    };
    function.instructions[8] = I::Jump {
        target: JumpTarget::new(2),
    };
    verify_program(&backedge).unwrap();
}

#[test]
fn async_artifact_validation_contract() {
    for layout in [NativeStorageLayout::Future, NativeStorageLayout::Task] {
        let mut valid = program();
        let PublicItem::Type(ty) = &mut valid.modules[0].public_items[0] else {
            unreachable!()
        };
        ty.kind = TypeDefKind::NativeStorage(layout);
        let artifact = KbcArtifact::from_program(valid.clone(), Default::default()).unwrap();
        let decoded = KbcArtifact::from_bytes(&artifact.to_bytes().unwrap()).unwrap();
        decoded.validate_for_loader(&Default::default()).unwrap();
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
                    function.metadata.semantic.registers.remove(&0);
                }
                3 => {
                    function.metadata.semantic.registers.insert(
                        1,
                        Ty::Array(
                            Box::new(Ty::Builtin(BuiltinType::I32)),
                            CollectionAccess::ReadOnly,
                        ),
                    );
                }
                4 => {
                    let I::Await { value, .. } = &mut function.instructions[1] else {
                        unreachable!()
                    };
                    *value = Register::new(1);
                }
                5 => {
                    function.instructions[1] = I::Call {
                        dst: Some(Register::new(1)),
                        callee: CallTarget::Function(FunctionRef::new(0)),
                        args: vec![Register::new(0)],
                    };
                }
                _ => unreachable!(),
            }
            assert!(
                verify_program(&invalid).is_err(),
                "invalid {layout:?} await contract {mutation}"
            );
        }
    }
}

#[test]
fn async_factory_artifact_contract() {
    let mut valid = program();
    let module = &mut valid.modules[0];
    let mut factory = module.functions[0].clone();
    let future = factory.metadata.semantic.params[&0].clone();
    factory.id = FunctionRef::new(1);
    factory.name = "factory".into();
    factory.metadata.effects.may_suspend = false;
    factory.metadata.semantic.result = Some(future.clone());
    factory
        .metadata
        .semantic
        .registers
        .insert(1, future.clone());
    factory.instructions[1] = I::MakeFuture {
        dst: Register::new(1),
        function: FunctionRef::new(0),
        arguments: vec![Register::new(0)],
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
    let artifact = KbcArtifact::from_program(valid.clone(), Default::default()).unwrap();
    KbcArtifact::from_bytes(&artifact.to_bytes().unwrap())
        .unwrap()
        .validate_for_loader(&Default::default())
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
            factory.instructions.remove(0);
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
            verify_program(&invalid).is_err(),
            "factory mutation {mutation}"
        );
    }
}
