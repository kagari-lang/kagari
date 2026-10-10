//! Invalid engine borrows remain a collector fault even though cursors cannot escape.
use crate::{Runtime, error::RuntimeErrorKind, value::Value};
use kagari_bytecode::instruction::Register;
use kagari_compiler::{bytecode::lower_program_to_bytecode, source::program::lower_program_to_mir};
use kagari_hir::analysis::AnalysisDatabase;
use kagari_source::source_database::{SourceDatabase, SourceLayer};

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
    let code = lower_program_to_bytecode(&mir).unwrap();
    let mut runtime = Runtime::default();
    let loaded = runtime.load_program("borrow", code).unwrap();
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
            .write_register(&runtime, Register::new(0), Value::I32(99))
            .is_err()
    );
    assert!(frame.read_register(&runtime, Register::new(0)).is_err());
    drop(frame);
    assert!(stack.execute_region(&runtime, &mut None).is_err());
    drop(stack);
    assert_eq!(runtime.gc().active_roots(), 0);
    assert_eq!(runtime.resources().counters().current_call_depth, 0);
}
