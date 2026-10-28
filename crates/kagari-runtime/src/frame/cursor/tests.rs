//! Region results remain inspectable only after successful publication and released borrows.
mod managed_writes;
use crate::{
    Runtime,
    error::{RuntimeError, RuntimeErrorKind},
    frame::{
        ExecutionStack,
        cursor::kernel::{RegionError, RegionExit},
        driver::{ExecutionAction, ReturnAction},
    },
    module::ExecutableModule,
    native::{
        binding::NativeResult, builder::ModuleBuilder, catalog::DeclarationCatalog,
        context::CallContext, declarations::FunctionDecl, types::Type,
    },
    value::Value,
};
use kagari_bytecode::{
    instruction::{BytecodeInstruction, CallTarget, Slot},
    program::updates,
};
use kagari_common::identity::metadata::DefinitionMetadata;
use kagari_compiler::{bytecode::lower_program_to_bytecode, source::program::lower_program_to_mir};
use kagari_hir::analysis::AnalysisDatabase;
use kagari_source::source_database::{SourceDatabase, SourceLayer};
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

fn native_completion_fixture(
    fail_second: bool,
    discard: bool,
) -> (Runtime, ExecutableModule, Arc<AtomicUsize>) {
    let calls = Arc::new(AtomicUsize::new(0));
    let invoked = calls.clone();
    let mut native =
        ModuleBuilder::new("test::completion", &DeclarationCatalog::default()).unwrap();
    let value = native
        .define_function(FunctionDecl::new("value").returns(if discard {
            Type::unit()
        } else {
            Type::i32()
        }))
        .unwrap();
    if discard {
        native
            .bind(value, move |cx: &mut CallContext<'_>| -> NativeResult<()> {
                assert!(!cx.runtime.capture_error_trace().incomplete);
                cx.runtime.collect_garbage()?;
                invoked.fetch_add(1, Ordering::Relaxed);
                Ok(())
            })
            .unwrap();
    } else {
        native
            .bind(
                value,
                move |cx: &mut CallContext<'_>| -> NativeResult<i32> {
                    assert!(!cx.runtime.capture_error_trace().incomplete);
                    cx.runtime.collect_garbage()?;
                    if invoked.fetch_add(1, Ordering::Relaxed) == 1 && fail_second {
                        Err(RuntimeError::new(
                            RuntimeErrorKind::ScriptTrap,
                            "native failure",
                        ))
                    } else {
                        Ok(7)
                    }
                },
            )
            .unwrap();
    }
    let native = native.finish().unwrap();
    let source = if discard {
        "use test::completion::value; fn main() { value(); }"
    } else {
        // Keep two intermediates live before their sum. Constant-operand lowering
        // otherwise updates the first result in place and leaves no retired slot
        // for the second native destination to reuse.
        "use test::completion::value; fn main() -> i32 {
            val a = value(); val b = a + 1; (a + b) + value() - 7
        }"
    };
    let mut sources = SourceDatabase::default();
    let root = sources
        .set("completion.kgr", source.into(), SourceLayer::Base)
        .unwrap();
    let mut analysis = AnalysisDatabase::default();
    analysis.set_native_modules(vec![Arc::new(native.to_declaration())]);
    let snapshot = analysis
        .snapshot(sources.snapshot(), &Default::default())
        .unwrap();
    let checked = snapshot.check_program(root, &Default::default()).unwrap();
    let mir = lower_program_to_mir(&checked, &Default::default()).unwrap();
    let code = lower_program_to_bytecode(&mir, &Default::default())
        .map(|program| program.into_unverified())
        .unwrap();
    let code = if discard {
        let (definitions, mut program) = code.into_parts();
        for function in &mut program.modules[program.root.index()].functions {
            for instruction in &mut function.instructions {
                if let BytecodeInstruction::Call {
                    dst,
                    callee: CallTarget::Native(_),
                    ..
                } = instruction
                {
                    *dst = None;
                }
            }
        }
        program.update_evidence = updates::project(
            &program.modules,
            &program.update_evidence,
            &definitions,
            &Default::default(),
        )
        .unwrap();
        DefinitionMetadata::checked(definitions, program, &Default::default()).unwrap()
    } else {
        code
    };
    let mut runtime = Runtime::default();
    native.install(&mut runtime).unwrap();
    let loaded_program = runtime.load_program("completion", code).unwrap();
    let loaded = runtime.select_program(&loaded_program).unwrap();
    (runtime, loaded, calls)
}

#[test]
fn native_publication_exposes_first_definitions_only_after_success() {
    for fail_second in [false, true] {
        let (runtime, loaded, calls) = native_completion_fixture(fail_second, false);
        let entry = loaded
            .bytecode
            .functions
            .iter()
            .find(|function| function.name == "main")
            .unwrap()
            .id;
        let stack = runtime.enter_execution_stack(&loaded).unwrap();
        stack
            .push(&runtime, loaded.slot(), entry, &[], None)
            .unwrap();
        for call in 0..2 {
            let result = complete_native_call(&stack, &runtime);
            let frame = stack.current().unwrap();
            let pc = frame.instruction_offset();
            let BytecodeInstruction::Call { dst: Some(dst), .. } =
                frame.function().unwrap().instructions[pc]
            else {
                panic!("native call remains the observable PC");
            };
            if call == 1 {
                assert!(
                    frame
                        .slot_layout
                        .as_ref()
                        .unwrap()
                        .reused_slots
                        .iter()
                        .any(|slot| {
                            slot.logical == dst.index()
                                && slot.range.is_some_and(|(start, _)| start == pc)
                        }),
                    "exercise the first definition of a reused native destination"
                );
            }
            if call == 1 && fail_second {
                assert!(
                    matches!(result, Err(RegionError::Runtime(ref error)) if error.kind() == RuntimeErrorKind::ScriptTrap)
                );
                assert_eq!(
                    frame.read_slot(&runtime, dst).unwrap_err().kind(),
                    RuntimeErrorKind::ModuleValidation
                );
            } else {
                assert!(matches!(result, Ok(RegionExit::Safepoint)));
                assert_eq!(frame.read_slot(&runtime, dst).unwrap(), Value::I32(7));
            }
        }
        assert_eq!(calls.load(Ordering::Relaxed), 2);
        if !fail_second {
            let RegionExit::Return(value) = stack
                .execute_region(&runtime, &mut None, |_, _, _, _| unreachable!())
                .unwrap()
            else {
                panic!("native caller returns");
            };
            assert_eq!(value.materialize(), Some(Value::I32(15)));
        }
        drop(stack);
        assert_eq!(runtime.resources().counters().current_call_depth, 0);
        assert_eq!(runtime.gc().active_roots(), 0);
    }
}

#[test]
fn native_without_destination_finishes_once_before_the_successor() {
    let (runtime, loaded, calls) = native_completion_fixture(false, true);
    let entry = loaded
        .bytecode
        .functions
        .iter()
        .find(|function| function.name == "main")
        .unwrap()
        .id;
    let stack = runtime.enter_execution_stack(&loaded).unwrap();
    stack
        .push(&runtime, loaded.slot(), entry, &[], None)
        .unwrap();
    assert!(matches!(
        complete_native_call(&stack, &runtime),
        Ok(RegionExit::Safepoint)
    ));
    {
        let frame = stack.current().unwrap();
        assert!(matches!(
            frame.function().unwrap().instructions[frame.instruction_offset()],
            BytecodeInstruction::Call { dst: None, .. }
        ));
    }
    let RegionExit::Return(value) = stack
        .execute_region(&runtime, &mut None, |_, _, _, _| unreachable!())
        .unwrap()
    else {
        panic!("discarding caller returns");
    };
    assert_eq!(value.materialize(), Some(Value::Unit));
    assert_eq!(calls.load(Ordering::Relaxed), 1);
    drop(stack);
    assert_eq!(runtime.resources().counters().current_call_depth, 0);
    assert_eq!(runtime.gc().active_roots(), 0);
}

fn complete_native_call(
    stack: &ExecutionStack<'_>,
    runtime: &Runtime,
) -> Result<RegionExit, RegionError> {
    assert!(matches!(
        stack.execute_region(runtime, &mut None, |_, _, _, _| unreachable!())?,
        RegionExit::Call(ExecutionAction::NativeEntry)
    ));
    if let Err(error) = stack.start_native_entry(runtime, |_, _, _, _| unreachable!()) {
        stack.pop()?;
        return Err(error.into());
    }
    assert_eq!(
        stack.finish_native_return(runtime)?,
        ReturnAction::Continue(ExecutionAction::Script)
    );
    Ok(RegionExit::Safepoint)
}

#[test]
fn collection_during_an_operand_borrow_quarantines_without_further_execution() {
    let mut sources = SourceDatabase::default();
    let root = sources
        .set(
            "borrow.kgr",
            "fn main() -> i32 { 42 }".into(),
            SourceLayer::Base,
        )
        .unwrap();
    let snapshot = AnalysisDatabase::default()
        .snapshot(sources.snapshot(), &Default::default())
        .unwrap();
    let checked = snapshot.check_program(root, &Default::default()).unwrap();
    let mir = lower_program_to_mir(&checked, &Default::default()).unwrap();
    let code = lower_program_to_bytecode(&mir, &Default::default())
        .map(|program| program.into_unverified())
        .unwrap();
    let mut runtime = Runtime::default();
    let loaded_program = runtime.load_program("borrow", code).unwrap();
    let loaded = runtime.select_program(&loaded_program).unwrap();
    let entry = loaded
        .bytecode
        .functions
        .iter()
        .find(|function| function.name == "main")
        .unwrap()
        .id;
    let stack = runtime.enter_execution_stack(&loaded).unwrap();
    stack
        .push(&runtime, loaded.slot(), entry, &[], None)
        .unwrap();
    let mut frame = stack.current_mut().unwrap();
    // Only engine code can now retain this borrow; GC roots live in the banks,
    // independently of frame inspection. Simulate violating that engine contract.
    let values = runtime.resources().frame_values.borrow_mut();
    let error = runtime.collect_garbage().unwrap_err();
    assert_eq!(error.kind(), RuntimeErrorKind::EngineFault);
    drop(values);
    assert!(
        frame
            .write_slot(&runtime, Slot::new(0), Value::I32(99))
            .is_err()
    );
    assert!(frame.read_slot(&runtime, Slot::new(0)).is_err());
    drop(frame);
    assert!(
        stack
            .execute_region(&runtime, &mut None, |_, _, _, _| unreachable!(
                "quarantined execution cannot invoke script"
            ))
            .is_err()
    );
    drop(stack);
    assert_eq!(runtime.gc().active_roots(), 0);
    assert_eq!(runtime.resources().counters().current_call_depth, 0);
}
