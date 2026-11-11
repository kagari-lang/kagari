//! Checked bytecode fixture preparation for standard-provider integration.
#[path = "../../../kagari-runtime/tests/support/bytecode.rs"]
pub(super) mod bytecode;
pub(super) mod native;

use kagari_runtime::{
    Runtime, execution::owned::DriveResult, gc::roots::RootedValue, module::ExecutableModule,
    value::Value,
};
use std::num::NonZeroUsize;

/// Drive a checked public script entry with retained arguments and result roots.
pub(super) fn invoke(
    runtime: &mut Runtime,
    module: &ExecutableModule,
    name: &str,
    arguments: &[Value],
) -> RootedValue {
    let execution = runtime
        .start(module, name, arguments, Default::default())
        .unwrap();
    loop {
        match runtime
            .drive(&execution, NonZeroUsize::new(1024).unwrap())
            .unwrap()
        {
            DriveResult::Runnable => {}
            DriveResult::Waiting => panic!("synchronous fixture unexpectedly waited"),
            DriveResult::Complete(result) => return result.unwrap(),
        }
    }
}
