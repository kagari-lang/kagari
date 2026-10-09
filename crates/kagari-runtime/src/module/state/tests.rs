use super::*;
use crate::error::RuntimeErrorKind;
use kagari_abi::representation::ValueType;
use kagari_bytecode::{
    instruction::{BytecodeInstruction, Register},
    module::{BytecodeFunction, BytecodeModule, FunctionMetadata, FunctionRecord, RootSlotLayout},
    program::{BytecodeProgram, ModuleRef},
};
use kagari_contract::ids::FunctionRef;
use kagari_types::{scalar::BuiltinType, ty::Ty};

fn program(ty: ValueType, mutable: bool) -> BytecodeProgram {
    let function = BytecodeFunction {
        id: FunctionRef::new(0),
        name: "main".into(),
        register_count: 1,
        metadata: FunctionMetadata {
            registers: vec![ValueType::HeapObject],
            roots: RootSlotLayout::from_types(&[], &[ValueType::HeapObject]),
            ..Default::default()
        },
        instructions: vec![BytecodeInstruction::Return(None)],
        ..Default::default()
    };
    BytecodeProgram {
        root: ModuleRef::new(0),
        modules: vec![BytecodeModule {
            types: vec![ValueType::Unit, ValueType::HeapObject, ty],
            module_slots: vec![BytecodeModuleSlot {
                name: "state".into(),
                ty,
                mutable,
            }],
            function_table: vec![FunctionRecord {
                id: function.id,
                identity: None,
                name: function.name.clone(),
                params: vec![],
                return_type: ValueType::Unit,
                effects: Default::default(),
            }],
            functions: vec![function],
            ..Default::default()
        }],
    }
}

fn array(runtime: &Runtime, owner: &LoadedModule, value: i32) -> Value {
    Value::Array(
        runtime
            .alloc_array(
                owner,
                Ty::Builtin(BuiltinType::I32),
                vec![Value::I32(value)],
            )
            .unwrap(),
    )
}

#[test]
fn checked_writes_preserve_slots_on_type_access_bounds_and_owner_errors() {
    let mut runtime = Runtime::default();
    let module = runtime
        .load_program("state", program(ValueType::I32, true))
        .unwrap();
    let readonly = runtime
        .load_program("readonly", program(ValueType::Unit, false))
        .unwrap();
    let slot = ModuleSlot::new(0);
    runtime
        .write_module_slot(&module, slot, Value::I32(7))
        .unwrap();
    assert_eq!(
        runtime
            .write_module_slot(&module, slot, Value::Bool(true))
            .unwrap_err()
            .kind(),
        RuntimeErrorKind::ModuleValidation
    );
    assert_eq!(
        runtime
            .write_module_slot(&module, ModuleSlot::new(1), Value::I32(9))
            .unwrap_err()
            .kind(),
        RuntimeErrorKind::ModuleValidation
    );
    assert_eq!(
        runtime
            .read_module_slot(&module, ModuleSlot::new(1))
            .unwrap_err()
            .kind(),
        RuntimeErrorKind::ModuleValidation
    );
    assert_eq!(
        runtime
            .write_module_slot(&readonly, slot, Value::Unit)
            .unwrap_err()
            .kind(),
        RuntimeErrorKind::ModuleValidation
    );
    let foreign = Runtime::default();
    assert!(
        foreign
            .write_module_slot(&module, slot, Value::I32(9))
            .is_err()
    );
    assert!(foreign.read_module_slot(&module, slot).is_err());
    assert_eq!(
        runtime.read_module_slot(&module, slot).unwrap(),
        Value::I32(7)
    );
    assert_eq!(
        runtime.read_module_slot(&readonly, slot).unwrap(),
        Value::Unit
    );
    let mut snapshot = runtime.module_instance_snapshot(&module).unwrap();
    snapshot.module_slots.clear();
    assert_eq!(
        runtime.read_module_slot(&module, slot).unwrap(),
        Value::I32(7)
    );
    assert!(!runtime.is_quarantined());
    assert!(!foreign.is_quarantined());
}

#[test]
fn replacement_and_removal_update_gc_edges_and_reject_foreign_or_stale_values() {
    let mut runtime = Runtime::default();
    let module = runtime
        .load_program("state", program(ValueType::HeapObject, true))
        .unwrap();
    let slot = ModuleSlot::new(0);
    let first = array(&runtime, &module, 7);
    runtime.write_module_slot(&module, slot, first).unwrap();
    runtime.collect_garbage().unwrap();
    assert!(runtime.gc().validate_value(&first));
    let mut other = Runtime::default();
    let other_module = other
        .load_program("state", program(ValueType::HeapObject, true))
        .unwrap();
    let foreign = other
        .gc()
        .alloc_tuple(vec![array(&other, &other_module, 9)])
        .unwrap();
    assert_eq!(
        runtime
            .write_module_slot(&module, slot, foreign)
            .unwrap_err()
            .kind(),
        RuntimeErrorKind::ScriptTrap
    );
    assert_eq!(runtime.read_module_slot(&module, slot).unwrap(), first);
    let second = array(&runtime, &module, 42);
    runtime.write_module_slot(&module, slot, second).unwrap();
    runtime.collect_garbage().unwrap();
    assert!(!runtime.gc().validate_value(&first));
    assert!(runtime.gc().validate_value(&second));
    assert_eq!(
        runtime
            .write_module_slot(&module, slot, first)
            .unwrap_err()
            .kind(),
        RuntimeErrorKind::ScriptTrap
    );
    assert_eq!(runtime.read_module_slot(&module, slot).unwrap(), second);
    runtime
        .write_module_slot(&module, slot, runtime.gc().alloc_tuple(vec![]).unwrap())
        .unwrap();
    runtime.collect_garbage().unwrap();
    assert!(!runtime.gc().validate_value(&second));
    // The installed slot retains the immutable empty tuple itself.
    assert_eq!(runtime.gc().allocated_objects(), 1);
    assert!(!runtime.is_quarantined());
}

#[test]
fn candidate_slot_writes_validate_objects_before_and_after_initialization() {
    let mut runtime = Runtime::default();
    let code = program(ValueType::HeapObject, true);
    let baseline = runtime.load_program("state", code.clone()).unwrap();
    let external = array(&runtime, &baseline, 7);
    let candidate = runtime
        .stage_reload_program(&baseline, "state", code)
        .unwrap();
    let session = runtime.begin_candidate_initialization(&candidate).unwrap();
    let slot = ModuleSlot::new(0);
    let local = array(&runtime, candidate.module(), 42);
    runtime
        .write_module_slot(candidate.module(), slot, local)
        .unwrap();
    for target in [&baseline, candidate.module()] {
        assert_eq!(
            runtime
                .write_module_slot(target, slot, external)
                .unwrap_err()
                .kind(),
            RuntimeErrorKind::ExecutionPhaseViolation
        );
    }
    assert_eq!(
        runtime.read_module_slot(candidate.module(), slot).unwrap(),
        local
    );
    drop(session);
    assert_eq!(
        runtime
            .write_module_slot(candidate.module(), slot, external)
            .unwrap_err()
            .kind(),
        RuntimeErrorKind::ExecutionPhaseViolation
    );
    let current = runtime.publish_staged_reload(candidate).unwrap();
    runtime.collect_garbage().unwrap();
    assert_eq!(runtime.read_module_slot(&current, slot).unwrap(), local);
    assert!(!runtime.is_quarantined());
}

#[test]
fn corrupt_module_storage_quarantines_and_cleans_frame_resources() {
    // Fault injection stays inside the runtime; public callers cannot resize slots.
    for missing in [false, true] {
        let mut runtime = Runtime::default();
        let module = runtime
            .load_program("state", program(ValueType::I32, true))
            .unwrap();
        let slot = ModuleSlot::new(0);
        runtime
            .write_module_slot(&module, slot, Value::I32(7))
            .unwrap();
        assert_eq!(
            runtime.read_module_slot(&module, slot).unwrap(),
            Value::I32(7)
        );
        let stack = runtime.enter_execution_stack(&module).unwrap();
        stack
            .push(&runtime, module.slot(), FunctionRef::new(0), &[], None)
            .unwrap();
        stack
            .current_mut()
            .unwrap()
            .write_register(&runtime, Register::new(0), array(&runtime, &module, 7))
            .unwrap();
        assert_eq!(runtime.resources().counters().current_call_depth, 1);
        assert!(runtime.gc().active_roots() > 0);
        {
            let mut instance = runtime.modules.instance_mut(module.key()).unwrap();
            if missing {
                instance.module_slots.clear();
            } else {
                instance.module_slots[0] = Value::Bool(true);
            }
        }
        assert_eq!(
            runtime.read_module_slot(&module, slot).unwrap_err().kind(),
            RuntimeErrorKind::EngineFault
        );
        assert!(runtime.is_quarantined());
        drop(stack);
        assert!(runtime.execution_root().is_none());
        assert_eq!(runtime.resources().counters().current_call_depth, 0);
        assert_eq!(runtime.gc().active_roots(), 0);
        assert_eq!(
            runtime.modules.retention_counts(module.key()).active_calls,
            0
        );
    }
}

#[test]
fn borrowed_storage_rejects_slot_writes_before_changing_edges() {
    let mut runtime = Runtime::default();
    let module = runtime
        .load_program("state", program(ValueType::I32, true))
        .unwrap();
    let slot = ModuleSlot::new(0);
    runtime
        .write_module_slot(&module, slot, Value::I32(7))
        .unwrap();
    let held = runtime.modules.instance_mut(module.key()).unwrap();
    assert_eq!(
        runtime
            .write_module_slot(&module, slot, Value::I32(9))
            .unwrap_err()
            .kind(),
        RuntimeErrorKind::EngineFault
    );
    assert_eq!(held.module_slots, vec![Value::I32(7)]);
    assert!(runtime.is_quarantined());
}
