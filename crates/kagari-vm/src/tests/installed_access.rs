use crate::{
    debug::{DebugSession, SourceBreakpoint},
    tests::common::standard_runtime,
};

use kagari_runtime::RuntimeConfig;

#[test]
fn debugger_control_needs_no_permission_flags() {
    let debug_runtime = standard_runtime(RuntimeConfig {
        ..RuntimeConfig::default()
    });
    let mut session = DebugSession::new(&debug_runtime).expect("attach should be allowed");
    session
        .add_breakpoint(SourceBreakpoint::at_source_offset("debug.kgr", 0))
        .expect("breakpoints should be allowed");
    session.step_into().unwrap();
}
