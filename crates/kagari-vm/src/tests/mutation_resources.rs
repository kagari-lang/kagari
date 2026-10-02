use crate::{
    error::VmError,
    tests::{common::compile_test_bytecode, native_fixtures},
    vm::Vm,
};
use kagari_bytecode::artifact::KbcArtifact;
use kagari_runtime::{Runtime, RuntimeConfig, error::RuntimeErrorKind, resource::RuntimeLimits};

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
                        limits: RuntimeLimits {
                            max_heap_units: heap_limit.then_some(limit),
                            max_allocation_units: (!heap_limit).then_some(limit),
                            ..Default::default()
                        },

                        ..Default::default()
                    });
                    let program = module.clone();
                    let program = if encoded {
                        let artifact =
                            KbcArtifact::from_program(program, Default::default()).unwrap();
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
                        vm.execute_prepared(&loaded, "main", &native_fixtures::unsupported())
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
