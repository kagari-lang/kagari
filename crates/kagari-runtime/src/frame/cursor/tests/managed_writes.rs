//! Managed constants and copies keep the checked publication boundary in closed regions.
use crate::{
    Runtime,
    error::{RuntimeError, RuntimeErrorKind},
    frame::cursor::kernel::{RegionError, RegionExit},
    module::ExecutableModule,
    native::context::ScriptInvoker,
    test_support::native::program::ProgramFixture,
};
use kagari_abi::representation::ValueType;
use kagari_bytecode::{
    instruction::{BytecodeInstruction as I, ConstantId, ConstantOperand, Slot},
    module::{
        BytecodeFunction, BytecodeModule, FunctionMetadata, FunctionRecord, InstructionOrigin,
        RootSlotLayout,
    },
    program::ModuleRef,
};
use kagari_common::identity::map::DefinitionContext;
use kagari_contract::ids::FunctionRef;

fn managed_copies(runtime: &mut Runtime) -> ExecutableModule {
    let context = DefinitionContext::new().unwrap();
    let instructions = vec![
        I::LoadConst {
            dst: Slot::new(0),
            constant: ConstantId::new(0),
        },
        I::Move {
            dst: Slot::new(1),
            src: Slot::new(0),
        },
        I::Move {
            dst: Slot::new(0),
            src: Slot::new(1),
        },
        I::Move {
            dst: Slot::new(1),
            src: Slot::new(0),
        },
        I::Return(Some(Slot::new(1))),
    ];
    let function = BytecodeFunction {
        name: "copies".into(),
        slot_count: 2,
        metadata: FunctionMetadata {
            slots: vec![ValueType::Str; 2],
            roots: RootSlotLayout::from_types(&[ValueType::Str; 2]),
            return_type: ValueType::Str,
            origins: (0..instructions.len())
                .map(|instruction| InstructionOrigin {
                    instruction,
                    span: Default::default(),
                    line: None,
                    column: None,
                })
                .collect(),
            ..Default::default()
        },
        instructions,
        ..Default::default()
    };
    let program = runtime
        .load_program(
            "managed-copies",
            ProgramFixture {
                root: ModuleRef::new(0),
                exports: Default::default(),
                modules: vec![BytecodeModule {
                    types: vec![ValueType::Unit, ValueType::Str],
                    constants: vec![ConstantOperand::Str("retained".into())],
                    function_table: vec![FunctionRecord {
                        id: function.id,
                        identity: None,
                        name: function.name.clone(),
                        params: vec![],
                        return_type: ValueType::Str,
                        effects: Default::default(),
                    }],
                    functions: vec![function],
                    ..Default::default()
                }],
            }
            .metadata(context.snapshot(), &Default::default())
            .unwrap(),
        )
        .unwrap();
    runtime.select_program(&program).unwrap()
}

#[test]
fn closed_write_regions_revalidate_after_cancellation_or_graph_fault() {
    for cancelled in [false, true] {
        let mut runtime = Runtime::default();
        let owner = managed_copies(&mut runtime);
        let stack = runtime.enter_execution_stack(&owner).unwrap();
        stack
            .push(&runtime, owner.slot(), FunctionRef::new(0), &[], None)
            .unwrap();
        let links = stack.current().unwrap().links.clone().unwrap();
        links
            .constants
            .materialize(&runtime, &owner, ConstantId::new(0))
            .unwrap();
        assert!(matches!(
            stack
                .execute_region(&runtime, &mut Some(1), unsafe {
                    // SAFETY: The callback rejects execution or returns only a scalar; it retains no raw edges.
                    ScriptInvoker::new(|_, _, _, _| unreachable!())
                })
                .unwrap(),
            RegionExit::Slice,
        ));
        let slots = stack.current().unwrap().slots;
        let before = runtime
            .resources()
            .frame_values
            .borrow()
            .managed_values(slots)
            .unwrap()
            .to_vec();
        let expected = if cancelled {
            runtime.execution_options().cancellation.cancel();
            let error = runtime.resources().poll_execution().unwrap_err();
            assert_eq!(error.kind(), RuntimeErrorKind::Cancelled);
            error
        } else {
            runtime.gc.graph.lock().fault();
            // Execution permission remains valid; only publication must reject.
            assert!(runtime.gc.ensure_execution_allowed().is_ok());
            RuntimeError::new(
                RuntimeErrorKind::EngineFault,
                "invalid execution operand slot",
            )
        };
        let RegionError::Runtime(error) = stack
            .execute_region(&runtime, &mut None, unsafe {
                // SAFETY: The callback rejects execution or returns only a scalar; it retains no raw edges.
                ScriptInvoker::new(|_, _, _, _| unreachable!("no callback on failed publication"))
            })
            .err()
            .unwrap()
        else {
            panic!("runtime failure")
        };
        assert_eq!(error.kind(), expected.kind());
        assert_eq!(error.message(), expected.message());
        assert_eq!(
            runtime
                .resources()
                .frame_values
                .borrow()
                .managed_values(slots)
                .unwrap(),
            before
        );
        drop(stack);
        assert_eq!(runtime.resources().counters().current_call_depth, 0);
    }
}

#[test]
fn cached_constant_and_copies_publish_after_initial_roots() {
    let mut runtime = Runtime::default();
    let owner = managed_copies(&mut runtime);
    let stack = runtime.enter_execution_stack(&owner).unwrap();
    stack
        .push(&runtime, owner.slot(), FunctionRef::new(0), &[], None)
        .unwrap();
    let links = stack.current().unwrap().links.clone().unwrap();
    let constant = links
        .constants
        .materialize(&runtime, &owner, ConstantId::new(0))
        .unwrap();
    runtime.gc.graph.lock().begin_cycle().unwrap();
    runtime.gc.graph.lock().finish_roots().unwrap();
    let result = stack
        .execute_region(&runtime, &mut None, unsafe {
            // SAFETY: The callback rejects execution or returns only a scalar; it retains no raw edges.
            ScriptInvoker::new(|_, _, _, _| unreachable!())
        })
        .unwrap();
    let RegionExit::Return(value) = result else {
        panic!("closed managed region returns")
    };
    assert_eq!(value.materialize(), Some(constant));
    runtime.gc.drain_graph_work(&runtime.modules).unwrap();
    runtime.gc.graph.lock().close_mark().unwrap();
    assert!(
        runtime
            .gc
            .graph_survivor(
                &runtime.modules,
                constant.direct_reference().unwrap().graph()
            )
            .unwrap()
    );
    runtime.gc.graph.lock().finish_sweep().unwrap();
    drop(stack);
    assert_eq!(runtime.resources().counters().current_call_depth, 0);
}
