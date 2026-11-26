use crate::{
    artifact::KbcArtifact,
    instruction::{
        BytecodeInstruction as I, CallTarget, ConstantId, ConstantOperand, JumpTarget, PathId, Slot,
    },
    module::{
        BytecodeFunction, BytecodeModule, FunctionMetadata, FunctionRecord, LocalLiveRange,
        PathRecord, RootSlotLayout,
    },
    program::{
        BytecodeProgram, ModuleRef,
        updates::{self, EvidenceInputs},
        verified::VerifiedBytecodeProgram,
        verify_program,
    },
    verifier::{BytecodeVerificationError, verify_module},
};
use kagari_abi::representation::ValueType;
use kagari_common::identity::{DefinitionKind, ModuleIdentity, map::DefinitionContext};
use kagari_contract::{
    ids::FunctionRef,
    types::{ConcreteFunctionIdentity, PublicItem, named::NamedContract},
    updates::BodyClassification,
};
use kagari_types::{
    declaration::{
        TypeDeclaration, TypeDef, TypeDefKind,
        host::{HostReflectionPolicy, HostTypeOwnership, HostTypePolicy, PathAccess},
        module::ModuleDecl,
        native::NativeStorageLayout,
        path::{HostPathDeclaration, HostPathSegmentDeclaration, HostVirtualSegmentDeclaration},
    },
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
    crate::program::updates::fixtures::certify(
        BytecodeProgram {
            update_evidence: crate::program::updates::empty(),
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
        },
        context,
    )
}

fn loop_program(context: &DefinitionContext) -> BytecodeProgram {
    let mut program = program(context);
    let module = &mut program.modules[0];
    let root = context.intern_root(&module.identity).unwrap();
    let host = context
        .intern_child(root, DefinitionKind::Struct, "HostRoot", 0)
        .unwrap();
    let definition = TypeDef {
        name: "HostRoot".into(),
        kind: TypeDefKind::Host(HostTypePolicy {
            ownership: HostTypeOwnership::HostRoot,
            reflection: HostReflectionPolicy::Metadata,
            path_access: PathAccess::ReadWrite,
        }),
        generic_params: vec![],
        bounds: vec![],
        fields: vec![],
        variants: vec![],
    };
    module
        .public_items
        .push(PublicItem::Type(definition.clone()));
    module.external_types.push(TypeDeclaration {
        declaration: host,
        definition,
    });
    module
        .types
        .extend([ValueType::Bool, ValueType::HostHandle]);
    let path = HostPathDeclaration {
        root: host,
        segments: vec![HostPathSegmentDeclaration::Virtual(
            HostVirtualSegmentDeclaration {
                name: "related".into(),
                result: Ty::Host(host),
                access: PathAccess::ReadOnly,
            },
        )],
        access: PathAccess::ReadOnly,
        schema_epoch: 1,
    };
    let definitions = context.snapshot();
    module.paths.push(PathRecord {
        id: PathId::new(0),
        contract_fingerprint: path
            .contract(&module.external_types, &definitions)
            .unwrap()
            .fingerprint(&definitions)
            .unwrap(),
        root_ty: ValueType::HostHandle,
        result_ty: ValueType::HostHandle,
        read_only: true,
        debug_name: "related".into(),
    });
    module.path_declarations.push(path);
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
    function.metadata.semantic.params.insert(3, Ty::Host(host));
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
    function.metadata.semantic.slots.insert(4, Ty::Host(host));
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
        I::MakePathView {
            dst: Slot::new(4),
            root_or_view: Slot::new(9),
            path: PathId::new(0),
            dynamic_args: vec![],
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
    module.named_declarations.clear();
    crate::program::updates::fixtures::certify(program, context)
}

#[test]
fn async_flow_validation_contract() {
    let context = DefinitionContext::new().unwrap();
    let valid = loop_program(&context);
    let verified =
        VerifiedBytecodeProgram::new(valid.clone(), &context, &Default::default()).unwrap();
    let points = verified.suspensions(ModuleRef::new(0)).unwrap();
    assert_eq!(points[0].len(), 1);
    let live: Vec<_> = points[0][0].live_slots().collect();
    assert_eq!(points[0][0].instruction(), 5);
    assert!(!live.contains(&4), "dead host temporary must be discarded");
    assert!(!live.contains(&9), "dead host parameter must be discarded");
    for mutation in 0..6 {
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
                // The parameter is debug-visible after its home was overwritten by a view.
                let I::MakePathView { dst, .. } = &mut function.instructions[3] else {
                    unreachable!()
                };
                *dst = Slot::new(9);
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
            5 => {
                function.instructions[3] = I::Move {
                    dst: Slot::new(4),
                    src: Slot::new(9),
                };
                function.metadata.semantic.slots.remove(&4);
                function.instructions[6] = I::Move {
                    dst: Slot::new(5),
                    src: Slot::new(4),
                };
                "host capability live across await"
            }
            _ => unreachable!(),
        };
        assert_eq!(
            verify_program(&invalid, &context.snapshot(), &Default::default()),
            Err(BytecodeVerificationError::InvalidOperation {
                function: FunctionRef::new(0),
                reason,
            }),
            "flow mutation {mutation}"
        );
    }
    let mut backedge = valid;
    let function = &mut backedge.modules[0].functions[0];
    function.instructions[7] = I::Jump {
        target: JumpTarget::new(1),
    };
    backedge.update_evidence = crate::program::updates::project(
        &backedge.modules,
        &backedge.update_evidence,
        &context.snapshot(),
        &Default::default(),
    )
    .unwrap();
    let verified = VerifiedBytecodeProgram::new(backedge, &context, &Default::default()).unwrap();
    let live = &verified.suspensions(ModuleRef::new(0)).unwrap()[0][0];
    assert!(
        live.live_slots().any(|slot| slot == 9),
        "backedge needs the original host parameter"
    );
    assert!(
        !live.live_slots().any(|slot| slot == 4),
        "dead path view is not parked"
    );
}

#[test]
fn source_free_retention_checks_only_the_projected_tuple_member() {
    let context = DefinitionContext::new().unwrap();
    let mut program = loop_program(&context);
    let module = &mut program.modules[0];
    module
        .constants
        .extend([ConstantOperand::I32(0), ConstantOperand::I32(1)]);
    module.types.push(ValueType::I32);
    let f = &mut module.functions[0];
    let slot = Slot::new;
    let tuple = Ty::Tuple(vec![f.metadata.semantic.params[&3].clone(); 2]);
    for i in [3, 10] {
        f.metadata.semantic.slots.insert(i, tuple.clone());
    }
    f.metadata.slots.extend([ValueType::I32; 2]);
    f.slot_count += 2;
    f.metadata.roots = RootSlotLayout::from_types(&f.metadata.slots);
    let load_index = |n| I::LoadConst {
        dst: slot(11),
        constant: ConstantId::new(n),
    };
    f.instructions = vec![
        f.instructions[3].clone(), // Produce a real path, typed identically to its root.
        I::Branch {
            cond: slot(8),
            then_target: JumpTarget::new(2),
            else_target: JumpTarget::new(5),
        },
        I::MakeTuple {
            dst: slot(3),
            elements: vec![slot(4), slot(9)],
        },
        load_index(1),
        I::Jump {
            target: JumpTarget::new(7),
        },
        I::MakeTuple {
            dst: slot(3),
            elements: vec![slot(9), slot(9)],
        },
        load_index(1),
        I::Move {
            dst: slot(10),
            src: slot(3),
        },
        I::Move {
            dst: slot(12),
            src: slot(11),
        },
        I::ReadAggregateIndex {
            dst: slot(4),
            base: slot(10),
            index: slot(12),
        },
        I::Await {
            dst: slot(1),
            value: slot(6),
            future: f.metadata.semantic.params[&0].clone(),
        },
        I::Move {
            dst: slot(5),
            src: slot(4),
        },
        I::Return(Some(slot(1))),
    ];
    f.metadata.origins = (0..f.instructions.len())
        .map(|instruction| crate::module::InstructionOrigin {
            instruction,
            span: Default::default(),
            line: None,
            column: None,
        })
        .collect();
    let valid = crate::program::updates::fixtures::certify(program, &context);
    let verified =
        VerifiedBytecodeProgram::new(valid.clone(), &context, &Default::default()).unwrap();
    let point = &verified.suspensions(ModuleRef::new(0)).unwrap()[0][0];
    assert_eq!(point.live_slots().collect::<Vec<_>>(), [4, 6]);
    let mut invalid = valid.clone();
    // The index now differs across predecessors. On the view-producing edge it
    // selects the view; the other edge must not erase that possibility.
    invalid.modules[0].functions[0].instructions[3] = load_index(0);
    assert_eq!(
        verify_program(&invalid, &context.snapshot(), &Default::default()),
        Err(BytecodeVerificationError::InvalidOperation {
            function: FunctionRef::new(0),
            reason: "host capability live across await"
        })
    );

    // Exercise the same joined projection at publication rather than parking.
    let mut module = valid.modules.into_iter().next().unwrap();
    let f = &mut module.functions[0];
    f.instructions[10] = I::MakeFuture {
        dst: slot(0),
        function: FunctionRef::new(0),
        arguments: [6, 7, 8, 4].map(slot).to_vec(),
        future: f.metadata.semantic.params[&0].clone(),
    };
    f.instructions[12] = I::Return(Some(slot(7)));
    verify_module(&module, &context.snapshot(), &Default::default()).unwrap();
    module.functions[0].instructions[3] = load_index(0);
    assert_eq!(
        verify_module(&module, &context.snapshot(), &Default::default()),
        Err(BytecodeVerificationError::InvalidOperation {
            function: FunctionRef::new(0),
            reason: "cannot capture a host path view"
        })
    );
}

#[test]
fn ordinary_closure_rejects_a_view_on_one_incoming_edge() {
    let context = DefinitionContext::new().unwrap();
    let mut program = loop_program(&context);
    let module = &mut program.modules[0];
    let function = &mut module.functions[0];
    function.metadata.effects.may_suspend = false;
    module.function_table[0].effects = function.metadata.effects;
    let copy_handle = I::Move {
        dst: Slot::new(4),
        src: Slot::new(9),
    };
    function.instructions[0] = copy_handle.clone();
    let I::Branch { else_target, .. } = &mut function.instructions[2] else {
        unreachable!()
    };
    *else_target = JumpTarget::new(5);
    let view = std::mem::replace(&mut function.instructions[3], copy_handle);
    function.instructions[5] = I::MakeClosure {
        dst: Slot::new(10),
        function: FunctionRef::new(0),
        captures: [6, 7, 8, 4].map(Slot::new).to_vec(),
    };
    function.metadata.semantic.slots.insert(
        10,
        Ty::Function {
            params: vec![],
            result: Box::new(function.metadata.semantic.result.clone().unwrap()),
        },
    );
    function.instructions[6] = I::Move {
        dst: Slot::new(1),
        src: Slot::new(7),
    };
    verify_module(module, &context.snapshot(), &Default::default()).unwrap();
    module.functions[0].instructions[3] = view;
    assert_eq!(
        verify_module(module, &context.snapshot(), &Default::default()),
        Err(BytecodeVerificationError::InvalidOperation {
            function: FunctionRef::new(0),
            reason: "cannot capture a host path view"
        })
    );
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
        let artifact = KbcArtifact::from_program(
            valid.clone(),
            Default::default(),
            &context,
            &Default::default(),
        )
        .unwrap();
        let decoded = KbcArtifact::from_bytes(&artifact.to_bytes(&context.snapshot()).unwrap())
            .map(|metadata| {
                metadata
                    .import_into(&context, &Default::default())
                    .unwrap()
                    .into_records()
            })
            .unwrap();
        decoded
            .validate_for_loader(&Default::default(), &context, &Default::default())
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
                verify_program(&invalid, &context.snapshot(), &Default::default()).is_err(),
                "invalid {layout:?} await contract {mutation}"
            );
        }
    }
}

#[test]
fn async_factory_artifact_contract() {
    let context = DefinitionContext::new().unwrap();
    let mut valid = loop_program(&context);
    let module = &mut valid.modules[0];
    let mut factory = module.functions[0].clone();
    let future = factory.metadata.semantic.params[&0].clone();
    factory.id = FunctionRef::new(1);
    factory.name = "factory".into();
    let factory_identity = ConcreteFunctionIdentity {
        declaration: context
            .intern_child(
                context.intern_root(&module.identity).unwrap(),
                DefinitionKind::Function,
                "factory",
                0,
            )
            .unwrap(),
        arguments: vec![],
    };
    let resume_identity = ConcreteFunctionIdentity {
        declaration: context
            .intern_child(
                factory_identity.declaration,
                DefinitionKind::Function,
                "$resume",
                0,
            )
            .unwrap(),
        arguments: factory_identity.arguments.clone(),
    };
    module.functions[0].identity = Some(resume_identity.clone());
    module.function_table[0].identity = Some(resume_identity.clone());
    factory.identity = Some(factory_identity.clone());
    let declaration = &mut module.named_declarations[0];
    declaration.declaration = factory_identity.declaration;
    let NamedContract::Function(signature) = &mut declaration.contract else {
        unreachable!()
    };
    signature.name = factory.name.clone();
    signature.return_type = future.clone();
    factory.metadata.effects.may_suspend = false;
    factory.metadata.semantic.result = Some(future.clone());
    factory.metadata.semantic.slots.insert(1, future.clone());
    factory.instructions = vec![
        I::Move {
            dst: Slot::new(0),
            src: Slot::new(6),
        },
        I::MakeFuture {
            dst: Slot::new(1),
            function: FunctionRef::new(0),
            arguments: [0, 7, 8, 9].map(Slot::new).to_vec(),
            future,
        },
        I::Return(Some(Slot::new(1))),
    ];
    factory.metadata.origins.truncate(3);
    module.function_table.push(FunctionRecord {
        id: factory.id,
        identity: factory.identity.clone(),
        name: factory.name.clone(),
        params: factory.metadata.params.clone(),
        return_type: factory.metadata.return_type,
        effects: factory.metadata.effects,
    });
    module.functions.push(factory);
    let mut sources = valid.update_evidence.sources.clone();
    sources[0].declaration = factory_identity.declaration;
    sources[0].bodies = vec![resume_identity.clone()];
    valid.update_evidence = updates::build(
        &valid.modules,
        EvidenceInputs {
            sources,
            initializers: vec![],
            selections: vec![],
            classifications: [
                (
                    resume_identity,
                    BodyClassification::Resume(factory_identity.clone()),
                ),
                (
                    factory_identity.clone(),
                    BodyClassification::Named(factory_identity.declaration),
                ),
            ]
            .into(),
        },
        &context.snapshot(),
        &Default::default(),
    )
    .unwrap();
    let artifact = KbcArtifact::from_program(
        valid.clone(),
        Default::default(),
        &context,
        &Default::default(),
    )
    .unwrap();
    KbcArtifact::from_bytes(&artifact.to_bytes(&context.snapshot()).unwrap())
        .map(|metadata| {
            metadata
                .import_into(&context, &Default::default())
                .unwrap()
                .into_records()
        })
        .unwrap()
        .validate_for_loader(&Default::default(), &context, &Default::default())
        .unwrap();
    let mut view_capture = valid.clone();
    let factory = &mut view_capture.modules[0].functions[1];
    factory.instructions[0] = I::MakePathView {
        dst: Slot::new(4),
        root_or_view: Slot::new(9),
        path: PathId::new(0),
        dynamic_args: vec![],
    };
    let I::MakeFuture { arguments, .. } = &mut factory.instructions[1] else {
        unreachable!()
    };
    arguments[0] = Slot::new(6);
    arguments[3] = Slot::new(4);
    assert_eq!(
        verify_program(&view_capture, &context.snapshot(), &Default::default()),
        Err(BytecodeVerificationError::InvalidOperation {
            function: FunctionRef::new(1),
            reason: "cannot capture a host path view"
        })
    );
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
            verify_program(&invalid, &context.snapshot(), &Default::default()).is_err(),
            "factory mutation {mutation}"
        );
    }
}
