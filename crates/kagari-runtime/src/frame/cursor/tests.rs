//! Region results remain inspectable only after successful publication and released borrows.
#[cfg(test)]
use crate::test_support::bytecode::load_bytecode;
use crate::{
    Runtime,
    error::{RuntimeError, RuntimeErrorKind, trap::TrapReason},
    frame::{
        FrameStack,
        cursor::kernel::{RegionError, RegionExit},
        driver::{ExecutionAction, ReturnAction},
    },
    module::ExecutableModule,
    native::{
        binding::NativeResult, builder::ModuleBuilder, catalog::DeclarationCatalog,
        context::NativeCall, declarations::FunctionDecl, types::Type,
    },
};

use kagari_abi::representation::ValueType;
use kagari_bytecode::{
    instruction::{BytecodeInstruction, CallTarget},
    program::updates,
};
use kagari_common::identity::metadata::DefinitionMetadata;
use kagari_compiler::{bytecode::lower_program_to_bytecode, source::program::lower_program_to_mir};
use kagari_hir::{analysis::AnalysisDatabase, compile_input::program::CheckedCompilation};
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
            .bind(value, move |cx: &mut NativeCall<'_>| -> NativeResult<()> {
                assert!(!cx.runtime.capture_error_trace().incomplete);
                cx.runtime.collect_garbage()?;
                invoked.fetch_add(1, Ordering::Relaxed);
                Ok(())
            })
            .unwrap();
    } else {
        native
            .bind(value, move |cx: &mut NativeCall<'_>| -> NativeResult<i32> {
                assert!(!cx.runtime.capture_error_trace().incomplete);
                cx.runtime.collect_garbage()?;
                if invoked.fetch_add(1, Ordering::Relaxed) == 1 && fail_second {
                    Err(RuntimeError::from(TrapReason::Provider(
                        "native failure".into(),
                    )))
                } else {
                    Ok(7)
                }
            })
            .unwrap();
    }
    let native = native.finish().unwrap();
    let source = if discard {
        include_str!("../../../tests/fixtures/execution/native-completion-discard.kgr")
    } else {
        // Keep two intermediates live before their sum. Constant-operand lowering
        // otherwise updates the first result in place and leaves no retired slot
        // for the second native destination to reuse.
        include_str!("../../../tests/fixtures/execution/native-completion-value.kgr")
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
    let mir = lower_program_to_mir(
        &CheckedCompilation::from_program(&checked, &Default::default()).unwrap(),
        &Default::default(),
    )
    .unwrap();
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
    let loaded_program = load_bytecode(&mut runtime, "completion", code).unwrap();
    let loaded = runtime.select_program(&loaded_program).unwrap();
    (runtime, loaded, calls)
}

#[test]
fn native_publication_exposes_first_definitions_only_after_success() {
    for fail_second in [false, true] {
        let (mut runtime, loaded, calls) = native_completion_fixture(fail_second, false);
        let entry = loaded
            .bytecode
            .functions
            .iter()
            .find(|function| function.name == "main")
            .unwrap()
            .id;
        let roots_before = runtime.gc_stats().root_registrations;
        let mut stack_activation = runtime.enter_execution_stack(&loaded).unwrap();
        stack_activation
            .with(|runtime, stack| {
                stack
                    .push(runtime, loaded.slot(), entry, &[], None)
                    .unwrap();
                for call in 0..2 {
                    let result = complete_native_call(stack, runtime);
                    let frame = stack.current(runtime).unwrap();
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
                            frame.read_slot(runtime, dst).unwrap_err().kind(),
                            RuntimeErrorKind::ModuleValidation
                        );
                    } else {
                        assert!(matches!(result, Ok(RegionExit::Safepoint)));
                        assert_eq!(frame.read_slot(runtime, dst).unwrap().as_i32(), Some(7));
                    }
                }
                assert_eq!(calls.load(Ordering::Relaxed), 2);
                if !fail_second {
                    let RegionExit::Return(value) = stack
                        .execute_region(runtime, &mut None, |_, _, _, _, _| unreachable!())
                        .unwrap()
                    else {
                        panic!("native caller returns");
                    };
                    assert_eq!(value.payload(), Some((ValueType::I32, 15)));
                }

                Ok(())
            })
            .unwrap();
        drop(stack_activation);
        assert_eq!(runtime.resources().counters().current_call_depth, 0);
        assert_eq!(runtime.gc_stats().root_registrations, roots_before);
    }
}

#[test]
fn native_without_destination_finishes_once_before_the_successor() {
    let (mut runtime, loaded, calls) = native_completion_fixture(false, true);
    let entry = loaded
        .bytecode
        .functions
        .iter()
        .find(|function| function.name == "main")
        .unwrap()
        .id;
    let roots_before = runtime.gc_stats().root_registrations;
    let mut stack_activation = runtime.enter_execution_stack(&loaded).unwrap();
    stack_activation
        .with(|runtime, stack| {
            stack
                .push(runtime, loaded.slot(), entry, &[], None)
                .unwrap();
            assert!(matches!(
                complete_native_call(stack, runtime),
                Ok(RegionExit::Safepoint)
            ));
            {
                let frame = stack.current(runtime).unwrap();
                assert!(matches!(
                    frame.function().unwrap().instructions[frame.instruction_offset()],
                    BytecodeInstruction::Call { dst: None, .. }
                ));
            }
            let RegionExit::Return(value) = stack
                .execute_region(runtime, &mut None, |_, _, _, _, _| unreachable!())
                .unwrap()
            else {
                panic!("discarding caller returns");
            };
            assert_eq!(value.payload(), Some((ValueType::Unit, 0)));
            assert_eq!(calls.load(Ordering::Relaxed), 1);

            Ok(())
        })
        .unwrap();
    drop(stack_activation);
    assert_eq!(runtime.resources().counters().current_call_depth, 0);
    assert_eq!(runtime.gc_stats().root_registrations, roots_before);
}

fn complete_native_call(
    stack: &FrameStack,
    runtime: &mut Runtime,
) -> Result<RegionExit, RegionError> {
    assert!(matches!(
        stack.execute_region(runtime, &mut None, |_, _, _, _, _| unreachable!())?,
        RegionExit::Call(ExecutionAction::NativeEntry)
    ));
    if let Err(error) = stack.start_native_entry(runtime, |_, _, _, _, _| unreachable!()) {
        stack.pop(runtime)?;
        return Err(error.into());
    }
    assert!(matches!(
        stack.finish_native_return(runtime)?,
        ReturnAction::Continue(ExecutionAction::Script)
    ));
    Ok(RegionExit::Safepoint)
}
