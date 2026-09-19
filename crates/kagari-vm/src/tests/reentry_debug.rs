use crate::{
    DebugPauseReason, DebugSession, SourceBreakpoint, Vm, tests::common::compile_test_bytecode,
};
use kagari_common::host_interface::standard_log;
use kagari_ir::bytecode::{
    BytecodeInstruction, BytecodeProgram, CallTarget, KbcArtifact, ModuleRef,
};
use kagari_runtime::{
    CapabilitySet, DebugVisibilityPolicy, HostExposurePolicy, LanguageProfile, Runtime,
    RuntimeConfig, SecurityContext, host::HostFunction, value::Value,
};

#[test]
fn nested_breakpoints_and_traps_include_the_suspended_host_caller() {
    for encoded in [false, true] {
        for jit in [false, true] {
            let source = "fn main() -> i32 { val kept = [42]; print(\"enter\"); kept[0] } fn nested(n: i32) -> i32 { val doubled = n + n; doubled + 2147483647 }";
            let module = compile_test_bytecode(source);
            let nested = module
                .functions
                .iter()
                .find(|f| f.name == "nested")
                .unwrap()
                .id;
            let caller_offset = module
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
                            callee: CallTarget::HostFunction(_),
                            ..
                        }
                    )
                })
                .unwrap();
            let mut program = BytecodeProgram {
                root: ModuleRef::new(0),
                modules: vec![module],
            };
            if encoded {
                program = KbcArtifact::from_bytes(
                    &KbcArtifact::from_program(program, Default::default())
                        .unwrap()
                        .to_bytes()
                        .unwrap(),
                )
                .unwrap()
                .program;
            }
            let mut runtime = Runtime::new(RuntimeConfig {
                security: SecurityContext {
                    profile: LanguageProfile {
                        allow_debugger: true,
                        allow_host_calls: true,
                        allow_jit: true,
                        ..Default::default()
                    },
                    capabilities: CapabilitySet {
                        debug_attach: true,
                        debug_breakpoints: true,
                        debug_pause: true,
                        debug_stack_inspection: true,
                        debug_value_inspection: true,
                        host_calls: true,
                        jit: true,
                        ..Default::default()
                    },
                },
                debug_visibility: DebugVisibilityPolicy {
                    visible_modules: vec!["debug-reentry.kgr".into()],
                    ..Default::default()
                },
                host_exposure: HostExposurePolicy {
                    allowed_host_functions: vec!["host.log".into()],
                    ..Default::default()
                },
                ..Default::default()
            });
            runtime
                .register_host_function(HostFunction::new(standard_log(), move |context, _| {
                    let root = context.runtime().execution_root().unwrap();
                    assert!(crate::reenter(context, &root, nested, &[Value::I32(2)]).is_err());
                    context.runtime().collect_garbage().unwrap();
                    Ok(Value::Unit)
                }))
                .unwrap();
            let loaded = runtime.load_program("debug-reentry.kgr", program).unwrap();
            let mut debug = DebugSession::new(&runtime).unwrap();
            let breakpoint = debug
                .add_breakpoint(SourceBreakpoint::at_source_offset(
                    "debug-reentry.kgr",
                    source.find("doubled +").unwrap(),
                ))
                .unwrap();
            let mut vm = Vm::new(runtime);
            vm.attach_debug_session(debug).unwrap();
            let mut backend = kagari_jit_cranelift::CraneliftBackend::for_host().unwrap();
            let report = if jit {
                vm.execute_with_backend(&loaded, "main", &mut backend)
            } else {
                vm.execute(&loaded, "main")
            }
            .unwrap();
            assert_eq!(report.return_value, Value::I32(42));
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
                assert_eq!(pause.frames.len(), 2);
                assert_eq!(pause.frames[0].function_name, "main");
                assert_eq!(pause.frames[0].instruction_offset, caller_offset);
                assert_eq!(pause.frames[1].function_name, "nested");
                assert!(
                    pause.frames[1]
                        .bindings
                        .iter()
                        .any(|binding| binding.name == "doubled" && binding.value == Value::I32(4))
                );
                assert_eq!(pause.frames[1].epoch, loaded.epoch.0);
            }
            assert_eq!(vm.runtime().resources().counters().current_call_depth, 0);
            assert!(vm.runtime().execution_root().is_none());
        }
    }
}
