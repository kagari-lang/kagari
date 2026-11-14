//! Region results remain inspectable only after successful publication and released borrows.
#[cfg(test)]
use crate::test_support::bytecode::load_bytecode;
use crate::{
    Runtime,
    error::{
        RuntimeError, RuntimeErrorKind, fault::EngineFault, reason::ErrorReason, trap::TrapReason,
    },
    executor::native,
    frame::{
        FrameStack,
        cursor::kernel::{RegionError, RegionExit},
        execution_frames::loaded,
    },
    module::ExecutableModule,
    native::{
        binding::{Codec, NativeBinding, NativeResult},
        builder::ModuleBuilder,
        catalog::DeclarationCatalog,
        context::NativeCall,
        declarations::FunctionDecl,
        types::Type,
    },
    value::view::ValueView,
};

use kagari_abi::representation::ValueType;
use kagari_bytecode::{
    instruction::{BytecodeInstruction, CallTarget},
    program::updates,
};
use kagari_common::identity::metadata::DefinitionMetadata;
use kagari_compiler::{bytecode::lower_program_to_bytecode, source::program::lower_program_to_mir};
use kagari_contract::ids::FunctionRef;
use kagari_hir::{analysis::AnalysisDatabase, compile_input::program::CheckedCompilation};
use kagari_source::source_database::{SourceDatabase, SourceLayer};
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

#[test]
fn region_rejects_a_suspended_scope_before_advancing_its_parent() {
    let mut runtime = Runtime::default();
    let (_program, owner) = loaded(&mut runtime);
    let mut outer = runtime.enter_execution_stack(&owner).unwrap();
    outer
        .with(|runtime, parent| {
            parent.push(runtime, owner.slot(), FunctionRef::new(0), &[], None)?;
            let mut inner = runtime.enter_execution_stack(&owner)?;
            inner.with(|runtime, _| {
                let error = parent
                    .execute_region(runtime, &mut None, native::invoke_script)
                    .map(|_| ())
                    .unwrap_err();
                let error = RuntimeError::from(error);
                assert!(matches!(
                    error.reason(),
                    ErrorReason::EngineFault(EngineFault::ExecutionUsedASuspendedFrameScope)
                ));
                assert_eq!(parent.frames(runtime)?[0].instruction_offset(), 0);
                assert!(runtime.is_quarantined());
                Ok(())
            })?;
            Ok(())
        })
        .unwrap();
    drop(outer);
    assert_eq!(runtime.resources().counters().current_call_depth, 0);
    assert!(runtime.execution_root().is_none());
}

fn assert_logical_native_entry(runtime: &Runtime) {
    let session = runtime.resources().active_session().unwrap().id;
    assert_eq!(
        runtime.resources().sessions.frames(session).unwrap().len(),
        1
    );
    assert_eq!(runtime.resources().counters().current_call_depth, 2);
    let trace = runtime.capture_error_trace();
    assert!(!trace.incomplete);
    assert_eq!(trace.frames.len(), 2);
    assert_eq!(trace.frames[0].function_name, "value");
    assert_eq!(trace.frames[1].function_name, "main");
}

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
                assert_logical_native_entry(cx.runtime);
                cx.runtime.collect_garbage()?;
                invoked.fetch_add(1, Ordering::Relaxed);
                Ok(())
            })
            .unwrap();
    } else {
        native
            .bind(
                value,
                NativeBinding::scoped([], Codec::Exact(Type::i32()), move |cx| {
                    assert_logical_native_entry(cx.runtime);
                    cx.runtime.collect_garbage()?;
                    let fail = invoked.fetch_add(1, Ordering::Relaxed) == 1 && fail_second;
                    cx.set_result(ValueView::from(7i32))?;
                    // Scratch shares the enclosing completion. Collection and a
                    // rejected second setter cannot replace that staged value.
                    cx.with_temporaries(|temporary| {
                        temporary.collect_garbage()?;
                        let error = temporary.set_result(ValueView::unit()).unwrap_err();
                        assert_eq!(error.kind(), RuntimeErrorKind::ModuleValidation);
                        assert!(error.to_string().contains("native result already staged"));
                        assert_eq!(temporary.result()?.as_i32(), Some(7));
                        Ok(())
                    })?;
                    if fail {
                        Err(RuntimeError::from(TrapReason::Provider(
                            "native failure".into(),
                        )))
                    } else {
                        Ok(())
                    }
                }),
            )
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
    runtime.collect_garbage().unwrap();
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
                        let Err(RegionError::Runtime(error)) = &result else {
                            unreachable!()
                        };
                        assert_eq!(error.trace().unwrap().frames[0].function_name, "value");
                        assert_eq!(error.trace().unwrap().frames[1].function_name, "main");
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
    stack.execute_region(runtime, &mut None, |_, _, _, _, _| unreachable!())
}
