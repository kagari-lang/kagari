mod compatibility;
mod contracts;
mod initialization;

use super::*;
use crate::{
    error::RuntimeErrorKind,
    module::{VerifiedProgram, state::cells::CellId},
};
use kagari_abi::representation::ValueType;
use kagari_bytecode::{
    instruction::{BytecodeInstruction, ConstantId, ConstantOperand, Slot},
    module::{
        BytecodeFunction, BytecodeModule, FunctionMetadata, FunctionRecord, InstructionOrigin,
        RootSlotLayout,
    },
    program::{BytecodeProgram, ModuleRef},
};
use kagari_common::identity::{
    DefinitionKind, map::DefinitionContext, metadata::DefinitionMetadata,
};
use kagari_contract::{
    representation::semantic_representation,
    statics::{StaticContract, StaticDescriptor},
    types::PublicItem,
};
use kagari_types::visibility::Visibility;
use kagari_types::{declaration::ConstDef, scalar::BuiltinType, ty::Ty};

pub(crate) fn add_static(
    context: &DefinitionContext,
    module: &mut BytecodeModule,
    name: &str,
    ty: Ty,
    mutable: bool,
    visibility: Visibility,
) {
    let root = context.intern_root(&module.identity).unwrap();
    let declaration = context
        .intern_child(root, DefinitionKind::Static, name, 0)
        .unwrap();
    let contract = StaticContract {
        declaration,
        supplier: module.identity.clone(),
        name: name.into(),
        ty: ty.clone(),
        mutable,
        visibility,
    };
    let id = FunctionRef::new(module.functions.len());
    let mut metadata = FunctionMetadata {
        return_type: semantic_representation(&ty),
        ..Default::default()
    };
    metadata.semantic.result = Some(ty);
    metadata.origins = vec![InstructionOrigin {
        instruction: 0,
        span: Default::default(),
        line: None,
        column: None,
    }];
    let function = BytecodeFunction {
        id,
        name: format!("initialize_{name}"),
        metadata,
        instructions: vec![BytecodeInstruction::Unreachable],
        ..Default::default()
    };
    module.types.push(function.metadata.return_type);
    module.function_table.push(FunctionRecord {
        id,
        identity: None,
        name: function.name.clone(),
        params: vec![],
        return_type: function.metadata.return_type,
        effects: function.metadata.effects,
    });
    module.functions.push(function);
    if visibility == Visibility::Public {
        module
            .public_items
            .push(PublicItem::Static(contract.clone()));
    }
    module.module_slots.push(StaticDescriptor {
        contract,
        initializer: id,
    });
}

pub(crate) fn program(ty: Ty, mutable: bool) -> DefinitionMetadata<BytecodeProgram> {
    let context = DefinitionContext::new().unwrap();
    let mut module = BytecodeModule {
        types: vec![ValueType::Unit, ValueType::HeapObject],
        ..Default::default()
    };
    let mut metadata = FunctionMetadata {
        slots: vec![ValueType::HeapObject],
        roots: RootSlotLayout::from_types(&[ValueType::HeapObject]),
        origins: vec![InstructionOrigin {
            instruction: 0,
            span: Default::default(),
            line: None,
            column: None,
        }],
        ..Default::default()
    };
    metadata.semantic.result = Some(Ty::Builtin(BuiltinType::Unit));
    module.functions.push(BytecodeFunction {
        id: FunctionRef::new(0),
        name: "main".into(),
        slot_count: 1,
        metadata,
        instructions: vec![BytecodeInstruction::Return(None)],
        ..Default::default()
    });
    module.function_table.push(FunctionRecord {
        id: FunctionRef::new(0),
        identity: None,
        name: "main".into(),
        params: vec![],
        return_type: ValueType::Unit,
        effects: Default::default(),
    });
    add_static(
        &context,
        &mut module,
        "state",
        ty,
        mutable,
        Visibility::Private,
    );
    DefinitionMetadata::checked(
        context.snapshot(),
        BytecodeProgram {
            exports: Default::default(),
            root: ModuleRef::new(0),
            modules: vec![module],
        },
        &Default::default(),
    )
    .unwrap()
}

fn scalar() -> Ty {
    Ty::Builtin(BuiltinType::I32)
}

fn array_type() -> Ty {
    Ty::Array(Box::new(scalar()))
}

fn cell(store: &ModuleStore, module: &LoadedModule, slot: usize) -> CellId {
    store.inner.borrow().resolve(module).unwrap().bindings[slot]
}

/// Seed only inside this internal test owner. There is no public initialization bypass.
pub(crate) fn ready(runtime: &Runtime, module: &LoadedModule, slot: usize, value: Value) {
    runtime
        .validate_heap_payloads(slice::from_ref(&value))
        .unwrap();
    assert!(runtime.matches_type_in(
        &value,
        &module.bytecode.module_slots[slot].contract.ty,
        module,
        None
    ));
    let id = cell(&runtime.modules, module, slot);
    runtime
        .modules
        .inner
        .borrow_mut()
        .cells
        .get_mut(&id)
        .unwrap()
        .state = StaticState::Ready(value);
}

fn array(runtime: &Runtime, owner: &LoadedModule, value: i32) -> Value {
    Value::Array(
        runtime
            .alloc_array(owner, scalar(), vec![Value::I32(value)])
            .unwrap(),
    )
}

#[test]
fn checked_writes_preserve_cells_on_semantic_access_bounds_and_owner_errors() {
    let mut runtime = Runtime::default();
    let module = runtime
        .load_program("state", program(scalar(), true))
        .unwrap();
    let readonly = runtime
        .load_program("readonly", program(Ty::Builtin(BuiltinType::Unit), false))
        .unwrap();
    let slot = ModuleSlot::new(0);
    assert!(matches!(
        runtime
            .module_instance_snapshot(&readonly)
            .unwrap()
            .module_slots[0],
        StaticState::Uninitialized(_)
    ));
    assert!(runtime.read_module_slot(&readonly, slot).is_err());
    assert!(
        runtime
            .write_module_slot(&readonly, slot, Value::Unit)
            .is_err()
    );
    assert!(
        runtime
            .write_module_slot(&module, slot, Value::I32(7))
            .is_err()
    );
    ready(&runtime, &module, 0, Value::I32(7));
    ready(&runtime, &readonly, 0, Value::Unit);
    for (target, index, value) in [
        (&module, slot, Value::Bool(true)),
        (&module, ModuleSlot::new(1), Value::I32(9)),
        (&readonly, slot, Value::Unit),
    ] {
        assert_eq!(
            runtime
                .write_module_slot(target, index, value)
                .unwrap_err()
                .kind(),
            RuntimeErrorKind::ModuleValidation
        );
    }
    assert!(
        runtime
            .read_module_slot(&module, ModuleSlot::new(1))
            .is_err()
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
    runtime
        .module_instance_snapshot(&module)
        .unwrap()
        .module_slots
        .clear();
    assert_eq!(
        runtime.read_module_slot(&module, slot).unwrap(),
        Value::I32(7)
    );
    assert!(!runtime.is_quarantined());
}

#[test]
fn replacement_updates_gc_edges_and_rejects_foreign_stale_and_wrong_semantic_values() {
    let mut runtime = Runtime::default();
    let module = runtime
        .load_program("state", program(array_type(), true))
        .unwrap();
    let slot = ModuleSlot::new(0);
    let first = array(&runtime, &module, 7);
    ready(&runtime, &module, 0, first);
    runtime.collect_garbage().unwrap();
    assert!(runtime.gc().validate_value(&first));
    let mut other = Runtime::default();
    let other_module = other
        .load_program("state", program(array_type(), true))
        .unwrap();
    let foreign = array(&other, &other_module, 9);
    assert_eq!(
        runtime
            .write_module_slot(&module, slot, foreign)
            .unwrap_err()
            .kind(),
        RuntimeErrorKind::ScriptTrap
    );
    let wrong = Value::Array(
        runtime
            .alloc_array(&module, Ty::Builtin(BuiltinType::Bool), vec![])
            .unwrap(),
    );
    assert_eq!(
        runtime
            .write_module_slot(&module, slot, wrong)
            .unwrap_err()
            .kind(),
        RuntimeErrorKind::ModuleValidation
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
    assert!(!runtime.is_quarantined());
}

#[test]
fn reload_shares_cells_preserves_recipe_and_isolates_candidate_and_new_installations() {
    let mut runtime = Runtime::default();
    let code = program(scalar(), true);
    let baseline = runtime.load_program("state", code.clone()).unwrap();
    let independent = runtime.load_program("independent", code.clone()).unwrap();
    let other_runtime = &mut Runtime::default();
    let foreign = other_runtime.load_program("state", code.clone()).unwrap();
    let (definitions, mut updated) = code.into_parts();
    let module = &mut updated.modules[0];
    let mut initializer = module.functions[1].clone();
    initializer.id = FunctionRef::new(2);
    initializer.name = "replacement_initializer".into();
    initializer.slot_count = 1;
    initializer.metadata.slots = vec![ValueType::I32];
    initializer.metadata.semantic.slots.insert(0, scalar());
    initializer.instructions = vec![
        BytecodeInstruction::LoadConst {
            dst: Slot::new(0),
            constant: ConstantId::new(0),
        },
        BytecodeInstruction::Return(Some(Slot::new(0))),
    ];
    initializer.metadata.origins.push(InstructionOrigin {
        instruction: 1,
        span: Default::default(),
        line: None,
        column: None,
    });
    let mut record = module.function_table[1].clone();
    record.id = initializer.id;
    record.name = initializer.name.clone();
    module.module_slots[0].initializer = initializer.id;
    module.functions.push(initializer);
    module.function_table.push(record);
    module.constants.push(ConstantOperand::I32(42));
    let code = DefinitionMetadata::checked(definitions, updated, &Default::default()).unwrap();
    let candidate = runtime
        .stage_reload_program(&baseline, "state", code.clone())
        .unwrap();
    assert_eq!(
        cell(&runtime.modules, &baseline, 0),
        cell(&runtime.modules, candidate.module(), 0)
    );
    assert_ne!(
        cell(&runtime.modules, &baseline, 0),
        cell(&runtime.modules, &independent, 0)
    );
    let session = runtime.begin_candidate_initialization(&candidate).unwrap();
    for target in [&baseline, candidate.module()] {
        assert_eq!(
            runtime
                .read_module_slot(target, ModuleSlot::new(0))
                .unwrap_err()
                .kind(),
            RuntimeErrorKind::ExecutionPhaseViolation
        );
        assert_eq!(
            runtime
                .write_module_slot(target, ModuleSlot::new(0), Value::I32(9))
                .unwrap_err()
                .kind(),
            RuntimeErrorKind::ExecutionPhaseViolation
        );
    }
    drop(session);
    let current = runtime.publish_staged_reload(candidate).unwrap();
    let StaticState::Uninitialized(recipe) = runtime
        .module_instance_snapshot(&current)
        .unwrap()
        .module_slots
        .remove(0)
    else {
        panic!("uninitialized")
    };
    assert_eq!(recipe.module.key(), baseline.key());
    assert_eq!(recipe.function, FunctionRef::new(1));
    assert_eq!(
        current.bytecode.module_slots[0].initializer,
        FunctionRef::new(2)
    );
    assert!(
        !runtime
            .collect_garbage()
            .unwrap()
            .reclaimed_modules
            .contains(&baseline.key())
    );
    ready(&runtime, &current, 0, Value::I32(100));
    runtime
        .write_module_slot(&baseline, ModuleSlot::new(0), Value::I32(101))
        .unwrap();
    assert_eq!(
        runtime
            .read_module_slot(&current, ModuleSlot::new(0))
            .unwrap(),
        Value::I32(101)
    );
    assert!(
        runtime
            .read_module_slot(&independent, ModuleSlot::new(0))
            .is_err()
    );
    assert!(
        other_runtime
            .read_module_slot(&foreign, ModuleSlot::new(0))
            .is_err()
    );
    assert!(
        runtime
            .collect_garbage()
            .unwrap()
            .reclaimed_modules
            .contains(&baseline.key())
    );
    let fresh = runtime.load_program("state", code).unwrap();
    let StaticState::Uninitialized(fresh_recipe) = runtime
        .module_instance_snapshot(&fresh)
        .unwrap()
        .module_slots
        .remove(0)
    else {
        panic!("fresh recipe")
    };
    assert_eq!(fresh_recipe.module.key(), fresh.key());
    assert_eq!(fresh_recipe.function, FunctionRef::new(2));
    assert!(
        runtime
            .read_module_slot(&fresh, ModuleSlot::new(0))
            .is_err()
    );
    assert_ne!(
        cell(&runtime.modules, &current, 0),
        cell(&runtime.modules, &fresh, 0)
    );
}

#[test]
fn additions_reordering_and_stale_publication_are_transactional_and_deletion_is_rejected() {
    let mut runtime = Runtime::default();
    let original = program(scalar(), true);
    let baseline = runtime.load_program("state", original.clone()).unwrap();
    ready(&runtime, &baseline, 0, Value::I32(7));
    let context = DefinitionContext::new().unwrap();
    let mut updated = original
        .import_into(&context, &Default::default())
        .unwrap()
        .into_records();
    add_static(
        &context,
        &mut updated.modules[0],
        "added",
        scalar(),
        true,
        Visibility::Public,
    );
    updated.modules[0].module_slots.swap(0, 1);
    let updated =
        DefinitionMetadata::checked(context.snapshot(), updated, &Default::default()).unwrap();
    let abandoned = runtime
        .stage_reload_program(&baseline, "state", updated.clone())
        .unwrap();
    let stale = runtime
        .stage_reload_program(&baseline, "state", updated.clone())
        .unwrap();
    let candidate = runtime
        .stage_reload_program(&baseline, "state", updated)
        .unwrap();
    let abandoned_key = abandoned.module().key();
    drop(abandoned);
    let current = runtime.publish_staged_reload(candidate).unwrap();
    assert!(runtime.publish_staged_reload(stale).is_err());
    assert_eq!(
        runtime
            .read_module_slot(&current, ModuleSlot::new(1))
            .unwrap(),
        Value::I32(7)
    );
    assert!(
        runtime
            .read_module_slot(&current, ModuleSlot::new(0))
            .is_err()
    );
    assert!(
        runtime
            .stage_reload_program(&current, "state", original)
            .is_err()
    );
    assert!(
        runtime
            .collect_garbage()
            .unwrap()
            .reclaimed_modules
            .contains(&abandoned_key)
    );
    assert_eq!(runtime.modules.inner.borrow().cells.len(), 2);
}

#[test]
fn private_contract_changes_and_nonstatic_public_additions_remain_rejected() {
    for mutation in 0..5 {
        let mut runtime = Runtime::default();
        let code = program(scalar(), true);
        let baseline = runtime.load_program("state", code.clone()).unwrap();
        ready(&runtime, &baseline, 0, Value::I32(7));
        let (definitions, mut candidate) = code.into_parts();
        match mutation {
            0 => candidate.modules[0].module_slots.clear(),
            1 => candidate.modules[0].module_slots[0].contract.mutable = false,
            2 => candidate.modules[0].module_slots[0].contract.visibility = Visibility::PublicCrate,
            3 => {
                candidate.modules[0].module_slots[0].contract.ty = Ty::Builtin(BuiltinType::I64);
                candidate.modules[0].functions[1].metadata.semantic.result =
                    Some(Ty::Builtin(BuiltinType::I64));
                candidate.modules[0].functions[1].metadata.return_type = ValueType::I64;
                candidate.modules[0].function_table[1].return_type = ValueType::I64;
                candidate.modules[0].types.push(ValueType::I64);
            }
            _ => {
                let context = DefinitionContext::new().unwrap();
                let mut candidate =
                    DefinitionMetadata::checked(definitions, candidate, &Default::default())
                        .unwrap()
                        .import_into(&context, &Default::default())
                        .unwrap()
                        .into_records();
                let root = context.intern_root(&candidate.modules[0].identity).unwrap();
                context
                    .intern_child(root, DefinitionKind::Const, "added_constant", 0)
                    .unwrap();
                candidate.modules[0]
                    .public_items
                    .push(PublicItem::Const(ConstDef {
                        name: "added_constant".into(),
                        ty: scalar(),
                        value: "7".into(),
                    }));
                // This is a valid constant addition, not malformed executable metadata.
                VerifiedProgram::new(
                    DefinitionMetadata::checked(
                        context.snapshot(),
                        candidate.clone(),
                        &Default::default(),
                    )
                    .unwrap(),
                    &context,
                )
                .unwrap();
                assert!(
                    runtime
                        .stage_reload_program(
                            &baseline,
                            "state",
                            DefinitionMetadata::checked(
                                context.snapshot(),
                                candidate,
                                &Default::default()
                            )
                            .unwrap()
                        )
                        .is_err()
                );
                assert_eq!(
                    runtime
                        .read_module_slot(&baseline, ModuleSlot::new(0))
                        .unwrap(),
                    Value::I32(7)
                );
                continue;
            }
        }
        assert!(
            runtime
                .stage_reload_program(
                    &baseline,
                    "state",
                    DefinitionMetadata::checked(definitions, candidate, &Default::default())
                        .unwrap()
                )
                .is_err()
        );
        assert_eq!(
            runtime
                .read_module_slot(&baseline, ModuleSlot::new(0))
                .unwrap(),
            Value::I32(7)
        );
    }
}

#[test]
fn failed_and_initializing_are_not_values_and_failed_state_releases_the_recipe() {
    let diagnostic = StaticFailure::new(&"界".repeat(500), Some(Span::default()));
    assert!(diagnostic.cause().len() <= 1024);
    assert_eq!(diagnostic.origin(), Some(Span::default()));
    let mut runtime = Runtime::default();
    let code = program(Ty::Builtin(BuiltinType::Unit), false);
    let old = runtime.load_program("state", code.clone()).unwrap();
    let candidate = runtime.stage_reload_program(&old, "state", code).unwrap();
    let current = runtime.publish_staged_reload(candidate).unwrap();
    let id = cell(&runtime.modules, &current, 0);
    let StaticState::Uninitialized(recipe) = runtime
        .modules
        .static_state(&current, ModuleSlot::new(0))
        .unwrap()
    else {
        panic!("recipe")
    };
    runtime
        .modules
        .inner
        .borrow_mut()
        .cells
        .get_mut(&id)
        .unwrap()
        .state = StaticState::Initializing(recipe);
    assert!(
        runtime
            .read_module_slot(&current, ModuleSlot::new(0))
            .unwrap_err()
            .to_string()
            .contains("recursive")
    );
    runtime
        .modules
        .inner
        .borrow_mut()
        .cells
        .get_mut(&id)
        .unwrap()
        .state = StaticState::Failed(StaticFailure::new("retained cause", None));
    assert!(
        runtime
            .read_module_slot(&current, ModuleSlot::new(0))
            .unwrap_err()
            .to_string()
            .contains("retained cause")
    );
    assert!(
        runtime
            .collect_garbage()
            .unwrap()
            .reclaimed_modules
            .contains(&old.key())
    );
}

#[test]
fn corrupt_storage_quarantines_and_cleans_frame_resources() {
    for corruption in 0..4 {
        let mut runtime = Runtime::default();
        let module = runtime
            .load_program("state", program(scalar(), true))
            .unwrap();
        ready(&runtime, &module, 0, Value::I32(7));
        let independent = runtime
            .load_program("independent", program(scalar(), true))
            .unwrap();
        ready(&runtime, &independent, 0, Value::I32(9));
        let other_cell = cell(&runtime.modules, &independent, 0);
        let stack = runtime.enter_execution_stack(&module).unwrap();
        stack
            .push(&runtime, module.slot(), FunctionRef::new(0), &[], None)
            .unwrap();
        stack
            .current_mut()
            .unwrap()
            .write_slot(&runtime, Slot::new(0), array(&runtime, &module, 7))
            .unwrap();
        let id = cell(&runtime.modules, &module, 0);
        if corruption == 0 {
            runtime.modules.inner.borrow_mut().cells.remove(&id);
        } else if corruption == 1 {
            runtime
                .modules
                .inner
                .borrow_mut()
                .cells
                .get_mut(&id)
                .unwrap()
                .state = StaticState::Ready(Value::Bool(true));
        } else {
            let mut store = runtime.modules.inner.borrow_mut();
            let record = store.records.get_mut(&module.key()).unwrap();
            if corruption == 2 {
                record.bindings[0] = other_cell;
            } else {
                record.bindings.clear();
            }
        }
        assert_eq!(
            runtime
                .read_module_slot(&module, ModuleSlot::new(0))
                .unwrap_err()
                .kind(),
            RuntimeErrorKind::EngineFault
        );
        drop(stack);
        assert!(runtime.execution_root().is_none());
        assert_eq!(runtime.resources().counters().current_call_depth, 0);
        assert_eq!(runtime.gc().active_roots(), 0);
    }
}

#[test]
fn corrupted_collection_root_quarantines_the_runtime() {
    let mut runtime = Runtime::default();
    let module = runtime
        .load_program("state", program(array_type(), true))
        .unwrap();
    let mut foreign = Runtime::default();
    let owner = foreign
        .load_program("foreign", program(array_type(), true))
        .unwrap();
    let value = array(&foreign, &owner, 7);
    let id = cell(&runtime.modules, &module, 0);
    runtime
        .modules
        .inner
        .borrow_mut()
        .cells
        .get_mut(&id)
        .unwrap()
        .state = StaticState::Ready(value);
    for _ in 0..2 {
        assert_eq!(
            runtime.collect_garbage().unwrap_err().kind(),
            RuntimeErrorKind::EngineFault
        );
    }
    assert!(runtime.is_quarantined());
}

#[test]
fn borrowed_storage_rejects_writes_before_changing_edges() {
    let mut runtime = Runtime::default();
    let module = runtime
        .load_program("state", program(scalar(), true))
        .unwrap();
    ready(&runtime, &module, 0, Value::I32(7));
    let id = cell(&runtime.modules, &module, 0);
    let held = runtime.modules.inner.borrow_mut();
    assert_eq!(
        runtime
            .write_module_slot(&module, ModuleSlot::new(0), Value::I32(9))
            .unwrap_err()
            .kind(),
        RuntimeErrorKind::EngineFault
    );
    assert!(matches!(
        held.cells[&id].state,
        StaticState::Ready(Value::I32(7))
    ));
}
