use crate::{Vm, VmError, tests::common::compile_test_bytecode};
use kagari_ir::bytecode::{BytecodeProgram, KbcArtifact, ModuleRef};
use kagari_runtime::{
    CapabilitySet, LanguageProfile, ResourcePolicy, Runtime, RuntimeConfig, RuntimeErrorKind,
    SecurityContext,
};

#[test]
fn standard_mutation_resource_failures_match_across_execution_routes() {
    for (source, limit) in [
        (
            "fn main() -> i32 { val a = [1]; a.push(2); a.push(3); 0 }",
            3,
        ),
        ("fn main() -> i32 { val a = [1]; a.pop(); 0 }", 2),
    ] {
        let module = compile_test_bytecode(source);
        for encoded in [false, true] {
            for jit in [false, true] {
                for heap_limit in [false, true] {
                    let mut runtime = Runtime::new(RuntimeConfig {
                        resources: ResourcePolicy {
                            max_heap_units: heap_limit.then_some(limit),
                            max_allocation_units: (!heap_limit).then_some(limit),
                            ..Default::default()
                        },
                        security: SecurityContext {
                            profile: LanguageProfile {
                                allow_jit: true,
                                ..Default::default()
                            },
                            capabilities: CapabilitySet {
                                jit: true,
                                ..Default::default()
                            },
                        },
                        ..Default::default()
                    });
                    let program = BytecodeProgram {
                        root: ModuleRef::new(0),
                        modules: vec![module.clone()],
                    };
                    let program = if encoded {
                        let artifact = KbcArtifact::from_program(program, Default::default());
                        let decoded =
                            KbcArtifact::from_bytes(&artifact.to_bytes().unwrap()).unwrap();
                        decoded.validate_for_loader(&Default::default()).unwrap();
                        decoded.program
                    } else {
                        program
                    };
                    let loaded = runtime.load_program("mutation.kgr", program).unwrap();
                    let mut vm = Vm::new(runtime);
                    let error = if jit {
                        vm.execute_with_backend(
                            &loaded,
                            "main",
                            &mut kagari_jit_cranelift::CraneliftBackend::for_host().unwrap(),
                        )
                        .unwrap_err()
                    } else {
                        vm.execute(&loaded, "main").unwrap_err()
                    };
                    let VmError::RuntimeError(error) = error else {
                        panic!("structured resource failure: {error:?}")
                    };
                    assert_eq!(error.kind(), RuntimeErrorKind::ResourceLimitExceeded);
                    assert!(error.message().contains(if heap_limit {
                        "heap units"
                    } else {
                        "allocation units"
                    }));
                    let counters = vm.runtime().resources().counters();
                    assert_eq!(counters.allocation_units, limit);
                    assert_eq!(counters.current_heap_units, limit);
                    assert_eq!(counters.peak_heap_units, limit);
                    assert_eq!(counters.current_call_depth, 0);
                    assert_eq!(vm.runtime().gc().active_roots(), 0);
                    vm.runtime().collect_garbage().unwrap();
                    assert_eq!(vm.runtime().resources().counters().current_heap_units, 0);
                    assert_eq!(vm.runtime().resources().counters().allocation_units, limit);
                }
            }
        }
    }
}
