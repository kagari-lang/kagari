#[cfg(test)]
use crate::test_support::bytecode::load_bytecode;
use crate::test_support::native::program::ProgramFixture;
use rstest::rstest;
mod compatibility;
mod contracts;
mod initialization;

use super::*;
use crate::{
    Runtime,
    closure::ClosureMode,
    error::RuntimeErrorKind,
    module::{
        ModuleEpochRetention, ModuleStore, VerifiedProgram,
        managed::{cell::StaticCellObject, source::RevisionSource},
        retention::RetentionOrigin,
        state::{cells::CellId, initialization::InitializationAttempt},
    },
    native::conversion::context::ConversionContext,
    value::carriers::RootedValue,
};
use kagari_abi::representation::ValueType;
use kagari_bytecode::{
    artifact::KbcArtifact,
    instruction::{BytecodeInstruction, ModuleSlot, Slot},
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
use std::{slice, sync::Arc};

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
    ProgramFixture {
        exports: Default::default(),
        root: ModuleRef::new(0),
        modules: vec![module],
    }
    .metadata(context.snapshot(), &Default::default())
    .unwrap()
}

fn scalar() -> Ty {
    Ty::Builtin(BuiltinType::I32)
}

fn array_type() -> Ty {
    Ty::Array(Box::new(scalar()))
}

fn cell(store: &ModuleStore, module: &ExecutableModule, slot: usize) -> CellId {
    store.inner.borrow().resolve(module).unwrap().bindings[slot]
}

/// Seed only inside this internal test owner. There is no public initialization bypass.
pub(crate) fn ready(
    runtime: &mut Runtime,
    module: &ExecutableModule,
    slot: usize,
    value: &RootedValue,
) {
    let ty = runtime
        .type_arguments(
            module,
            None,
            slice::from_ref(&module.bytecode.module_slots[slot].contract.ty),
        )
        .unwrap()
        .remove(0);
    let mut cx = ConversionContext::new(runtime, module).unwrap();
    let input = cx.import(value).unwrap();
    cx.check_value(&ty, &input).unwrap();
    drop(cx);
    seed(runtime, module, slot, value);
}

fn seed(runtime: &mut Runtime, module: &ExecutableModule, slot: usize, value: &RootedValue) {
    let attempt = Arc::new(InitializationAttempt::new(
        runtime.resources.static_failure_signal(),
        None,
    ));
    runtime
        .with_static_origin(
            module,
            RevisionSource::Current,
            ModuleSlot::new(slot),
            |slots, cell| {
                let value = value.get(slots.scope())?;
                StaticCellObject::begin(slots.scope(), cell, attempt.clone())?;
                StaticCellObject::complete(slots.scope(), cell, value, &attempt)?;
                Ok(())
            },
        )
        .unwrap();
}

fn array(runtime: &mut Runtime, owner: &ExecutableModule, value: i32) -> RootedValue {
    let mut cx = ConversionContext::new(runtime, owner).unwrap();
    let element = cx.type_for::<i32>().unwrap();
    let value = cx.encode_value(&element, value).unwrap();
    let array = cx.allocate_array(&element, &[value]).unwrap();
    cx.retain(&array).unwrap()
}

fn read_i32(runtime: &mut Runtime, owner: &ExecutableModule, slot: ModuleSlot) -> i32 {
    let value = runtime.read_module_slot(owner, slot).unwrap();
    ConversionContext::new(runtime, owner)
        .unwrap()
        .decode(&value)
        .unwrap()
}

fn assert_same(runtime: &mut Runtime, left: &RootedValue, right: &RootedValue) {
    runtime
        .memory
        .scope(&runtime.resources, |cx, _| {
            let left = left.get(cx)?;
            let right = right.get(cx)?;
            assert_eq!(left.kind(), right.kind());
            match (left.object(), right.object()) {
                (Some(left), Some(right)) => assert!(left.ptr_eq(right)),
                (None, None) => assert_eq!(left.scalar_payload(), right.scalar_payload()),
                _ => panic!("different value representations"),
            }
            Ok(())
        })
        .unwrap();
}

#[test]
fn checked_writes_preserve_cells_on_semantic_access_bounds_and_owner_errors() {
    let mut runtime = Runtime::default();
    let module_program = load_bytecode(&mut runtime, "state", program(scalar(), true)).unwrap();
    let module = runtime.select_program(&module_program).unwrap();
    let readonly_program = load_bytecode(
        &mut runtime,
        "readonly",
        program(Ty::Builtin(BuiltinType::Unit), false),
    )
    .unwrap();
    let readonly = runtime.select_program(&readonly_program).unwrap();
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
            .write_module_slot(&readonly, slot, &RootedValue::unit())
            .is_err()
    );
    assert!(
        runtime
            .write_module_slot(&module, slot, &RootedValue::from(7))
            .is_err()
    );
    ready(&mut runtime, &module, 0, &RootedValue::from(7));
    ready(&mut runtime, &readonly, 0, &RootedValue::unit());
    for (target, index, value) in [
        (&module, slot, RootedValue::from(true)),
        (&module, ModuleSlot::new(1), RootedValue::from(9)),
        (&readonly, slot, RootedValue::unit()),
    ] {
        assert_eq!(
            runtime
                .write_module_slot(target, index, &value)
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
    let mut foreign = Runtime::default();
    assert!(
        foreign
            .write_module_slot(&module, slot, &RootedValue::from(9))
            .is_err()
    );
    assert!(foreign.read_module_slot(&module, slot).is_err());
    assert_eq!(read_i32(&mut runtime, &module, slot), 7);
    let unit = runtime.read_module_slot(&readonly, slot).unwrap();
    runtime
        .memory
        .scope(&runtime.resources, |cx, _| {
            assert!(unit.get(cx)?.is_unit());
            Ok(())
        })
        .unwrap();
    runtime
        .module_instance_snapshot(&module)
        .unwrap()
        .module_slots
        .clear();
    assert_eq!(read_i32(&mut runtime, &module, slot), 7);
    assert!(!runtime.is_quarantined());
}

#[test]
fn replacement_updates_edges_and_detached_snapshots_keep_the_old_value_alive() {
    let mut runtime = Runtime::default();
    let module_program = load_bytecode(&mut runtime, "state", program(array_type(), true)).unwrap();
    let module = runtime.select_program(&module_program).unwrap();
    let slot = ModuleSlot::new(0);
    let first = array(&mut runtime, &module, 7);
    ready(&mut runtime, &module, 0, &first);
    let snapshot = runtime.module_instance_snapshot(&module).unwrap();
    drop(first);
    runtime.collect_garbage().unwrap();
    let mut other = Runtime::default();
    let other_module_program =
        load_bytecode(&mut other, "state", program(array_type(), true)).unwrap();
    let other_module = other.select_program(&other_module_program).unwrap();
    let foreign = array(&mut other, &other_module, 9);
    assert_eq!(
        runtime
            .write_module_slot(&module, slot, &foreign)
            .unwrap_err()
            .kind(),
        RuntimeErrorKind::ScriptTrap
    );
    let wrong = {
        let mut cx = ConversionContext::new(&mut runtime, &module).unwrap();
        let element = cx.type_for::<bool>().unwrap();
        let array = cx.allocate_array(&element, &[]).unwrap();
        cx.retain(&array).unwrap()
    };
    assert_eq!(
        runtime
            .write_module_slot(&module, slot, &wrong)
            .unwrap_err()
            .kind(),
        RuntimeErrorKind::ModuleValidation
    );
    drop(wrong);
    let StaticState::Ready(first) = &snapshot.module_slots[0] else {
        panic!("ready snapshot")
    };
    let read = runtime.read_module_slot(&module, slot).unwrap();
    assert_same(&mut runtime, &read, first);
    drop(read);
    runtime.collect_garbage().unwrap();
    let second = array(&mut runtime, &module, 42);
    runtime.write_module_slot(&module, slot, &second).unwrap();
    // The detached snapshot is genuine Host ownership, independent of the cell.
    let before = runtime.gc_stats().objects;
    runtime.collect_garbage().unwrap();
    assert_eq!(runtime.gc_stats().objects, before);
    drop(snapshot);
    runtime.collect_garbage().unwrap();
    assert_eq!(runtime.gc_stats().objects, before - 1);
    let read = runtime.read_module_slot(&module, slot).unwrap();
    assert_same(&mut runtime, &read, &second);
    assert!(!runtime.is_quarantined());
}

#[test]
fn reload_shares_cells_preserves_recipe_and_isolates_candidate_and_new_installations() {
    let mut runtime = Runtime::default();
    let code = program(scalar(), true);
    let prepared = VerifiedProgram::new(code.clone(), runtime.definition_context()).unwrap();
    let baseline_program = runtime
        .load_verified_program("state", prepared.clone())
        .unwrap();
    let baseline = runtime.select_program(&baseline_program).unwrap();
    let independent_program = runtime.load_verified_program("state", prepared).unwrap();
    let independent = runtime.select_program(&independent_program).unwrap();
    assert_ne!(baseline.installation_id(), independent.installation_id());
    assert!(
        runtime
            .modules
            .bind_static_reload(&baseline, &independent)
            .is_err()
    );
    let other_runtime = &mut Runtime::default();
    let foreign_program = load_bytecode(other_runtime, "state", code.clone()).unwrap();
    let foreign = other_runtime.select_program(&foreign_program).unwrap();
    let (definitions, mut updated) = code.into_parts();
    let module = &mut updated.modules[0];
    let mut initializer = module.functions[1].clone();
    initializer.id = FunctionRef::new(2);
    // Relocate the same protected recipe; compatible publication may not
    // replace its expression. The old installation still retains slot 1.
    let mut filler = module.functions[0].clone();
    filler.id = FunctionRef::new(1);
    filler.name = "unrelated".into();
    filler.identity = None;
    let mut filler_record = module.function_table[0].clone();
    filler_record.id = filler.id;
    filler_record.name = filler.name.clone();
    filler_record.identity = None;
    let mut record = module.function_table[1].clone();
    record.id = initializer.id;
    record.name = initializer.name.clone();
    module.module_slots[0].initializer = initializer.id;
    module.functions[1] = filler;
    module.function_table[1] = filler_record;
    module.functions.push(initializer);
    module.function_table.push(record);
    let code = ProgramFixture::from(updated)
        .metadata(definitions, &Default::default())
        .unwrap();
    let candidate = runtime
        .stage_reload_program(&baseline_program, None, code.clone())
        .unwrap();
    assert_eq!(
        cell(&runtime.modules, &baseline, 0),
        cell(&runtime.modules, candidate.module(), 0)
    );
    assert_ne!(
        cell(&runtime.modules, &baseline, 0),
        cell(&runtime.modules, &independent, 0)
    );
    let mut session = runtime.begin_candidate_initialization(&candidate).unwrap();
    for target in [&baseline, candidate.module()] {
        assert_eq!(
            session
                .runtime_mut()
                .read_module_slot(target, ModuleSlot::new(0))
                .unwrap_err()
                .kind(),
            RuntimeErrorKind::ExecutionPhaseViolation
        );
        assert_eq!(
            session
                .runtime_mut()
                .write_module_slot(target, ModuleSlot::new(0), &RootedValue::from(9))
                .unwrap_err()
                .kind(),
            RuntimeErrorKind::ExecutionPhaseViolation
        );
    }
    drop(session);
    let current = candidate.module().clone();
    runtime.publish_staged_reload(candidate).unwrap();
    let StaticState::Uninitialized(recipe) = runtime
        .module_instance_snapshot(&current)
        .unwrap()
        .module_slots
        .remove(0)
    else {
        panic!("uninitialized")
    };
    assert_eq!(current.installation_id(), baseline.installation_id());
    assert_eq!(recipe.module.key(), baseline.key());
    assert_eq!(recipe.function, FunctionRef::new(1));
    assert_eq!(
        current.bytecode.module_slots[0].initializer,
        FunctionRef::new(2)
    );
    runtime.collect_garbage().unwrap();
    assert!(runtime.modules().loaded(baseline.key()).is_some());
    ready(&mut runtime, &current, 0, &RootedValue::from(100));
    runtime
        .write_module_slot(&baseline, ModuleSlot::new(0), &RootedValue::from(101))
        .unwrap();
    assert_eq!(read_i32(&mut runtime, &current, ModuleSlot::new(0)), 101);
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
    runtime.collect_garbage().unwrap();
    assert!(runtime.modules().loaded(baseline.key()).is_none());
    let fresh_program = load_bytecode(&mut runtime, "state", code).unwrap();
    let fresh = runtime.select_program(&fresh_program).unwrap();
    let StaticState::Uninitialized(fresh_recipe) = runtime
        .module_instance_snapshot(&fresh)
        .unwrap()
        .module_slots
        .remove(0)
    else {
        panic!("fresh recipe")
    };
    assert_ne!(fresh.installation_id(), current.installation_id());
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
    assert_eq!(read_i32(&mut runtime, &current, ModuleSlot::new(0)), 101);
}

#[test]
fn additions_reordering_and_stale_publication_are_transactional_and_deletion_is_rejected() {
    let mut runtime = Runtime::default();
    let original = program(scalar(), true);
    let baseline_program = load_bytecode(&mut runtime, "state", original.clone()).unwrap();
    let baseline = runtime.select_program(&baseline_program).unwrap();
    ready(&mut runtime, &baseline, 0, &RootedValue::from(7));
    let _baseline_lease = runtime
        .retain_module(
            &baseline,
            ModuleEpochRetention::RuntimeValue,
            RetentionOrigin::Current,
        )
        .unwrap();
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
    let updated = ProgramFixture::from(updated)
        .metadata(context.snapshot(), &Default::default())
        .unwrap();
    let abandoned = runtime
        .stage_reload_program(&baseline_program, None, updated.clone())
        .unwrap();
    let stale = runtime
        .stage_reload_program(&baseline_program, None, updated.clone())
        .unwrap();
    let candidate = runtime
        .stage_reload_program(&baseline_program, None, updated)
        .unwrap();
    let abandoned_key = abandoned.module().key();
    drop(abandoned);
    let current = candidate.module().clone();
    runtime.publish_staged_reload(candidate).unwrap();
    assert!(runtime.publish_staged_reload(stale).is_err());
    assert_eq!(read_i32(&mut runtime, &current, ModuleSlot::new(1)), 7);
    runtime
        .write_module_slot(&baseline, ModuleSlot::new(0), &RootedValue::from(8))
        .unwrap();
    assert_eq!(read_i32(&mut runtime, &current, ModuleSlot::new(1)), 8);
    assert_eq!(read_i32(&mut runtime, &baseline, ModuleSlot::new(0)), 8);
    assert_eq!(
        runtime
            .module_instance_snapshot(&baseline)
            .unwrap()
            .module_slots
            .len(),
        1
    );
    assert!(
        runtime
            .read_module_slot(&current, ModuleSlot::new(0))
            .is_err()
    );
    assert!(
        runtime
            .stage_reload_program(&baseline_program, None, original)
            .is_err()
    );
    runtime.collect_garbage().unwrap();
    assert!(runtime.modules().loaded(abandoned_key).is_none());
    assert_eq!(runtime.modules.inner.borrow().cells.len(), 2);
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum StaticContractChange {
    Removed,
    Mutability,
    Visibility,
    SemanticType,
}

#[rstest]
#[case::removed(StaticContractChange::Removed)]
#[case::mutability(StaticContractChange::Mutability)]
#[case::visibility(StaticContractChange::Visibility)]
#[case::semantic_type(StaticContractChange::SemanticType)]
fn private_contract_changes_remain_rejected(#[case] mutation: StaticContractChange) {
    let mut runtime = Runtime::default();
    let code = program(scalar(), true);
    let baseline_program = load_bytecode(&mut runtime, "state", code.clone()).unwrap();
    let baseline = runtime.select_program(&baseline_program).unwrap();
    ready(&mut runtime, &baseline, 0, &RootedValue::from(7));
    let (definitions, mut candidate) = code.into_parts();
    match mutation {
        StaticContractChange::Removed => candidate.modules[0].module_slots.clear(),
        StaticContractChange::Mutability => {
            candidate.modules[0].module_slots[0].contract.mutable = false
        }
        StaticContractChange::Visibility => {
            candidate.modules[0].module_slots[0].contract.visibility = Visibility::PublicCrate
        }
        StaticContractChange::SemanticType => {
            candidate.modules[0].module_slots[0].contract.ty = Ty::Builtin(BuiltinType::I64);
            candidate.modules[0].functions[1].metadata.semantic.result =
                Some(Ty::Builtin(BuiltinType::I64));
            candidate.modules[0].functions[1].metadata.return_type = ValueType::I64;
            candidate.modules[0].function_table[1].return_type = ValueType::I64;
            candidate.modules[0].types.push(ValueType::I64);
        }
    }
    assert!(
        runtime
            .stage_reload_program(
                &baseline_program,
                None,
                ProgramFixture::from(candidate)
                    .metadata(definitions, &Default::default())
                    .unwrap()
            )
            .is_err()
    );
    assert_eq!(read_i32(&mut runtime, &baseline, ModuleSlot::new(0)), 7);
}

#[rstest]
#[case::uninitialized(false)]
#[case::ready(true)]
fn public_constant_additions_preserve_contracts_and_lazy_static_state_source_free(
    #[case] initialized: bool,
) {
    let mut runtime = Runtime::default();
    let code = program(scalar(), true);
    let baseline_program = load_bytecode(&mut runtime, "state", code.clone()).unwrap();
    let baseline = runtime.select_program(&baseline_program).unwrap();
    if initialized {
        ready(&mut runtime, &baseline, 0, &RootedValue::from(7));
    }
    let context = DefinitionContext::new().unwrap();
    let mut candidate = code
        .import_into(&context, &Default::default())
        .unwrap()
        .into_records();
    let root = context.intern_root(&candidate.modules[0].identity).unwrap();
    context
        .intern_child(root, DefinitionKind::Const, "added_constant", 0)
        .unwrap();
    let added = PublicItem::Const(ConstDef {
        name: "added_constant".into(),
        ty: scalar(),
        value: "7".into(),
    });
    candidate.modules[0].public_items.push(added.clone());
    let candidate = ProgramFixture::from(candidate).build(&context);
    VerifiedProgram::new(
        DefinitionMetadata::checked(context.snapshot(), candidate.clone(), &Default::default())
            .unwrap(),
        &context,
    )
    .unwrap();
    let artifact =
        KbcArtifact::from_program(candidate, Default::default(), &context, &Default::default())
            .unwrap();
    let bytes = artifact.to_bytes(&context.snapshot()).unwrap();
    let staged = runtime
        .stage_reload_artifact(
            &baseline_program,
            None,
            KbcArtifact::from_bytes(&bytes).unwrap(),
            &Default::default(),
        )
        .unwrap();
    let published = staged.module().clone();
    runtime.publish_staged_reload(staged).unwrap();
    assert!(published.bytecode.public_items.contains(&added));
    assert_eq!(published.bytecode.module_slots.len(), 1);
    assert_eq!(
        cell(&runtime.modules, &baseline, 0),
        cell(&runtime.modules, &published, 0)
    );
    if initialized {
        assert_eq!(read_i32(&mut runtime, &published, ModuleSlot::new(0)), 7);
    } else {
        assert!(matches!(
            runtime
                .module_instance_snapshot(&published)
                .unwrap()
                .module_slots
                .remove(0),
            StaticState::Uninitialized(_)
        ));
    }
    assert_eq!(runtime.resources().counters().peak_call_depth, 0);
    // Once published, the added constant is an old contract, not disposable metadata.
    assert!(
        runtime
            .stage_reload_program(&baseline_program, None, code)
            .is_err()
    );
    assert_eq!(
        runtime.modules().current_for(&published).unwrap().key(),
        published.key()
    );
}

#[test]
fn corrupt_semantic_cell_quarantines_and_cleans_frame_resources() {
    let mut runtime = Runtime::default();
    let module_program = load_bytecode(&mut runtime, "state", program(scalar(), true)).unwrap();
    let module = runtime.select_program(&module_program).unwrap();
    // Deliberately bypass this test owner's semantic helper, while preserving the
    // actual provider's tracing/publication protocol. Reads must detect the fault.
    seed(&mut runtime, &module, 0, &RootedValue::from(true));
    let baseline = runtime.collected_gc_stats().root_registrations;
    let mut activation = runtime.enter_execution_stack(&module).unwrap();
    activation
        .with(|runtime, stack| {
            stack
                .push(runtime, module.slot(), FunctionRef::new(0), &[], None)
                .unwrap();
            let value = array(runtime, &module, 7);
            let mut cx = ConversionContext::new(runtime, &module).unwrap();
            let view = cx.import(&value).unwrap();
            stack
                .write_slot(cx.runtime_mut(), Slot::new(0), &view)
                .unwrap();
            drop(cx);
            drop(value);
            assert_eq!(
                runtime
                    .read_module_slot(&module, ModuleSlot::new(0))
                    .unwrap_err()
                    .kind(),
                RuntimeErrorKind::EngineFault
            );
            Ok(())
        })
        .unwrap();
    drop(activation);
    assert!(runtime.is_quarantined());
    assert!(runtime.execution_root().is_none());
    assert_eq!(runtime.resources().counters().current_call_depth, 0);
    assert_eq!(runtime.collected_gc_stats().root_registrations, baseline);
}
