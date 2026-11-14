use super::*;
use crate::error::RuntimeErrorKind;
use crate::error::reason::ErrorReason;
use crate::error::trap::TrapReason;
use crate::module::{entry::ProgramEntry, retention::RetentionOrigin};
#[cfg(test)]
use crate::test_support::bytecode::load_bytecode;
use crate::{
    execution::fixtures::FixtureExecution,
    execution::owned::DriveResult,
    frame::ExecutionFrame,
    interpreter_tests::common::{round_trip_bytecode, test_runtime},
    native::conversion::context::ConversionContext,
    test_support::native::program::ProgramFixture,
    value::carriers::RootedValue,
};
use kagari_bytecode::{
    instruction::{BytecodeInstruction, ConstantId, StructId},
    program::{ModuleRef, verify_program},
};
use kagari_contract::ids::FunctionRef;
use std::{mem::size_of, num::NonZeroUsize};

#[test]
fn foreign_loaded_module_is_rejected_before_execution() {
    let bytecode = compile_test_bytecode("fn main() -> i32 { 7 }");
    let mut first = test_runtime(Default::default());
    let mut second = test_runtime(Default::default());
    let foreign = load_bytecode(&mut first, "same", bytecode.clone()).unwrap();
    let _foreign_program = foreign;
    let foreign = first.select_program(&_foreign_program).unwrap();
    let local = load_bytecode(&mut second, "same", bytecode).unwrap();
    let _local_program = local;
    let local = second.select_program(&_local_program).unwrap();
    assert_eq!(foreign.key(), local.key());
    let mut vm = second;
    assert!(
        matches!(vm.execute_module(&foreign, ProgramEntry::Main, vm.execution_options()), Err(ref error) if error.kind() == RuntimeErrorKind::StaleHandle)
    );
    assert_eq!(
        vm.execute_module(&local, ProgramEntry::Main, vm.execution_options())
            .unwrap()
            .return_value
            .as_i32(),
        Some(7)
    );
}

#[test]
fn executes_simple_arithmetic_function() {
    let (runtime, loaded) = load_test_module("fn main() -> i32 { val value = 1 + 2; value }");
    let _loaded_program = loaded;
    let loaded = runtime.select_program(&_loaded_program).unwrap();
    let mut vm = runtime;
    let report = vm
        .execute_module(&loaded, ProgramEntry::Main, vm.execution_options())
        .expect("vm should execute");

    assert_eq!(
        ConversionContext::new(&mut vm, &loaded)
            .unwrap()
            .decode::<i32>(&report.return_value)
            .unwrap(),
        3
    );
    assert_eq!(vm.modules().retention_counts(loaded.key()).active_calls, 0);
}

#[test]
fn rejects_unverified_bytecode_before_publication() {
    let bytecode = verified_module(
        vec![test_function(
            0,
            "main",
            vec![
                BytecodeInstruction::LoadConst {
                    dst: Slot::new(0),
                    constant: ConstantId::new(0),
                },
                BytecodeInstruction::Return(Some(Slot::new(0))),
            ],
            ValueType::I32,
            vec![ValueType::I32],
        )],
        vec![ConstantOperand::I32(1)],
    );
    let context = DefinitionContext::new().unwrap();
    let mut program = ProgramFixture {
        exports: Default::default(),
        root: ModuleRef::new(0),
        modules: vec![bytecode],
    }
    .build(&context);
    program.modules[0].function_table.clear();
    let mut runtime = test_runtime(RuntimeConfig {
        limits: RuntimeLimits {
            ..RuntimeLimits::default()
        },
        ..RuntimeConfig::default()
    });
    let error = load_bytecode(
        &mut runtime,
        "unverified.kbc",
        DefinitionMetadata::checked(context.snapshot(), program, &Default::default()).unwrap(),
    )
    .expect_err("runtime must reject unverified bytecode before publication");

    assert_eq!(error.kind(), RuntimeErrorKind::ModuleValidation);
    assert!(error.to_string().contains("function table length mismatch"));
    assert_eq!(runtime.modules().loaded_count(), 0);
}

#[test]
fn rejects_unsupported_bytecode_before_publication() {
    let register_call = verified_module(
        vec![test_function(
            0,
            "main",
            vec![
                BytecodeInstruction::LoadConst {
                    dst: Slot::new(0),
                    constant: ConstantId::new(0),
                },
                BytecodeInstruction::Call {
                    dst: None,
                    callee: CallTarget::Slot(Slot::new(0)),
                    args: vec![],
                },
                BytecodeInstruction::Return(None),
            ],
            ValueType::Unit,
            vec![ValueType::I32],
        )],
        vec![ConstantOperand::I32(1)],
    );
    let dynamic_call = verified_module(
        vec![test_function(
            0,
            "main",
            vec![
                BytecodeInstruction::Call {
                    dst: None,
                    callee: CallTarget::RuntimeHelper(RuntimeHelper::DynamicCall),
                    args: vec![],
                },
                BytecodeInstruction::Return(None),
            ],
            ValueType::Unit,
            vec![],
        )],
        vec![],
    );
    let mut runtime = test_runtime(Default::default());
    for (name, bytecode) in [
        ("register_call.kbc", register_call),
        ("dynamic_call.kbc", dynamic_call),
    ] {
        let error = load_bytecode(
            &mut runtime,
            name,
            ProgramFixture {
                exports: Default::default(),
                root: ModuleRef::new(0),
                modules: vec![bytecode],
            }
            .metadata(
                DefinitionContext::new().unwrap().snapshot(),
                &Default::default(),
            )
            .unwrap(),
        )
        .expect_err("unsupported calls must fail bytecode verification");
        assert_eq!(error.kind(), RuntimeErrorKind::ModuleValidation);
        assert!(error.to_string().contains("invalid operation"));
        assert_eq!(runtime.modules().loaded_count(), 0);
    }
}

#[test]
fn executes_if_control_flow() {
    let (runtime, loaded) = load_test_module("fn main() -> i32 { if true { 1 } else { 2 } }");
    let _loaded_program = loaded;
    let loaded = runtime.select_program(&_loaded_program).unwrap();
    let mut vm = runtime;
    let report = vm
        .execute_module(&loaded, ProgramEntry::Main, vm.execution_options())
        .expect("vm should execute");

    assert_eq!(report.return_value.as_i32(), Some(1));
}

#[test]
fn executes_direct_function_calls() {
    let (runtime, loaded) = load_test_module(
        r#"
fn callee() -> i32 { 7 }
fn main() -> i32 { callee() }
"#,
    );
    let _loaded_program = loaded;
    let loaded = runtime.select_program(&_loaded_program).unwrap();
    let mut vm = runtime;
    let report = vm
        .execute_module(&loaded, ProgramEntry::Main, vm.execution_options())
        .expect("vm should execute");

    assert_eq!(report.return_value.as_i32(), Some(7));
}

#[test]
fn deterministic_frames_keep_caller_and_callee_storage_isolated() {
    let mut vm = test_runtime(Default::default());
    let program = load_bytecode(
        &mut vm,
        "isolated-frames",
        round_trip_bytecode(compile_test_bytecode(include_str!(
            "../../../tests/fixtures/execution/isolated-call-frames.kgr"
        ))),
    )
    .unwrap();
    let loaded = vm.select_program(&program).unwrap();
    vm.collect_garbage().unwrap();
    let baseline = vm.gc_stats();
    let report = vm
        .execute_module(&loaded, ProgramEntry::Main, vm.execution_options())
        .expect("vm should execute");

    assert_eq!(report.return_value.as_i32(), Some(12));
    assert_eq!(vm.resources().counters().current_call_depth, 0);

    // A one-instruction slice must preserve both the entered callee's original
    // argument and the completed Call's published result, without observation
    // forcing an ordinary outer-driver transition.
    let execution = vm
        .start(&loaded, ProgramEntry::Main, &[], vm.execution_options())
        .unwrap();
    let mut entered = false;
    let mut published = false;
    let mut result = None;
    for _ in 0..32 {
        match vm.drive(&execution, NonZeroUsize::new(1).unwrap()).unwrap() {
            DriveResult::Complete(value) => {
                result = value.unwrap().as_i32();
                break;
            }
            DriveResult::Waiting => panic!("synchronous scalar frames cannot wait"),
            DriveResult::Runnable => {
                let frames = vm.resources.sessions.frames(execution.id).unwrap();
                assert!(frames.iter().all(|frame| !frame.tail_calls_elided()));
                if frames.len() == 2 {
                    let callee = &frames[1];
                    let function = callee.function().unwrap();
                    assert_eq!(frames[0].function().unwrap().name, "main");
                    assert_eq!(function.name, "callee");
                    if callee.instruction_offset() == 0 {
                        let input = function.metadata.parameter_slots[0];
                        assert_eq!(callee.read_slot(&vm, input).unwrap().as_i32(), Some(1));
                        entered = true;
                    }
                } else {
                    assert_eq!(frames.len(), 1);
                    let caller = &frames[0];
                    if let BytecodeInstruction::Call { dst: Some(dst), .. } =
                        caller.function().unwrap().instructions[caller.instruction_offset()]
                    {
                        assert_eq!(caller.read_slot(&vm, dst).unwrap().as_i32(), Some(2));
                        published = true;
                    }
                }
                drop(frames);
                vm.collect_garbage().unwrap();
            }
        }
    }
    assert_eq!(result, Some(12));
    assert!(entered && published);
    assert_eq!(vm.resources().counters().peak_call_depth, 2);
    assert_eq!(vm.resources().counters().current_call_depth, 0);
    assert!(vm.execution_root().is_none());
    assert!(!vm.is_quarantined());
    vm.collect_garbage().unwrap();
    assert_eq!(vm.gc_stats().objects, baseline.objects);
    assert_eq!(
        vm.gc_stats().root_registrations,
        baseline.root_registrations
    );
}

#[test]
fn vm_space_bounds_empty_and_wider_frames_and_unwinds_failure() {
    let code = round_trip_bytecode(compile_test_bytecode(include_str!(
        "../../../tests/fixtures/execution/stack-space.kgr"
    )));
    let mut peaks = Vec::new();
    for entry in ["empty", "larger"] {
        let limit = 16 * 1024;
        let mut vm = test_runtime(RuntimeConfig {
            limits: RuntimeLimits {
                max_stack_bytes: NonZeroUsize::new(limit).unwrap(),
                ..Default::default()
            },
            ..Default::default()
        });
        let program = load_bytecode(&mut vm, "stack-space", code.clone()).unwrap();
        let owner = vm.select_program(&program).unwrap();
        let layout = |name: &str| {
            let function = owner
                .bytecode
                .functions
                .iter()
                .find(|function| function.name == name)
                .unwrap();
            &owner.execution().functions[function.id.index()]
                .frame
                .slot_layout
        };
        assert_eq!(
            layout("empty").scalar_count + layout("empty").managed_count,
            0
        );
        assert!(layout("wide").scalar_count >= 8);
        vm.collect_garbage().unwrap();
        let baseline = vm.gc_stats();
        let error = vm
            .execute_module(&owner, ProgramEntry::Export(entry), vm.execution_options())
            .unwrap_err();
        assert_eq!(error.reason(), &ErrorReason::StackOverflow);
        let peak = vm.resources().counters().peak_call_depth;
        assert!(peak > 1);
        assert!(peak as usize * size_of::<ExecutionFrame>() <= limit);
        assert_eq!(error.trace().unwrap().frames.len(), peak as usize);
        assert_eq!(vm.resources().counters().current_call_depth, 0);
        assert!(!vm.is_quarantined());
        vm.collect_garbage().unwrap();
        assert_eq!(
            vm.gc_stats().root_registrations,
            baseline.root_registrations
        );
        assert_eq!(vm.gc_stats().objects, baseline.objects);
        assert_eq!(
            vm.execute_module(
                &owner,
                ProgramEntry::Export("healthy"),
                vm.execution_options()
            )
            .unwrap()
            .return_value
            .as_i32(),
            Some(9)
        );
        peaks.push(peak);
    }
    assert!(
        peaks[1] < peaks[0],
        "live scalar/outgoing capacity consumes space independently of frame count"
    );
}

#[test]
fn unreachable_instruction_is_a_script_trap() {
    let mut runtime = host_call_runtime();
    let loaded = load_bytecode(
        &mut runtime,
        "trap.kbc",
        ProgramFixture {
            exports: Default::default(),
            root: ModuleRef::new(0),
            modules: vec![verified_module(
                vec![test_function(
                    0,
                    "main",
                    vec![BytecodeInstruction::Unreachable],
                    ValueType::Unit,
                    vec![],
                )],
                vec![],
            )],
        }
        .metadata(
            DefinitionContext::new().unwrap().snapshot(),
            &Default::default(),
        )
        .unwrap(),
    )
    .expect("module should load");
    let _loaded_program = loaded;
    let loaded = runtime.select_program(&_loaded_program).unwrap();

    let mut vm = runtime;
    let error = vm
        .execute_fixture(&loaded, FunctionRef::new(0))
        .expect_err("unreachable should trap");

    assert!(
        matches!(error.reason(), ErrorReason::Trap(TrapReason::Provider(reason)) if reason == "unreachable")
    );
    assert_eq!(vm.resources().counters().current_call_depth, 0);
}

#[test]
fn executes_array_index_access() {
    let (mut runtime, installed) = load_test_module(include_str!(
        "../../../tests/fixtures/execution/array-index.kgr"
    ));
    let loaded = runtime.select_program(&installed).unwrap();
    let report = runtime
        .execute_module(&loaded, ProgramEntry::Main, runtime.execution_options())
        .unwrap();
    let mut conversion = ConversionContext::new(&mut runtime, &loaded).unwrap();
    assert_eq!(conversion.decode::<i32>(&report.return_value).unwrap(), 2);
}

#[test]
fn executes_struct_field_access() {
    let (mut runtime, installed) = load_test_module(include_str!(
        "../../../tests/fixtures/execution/struct-field.kgr"
    ));
    let loaded = runtime.select_program(&installed).unwrap();
    let report = runtime
        .execute_module(&loaded, ProgramEntry::Main, runtime.execution_options())
        .unwrap();
    let mut conversion = ConversionContext::new(&mut runtime, &loaded).unwrap();
    assert_eq!(conversion.decode::<i32>(&report.return_value).unwrap(), 2);
}

#[test]
fn executes_tuple_literal_return() {
    let (mut runtime, installed) = load_test_module(include_str!(
        "../../../tests/fixtures/execution/tuple-return.kgr"
    ));
    let loaded = runtime.select_program(&installed).unwrap();
    runtime.collect_garbage().unwrap();
    let baseline = runtime.gc_stats();
    let report = runtime
        .execute_module(&loaded, ProgramEntry::Main, runtime.execution_options())
        .unwrap();
    runtime.collect_garbage().unwrap();
    let mut conversion = ConversionContext::new(&mut runtime, &loaded).unwrap();
    assert_eq!(
        conversion
            .decode::<(bool, bool)>(&report.return_value)
            .unwrap(),
        (true, false)
    );
    drop((conversion, report));
    runtime.collect_garbage().unwrap();
    assert_eq!(runtime.gc_stats().objects, baseline.objects);
    assert_eq!(
        runtime.gc_stats().root_registrations,
        baseline.root_registrations
    );
}

#[test]
fn executes_struct_literal_return() {
    let (mut runtime, installed) = load_test_module(include_str!(
        "../../../tests/fixtures/execution/struct-return.kgr"
    ));
    let loaded = runtime.select_program(&installed).unwrap();
    runtime.collect_garbage().unwrap();
    let baseline = runtime.gc_stats();
    let report = runtime
        .execute_module(&loaded, ProgramEntry::Main, runtime.execution_options())
        .unwrap();
    runtime.collect_garbage().unwrap();
    let mut conversion = ConversionContext::new(&mut runtime, &loaded).unwrap();
    let value = conversion.import(&report.return_value).unwrap();
    let layout = conversion.struct_layout(&value).unwrap();
    assert_eq!(
        layout.layout().name(layout.module().definitions()),
        Some("Point")
    );
    assert_eq!(
        layout
            .layout()
            .fields
            .iter()
            .map(|field| field.name.as_str())
            .collect::<Vec<_>>(),
        ["x", "y"]
    );
    assert_eq!(
        conversion
            .struct_field(&value, &layout, 0)
            .unwrap()
            .as_i32(),
        Some(1)
    );
    assert_eq!(
        conversion
            .struct_field(&value, &layout, 1)
            .unwrap()
            .as_i32(),
        Some(2)
    );
    drop((conversion, report));
    runtime.collect_garbage().unwrap();
    assert_eq!(runtime.gc_stats().objects, baseline.objects);
    assert_eq!(
        runtime.gc_stats().root_registrations,
        baseline.root_registrations
    );
}

#[test]
fn lazy_static_driver_enters_hidden_initializer_but_stores_cannot_initialize() {
    let mut runtime = test_runtime(Default::default());
    let context = DefinitionContext::new().unwrap();
    let module = module_with_mutable_slot(&context, 7);
    let loaded = load_bytecode(
        &mut runtime,
        "module-slot-writes",
        ProgramFixture {
            exports: Default::default(),
            root: ModuleRef::new(0),
            modules: vec![module],
        }
        .metadata(context.snapshot(), &Default::default())
        .unwrap(),
    )
    .unwrap();
    let _loaded_program = loaded;
    let loaded = runtime.select_program(&_loaded_program).unwrap();
    let mut vm = runtime;
    let roots_before = vm.gc_stats().root_registrations;
    assert!(vm.execute_fixture(&loaded, FunctionRef::new(0)).is_err());
    assert!(
        vm.write_module_slot(&loaded, ModuleSlot::new(0), &RootedValue::from(9i32))
            .is_err()
    );
    assert!(vm.read_module_slot(&loaded, ModuleSlot::new(0)).is_err());
    let result = vm.execute_fixture(&loaded, FunctionRef::new(1)).unwrap();
    assert_eq!(result.return_value.as_i32(), Some(7));
    drop(result);
    vm.write_module_slot(&loaded, ModuleSlot::new(0), &RootedValue::from(9i32))
        .unwrap();
    assert_eq!(
        vm.read_module_slot(&loaded, ModuleSlot::new(0))
            .unwrap()
            .as_i32(),
        Some(9)
    );
    assert!(!vm.is_quarantined());
    assert!(vm.execution_root().is_none());
    assert_eq!(vm.resources().counters().current_call_depth, 0);
    assert_eq!(vm.gc_stats().root_registrations, roots_before);
}

#[test]
fn reload_preserves_active_old_epoch_while_new_calls_use_latest_epoch() {
    let mut runtime = test_runtime(Default::default());
    let first_loaded = load_bytecode(
        &mut runtime,
        "hot_reload.kgr",
        ProgramFixture {
            exports: Default::default(),
            root: ModuleRef::new(0),
            modules: vec![reloadable_value_module(1)],
        }
        .metadata(
            DefinitionContext::new().unwrap().snapshot(),
            &Default::default(),
        )
        .unwrap(),
    )
    .expect("first module epoch should load");
    let _first_loaded_program = first_loaded;
    let first_loaded = runtime.select_program(&_first_loaded_program).unwrap();
    let retention = runtime
        .retain_module(
            &first_loaded,
            ModuleEpochRetention::ActiveCall,
            RetentionOrigin::Current,
        )
        .unwrap();

    let second_loaded = runtime
        .stage_reload_program(
            &_first_loaded_program,
            Some("hot_reload.kgr".into()),
            ProgramFixture {
                exports: Default::default(),
                root: ModuleRef::new(0),
                modules: vec![reloadable_value_module(2)],
            }
            .metadata(
                DefinitionContext::new().unwrap().snapshot(),
                &Default::default(),
            )
            .unwrap(),
        )
        .expect("compatible reload should publish a new epoch");
    let staged = second_loaded;
    let second_loaded = staged.module().clone();
    runtime.publish_staged_reload(staged).unwrap();
    let _second_loaded_program = _first_loaded_program.clone();

    runtime.collect_garbage().unwrap();
    assert!(
        runtime.modules().loaded(first_loaded.key()).is_some(),
        "the actual retained old revision must remain reachable after reload"
    );
    assert!(runtime.modules().loaded(second_loaded.key()).is_some());
    assert_eq!(
        runtime
            .modules()
            .retention_counts(first_loaded.key())
            .active_calls,
        1
    );

    let mut vm = runtime;
    let old_report = vm
        .execute_fixture(&first_loaded, FunctionRef::new(0))
        .expect("an ordinary call through old evidence should select current code");
    let latest = vm
        .select_program(&_first_loaded_program)
        .expect("latest module epoch should be visible");
    let latest_report = vm
        .execute_fixture(&latest, FunctionRef::new(0))
        .expect("new call should use latest epoch");

    assert_eq!(second_loaded.epoch, latest.epoch);
    assert_eq!(old_report.epoch, second_loaded.epoch.0);
    assert_eq!(old_report.return_value.as_i32(), Some(2));
    assert_eq!(latest_report.epoch, second_loaded.epoch.0);
    assert_eq!(latest_report.return_value.as_i32(), Some(2));

    drop(retention);
    vm.collect_garbage().unwrap();
    assert!(vm.modules().loaded(first_loaded.key()).is_none());
    assert!(vm.modules().loaded(second_loaded.key()).is_some());
}

#[test]
fn aggregate_field_instructions_reject_a_different_nominal_receiver() {
    // Semantic receiver contracts reject layout substitution before execution.
    for write in [false, true] {
        let (definitions, mut bytecode) = compile_test_bytecode(
            "struct P { var x: i32 } struct Q { var x: i32 } fn main() -> i32 { val p = P { x: 1 }; p.x = 42; p.x }",
        ).into_parts();
        let wrong = StructId::new(
            bytecode.modules[bytecode.root.index()]
                .structures
                .iter()
                .position(|layout| layout.name(&definitions) == Some("Q"))
                .unwrap(),
        );
        for instruction in bytecode.modules[bytecode.root.index()]
            .functions
            .iter_mut()
            .flat_map(|function| &mut function.instructions)
        {
            match instruction {
                BytecodeInstruction::WriteAggregateField { field, .. } if write => {
                    field.structure = wrong
                }
                BytecodeInstruction::ReadAggregateField { field, .. } if !write => {
                    field.structure = wrong
                }
                _ => {}
            }
        }
        assert!(verify_program(&bytecode, &definitions, &Default::default()).is_err());
    }
}
