//! Public frame access cannot turn a previous physical alias into a logical value.
use crate::{
    Runtime,
    error::RuntimeErrorKind,
    frame::cursor::kernel::RegionExit,
    module::{LoadedModule, VerifiedProgram, execution::layout::ReusedSlot},
    value::Value,
};
use kagari_abi::representation::ValueType;
use kagari_bytecode::{
    artifact::KbcArtifact,
    instruction::{BytecodeInstruction as I, ConstantId, ConstantOperand, JumpTarget, Slot},
    module::{
        BytecodeDebugMetadata, BytecodeFunction, BytecodeModule, FunctionMetadata, FunctionRecord,
        InstructionOrigin, LocalLiveRange,
    },
    program::{BytecodeProgram, ModuleRef},
};
use kagari_common::{identity::map::DefinitionContext, span::Span};
use kagari_contract::ids::FunctionRef;
use std::mem::size_of;

fn reused_bindings(runtime: &mut Runtime) -> LoadedModule {
    let context = DefinitionContext::new().unwrap();
    let range = |slot, start, end| LocalLiveRange {
        slot: Slot::new(slot),
        binding: slot as u32,
        name: format!("binding{slot}"),
        span: Span::default(),
        start,
        end,
        ty: ValueType::I32,
        is_parameter: false,
    };
    let function = BytecodeFunction {
        name: "main".into(),
        slot_count: 2,
        metadata: FunctionMetadata {
            slots: vec![ValueType::I32; 2],
            return_type: ValueType::I32,
            origins: (0..4)
                .map(|instruction| InstructionOrigin {
                    instruction,
                    span: Span::default(),
                    line: None,
                    column: None,
                })
                .collect(),
            debug: BytecodeDebugMetadata {
                local_live_ranges: vec![range(0, 1, 2), range(1, 3, 4)],
                ..Default::default()
            },
            ..Default::default()
        },
        instructions: vec![
            I::LoadConst {
                dst: Slot::new(0),
                constant: ConstantId::new(0),
            },
            I::Jump {
                target: JumpTarget::new(2),
            },
            I::LoadConst {
                dst: Slot::new(1),
                constant: ConstantId::new(1),
            },
            I::Return(Some(Slot::new(1))),
        ],
        ..Default::default()
    };
    let artifact = KbcArtifact::from_program(
        BytecodeProgram {
            root: ModuleRef::new(0),
            modules: vec![BytecodeModule {
                types: vec![ValueType::Unit, ValueType::I32],
                constants: vec![ConstantOperand::I32(7), ConstantOperand::I32(9)],
                function_table: vec![FunctionRecord {
                    id: function.id,
                    identity: None,
                    name: function.name.clone(),
                    params: vec![],
                    return_type: ValueType::I32,
                    effects: Default::default(),
                }],
                functions: vec![function],
                ..Default::default()
            }],
            exports: Default::default(),
        },
        Default::default(),
        &context,
    )
    .unwrap();
    let bytes = artifact.to_bytes(&context.snapshot()).unwrap();
    let artifact = KbcArtifact::from_bytes(&bytes)
        .unwrap()
        .import_into(&context, &Default::default())
        .unwrap()
        .into_records();
    let verified = artifact
        .into_verified(&Default::default(), &context)
        .unwrap();
    runtime
        .load_verified_program(
            "reused-bindings",
            VerifiedProgram::from_bytecode(verified.into_bytecode()),
        )
        .unwrap()
}

#[test]
fn public_frame_access_preserves_reused_binding_lifetimes_and_initialized_entry() {
    let mut runtime = Runtime::default();
    let loaded = reused_bindings(&mut runtime);
    let stack = runtime.enter_execution_stack(&loaded).unwrap();
    stack
        .push(&runtime, loaded.slot(), FunctionRef::new(0), &[], None)
        .unwrap();
    {
        let frame = stack.current().unwrap();
        assert_eq!(
            frame.storage_counts(&runtime).unwrap(),
            (1, 0),
            "keep actual scalar reuse; do not pin both homes or allocate a mirror"
        );
        assert_eq!(frame.physical_slot_count(), 1);
        let guards = &frame.slot_layout.as_ref().unwrap().reused_slots;
        assert_eq!(guards.len(), 2);
        println!(
            "REUSED_FRAME_ACCESS,scalar_slots=1,managed_slots=0,shared_guard_bytes={}",
            guards.len() * size_of::<ReusedSlot>()
        );
    }
    assert!(matches!(
        stack.execute_region(&runtime, &mut Some(1), |_, _, _, _| unreachable!()),
        Ok(RegionExit::Slice)
    ));
    {
        let mut frame = stack.current_mut().unwrap();
        assert_eq!(
            frame.read_slot(&runtime, Slot::new(0)).unwrap(),
            Value::I32(7),
            "a successful slice publishes the first definition before PC preparation"
        );
        frame.prepare_instruction();
        assert_eq!(
            frame.read_slot(&runtime, Slot::new(0)).unwrap(),
            Value::I32(7)
        );
        for error in [
            frame
                .write_slot(&runtime, Slot::new(1), Value::I32(99))
                .unwrap_err(),
            frame.read_slot(&runtime, Slot::new(1)).unwrap_err(),
            frame.jump_to(&runtime, 3).unwrap_err(),
        ] {
            assert_eq!(error.kind(), RuntimeErrorKind::ModuleValidation);
        }
        assert_eq!(frame.instruction_offset(), 1);
        assert_eq!(
            frame.read_slot(&runtime, Slot::new(0)).unwrap(),
            Value::I32(7)
        );
        assert!(!runtime.is_quarantined());
        assert!(matches!(frame.next_instruction(), Some(I::Jump { .. })));
        frame.jump_to(&runtime, 2).unwrap();
        frame.prepare_instruction();
        assert!(
            frame.read_slot(&runtime, Slot::new(1)).is_err(),
            "an old alias is not an initialized new binding"
        );
        assert!(
            frame.jump_to(&runtime, 3).is_err(),
            "stopping at the defining PC does not prove that its write completed"
        );
    }
    assert!(matches!(
        stack.execute_region(&runtime, &mut Some(1), |_, _, _, _| unreachable!()),
        Ok(RegionExit::Slice)
    ));
    {
        let mut frame = stack.current_mut().unwrap();
        assert_eq!(
            frame.read_slot(&runtime, Slot::new(1)).unwrap(),
            Value::I32(9),
            "the completed replacement is visible at its defining PC"
        );
        frame.prepare_instruction();
        assert_eq!(
            frame.read_slot(&runtime, Slot::new(1)).unwrap(),
            Value::I32(9)
        );
        assert!(
            frame
                .write_slot(&runtime, Slot::new(0), Value::I32(99))
                .is_err()
        );
        frame
            .write_slot(&runtime, Slot::new(1), Value::I32(11))
            .unwrap();
        assert_eq!(
            frame.read_slot(&runtime, Slot::new(1)).unwrap(),
            Value::I32(11)
        );
    }
    drop(stack);
    assert_eq!(runtime.gc().active_roots(), 0);
    assert_eq!(runtime.resources().counters().current_call_depth, 0);
}
