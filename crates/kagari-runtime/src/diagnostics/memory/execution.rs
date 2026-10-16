//! Separate executable preparation from runtime linking before changing call plans.
use super::compile;
use crate::{
    Runtime,
    diagnostics::{self, allocations},
    module::VerifiedProgram,
};
use kagari_bytecode::program::BytecodeProgram;
use kagari_common::identity::{map::DefinitionContext, metadata::DefinitionMetadata};

const SOURCE: &str = r#"
fn identity(value: i32) -> i32 { value }
trait Forward { fn forward<T>(self, value: T) -> T { value } }
impl Forward for i32 {}
pub fn calls(left: i32, right: i32) -> i32 {
    val receiver: Forward = 0;
    val closure = |value: i32| identity(value);
    identity(left) + identity(right) + closure(left + right) + receiver.forward(left)
}
"#;

fn lifecycle(
    code: &DefinitionMetadata<BytecodeProgram>,
) -> (
    [allocations::Counts; 3],
    diagnostics::ExecutionCounts,
    allocations::Counts,
) {
    let ((phases, execution), total) = allocations::measure(|| {
        diagnostics::measure(|| {
            let context = DefinitionContext::new().unwrap();
            let input = allocations::current().unwrap();
            // Clone inside accounting so consumed input storage is balanced at teardown.
            // Source compilation remains outside all three allocation phases.
            let prepared = VerifiedProgram::new(code.clone(), &context).unwrap();
            let executable = allocations::current().unwrap();
            let mut runtime = Runtime::default();
            let _loaded = runtime
                .load_verified_program("call-storage", prepared)
                .unwrap();
            let linked = allocations::current().unwrap();
            [input, executable, linked]
        })
    });
    (phases, execution, total)
}

#[test]
#[ignore = "manual executable/metadata allocation accounting, not throughput"]
fn prepared_execution_retention() {
    let code = compile(SOURCE, None);
    lifecycle(&code);
    let (phases, execution, total) = lifecycle(&code);
    assert_eq!(
        total.net_bytes, 0,
        "warmed prepared code and runtime teardown"
    );
    println!(
        "EXECUTABLE_COUNTS,prepared_net_bytes={},linked_net_bytes={},phases={phases:?},execution={execution:?},total={total:?}",
        phases[1].net_bytes - phases[0].net_bytes,
        phases[2].net_bytes - phases[1].net_bytes,
    );
}
