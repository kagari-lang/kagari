use crate::tests::execution::{
    common::{
        compile_logging_test_bytecode, logging_module, round_trip_bytecode, standard_runtime,
    },
    native_fixtures,
};

#[cfg(test)]
use crate::tests::support::bytecode::load_bytecode;
use kagari_runtime::{
    RuntimeConfig,
    debug::{DebugPauseReason, DebugSession, SourceBreakpoint},
    execution::reentry::reenter,
    module::entry::ProgramEntry,
    value::view::ValueView,
};

use kagari_bytecode::instruction::{BytecodeInstruction, CallTarget};
use kagari_types::declaration::logging;

#[test]
fn nested_breakpoints_and_traps_include_the_suspended_host_caller() {
    for encoded in [false, true] {
        for jit in [false, true] {
            let source = include_str!("../../../tests/fixtures/execution/debug-reentry.kgr");
            let module = compile_logging_test_bytecode(source);
            let nested = module.records().modules[module.records().root.index()]
                .functions
                .iter()
                .find(|f| f.name == "nested")
                .unwrap()
                .id;
            let caller_offset = module.records().modules[module.records().root.index()]
                .functions
                .iter()
                .find(|f| f.name == "main")
                .unwrap()
                .instructions
                .iter()
                .position(|i| {
                    matches!(
                        i,
                        BytecodeInstruction::Call {
                            callee: CallTarget::Native(id),
                            ..
                        } if module.definitions().resolve(
                            module.records().modules[module.records().root.index()].native_imports[id.index()].binding
                        ).unwrap().module() == &logging::declaration().unwrap().records().identity
                    )
                })
                .unwrap();
            let mut program = module;
            if encoded {
                program = round_trip_bytecode(program);
            }
            let mut runtime = standard_runtime(RuntimeConfig {
                ..Default::default()
            });
            logging_module(move |context, _| {
                let root = context.runtime().execution_root().unwrap();
                let args = context
                    .retain_temporaries(&[ValueView::from(2i32)])
                    .unwrap();
                assert!(reenter(context, &root, nested, args).is_err());
                context.runtime_mut().collect_garbage().unwrap();
                Ok(ValueView::unit())
            })
            .install(&mut runtime)
            .unwrap();
            let loaded = load_bytecode(&mut runtime, "debug-reentry.kgr", program).unwrap();
            let _loaded_program = loaded;
            let loaded = runtime.select_program(&_loaded_program).unwrap();
            let mut debug = DebugSession::new(&runtime).unwrap();
            let breakpoint = debug
                .add_breakpoint(SourceBreakpoint::at_source_offset(
                    "debug-reentry.kgr",
                    source.find("doubled +").unwrap(),
                ))
                .unwrap();
            let mut vm = runtime;
            vm.attach_debug_session(debug).unwrap();
            let prepared = native_fixtures::unsupported();
            let report = if jit {
                vm.execute_prepared_module(
                    &loaded,
                    ProgramEntry::Main,
                    &prepared,
                    vm.execution_options(),
                )
            } else {
                vm.execute_module(&loaded, ProgramEntry::Main, vm.execution_options())
            }
            .unwrap();
            assert_eq!(report.return_value.as_i32(), Some(42));
            let debug = vm.debug_session().unwrap();
            for reason in [
                DebugPauseReason::Breakpoint(breakpoint),
                DebugPauseReason::Trap,
            ] {
                let pause = debug
                    .pauses()
                    .iter()
                    .find(|p| p.reason == reason)
                    .expect("nested pause");
                assert_eq!(pause.frames.len(), 3);
                assert_eq!(pause.frames[1].function_name, "log");
                assert_eq!(pause.frames[0].function_name, "main");
                assert_eq!(pause.frames[0].instruction_offset, caller_offset);
                assert_eq!(pause.frames[2].function_name, "nested");
                assert!(
                    pause.frames[2]
                        .bindings
                        .iter()
                        .any(|binding| binding.name == "doubled"
                            && binding.value.as_i32() == Some(4))
                );
                assert_eq!(pause.frames[2].epoch, loaded.epoch.0);
            }
            assert_eq!(vm.resources().counters().current_call_depth, 0);
            assert!(vm.execution_root().is_none());
        }
    }
}
