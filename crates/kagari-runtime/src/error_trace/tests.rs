use crate::interpreter_tests::common::{load_bytecode_program, round_trip_bytecode};
use kagari_bytecode::{
    instruction::{BytecodeInstruction, CallTarget},
    program::verify_program,
    verifier::BytecodeVerificationError,
};
use kagari_compiler::{bytecode::lower_program_to_bytecode, source::program::lower_program_to_mir};
use kagari_contract::standard::RuntimePrimitive;
use kagari_hir::{analysis::AnalysisDatabase, compile_input::program::CheckedCompilation};
use kagari_source::source_database::{SourceDatabase, SourceLayer};

#[test]
fn packaged_result_preserves_only_propagated_origins() {
    let cancel = Default::default();
    let mut sources = SourceDatabase::default();
    let root = sources
        .set(
            "authored-result-origin.kgr",
            include_str!("../../tests/fixtures/execution/authored-result-origin.kgr").into(),
            SourceLayer::Base,
        )
        .unwrap();
    let mut db = AnalysisDatabase::default();
    let snapshot = db.snapshot(sources.snapshot(), &cancel).unwrap();
    let checked = snapshot.check_program(root, &cancel).unwrap();
    let input = CheckedCompilation::from_program(&checked, &cancel).unwrap();
    let mir = lower_program_to_mir(&input, &Default::default()).unwrap();
    let bytecode = lower_program_to_bytecode(&mir, &cancel).unwrap();
    let decoded = round_trip_bytecode(bytecode.into_unverified());
    // Physical and semantic operands match. The application still cannot mint a
    // trace transfer by replacing its own same-named function in decoded code.
    let mut forged = decoded.records().clone();
    let shadowed = forged.modules[forged.root.index()]
        .functions
        .iter_mut()
        .find(|f| f.name == "shadowed")
        .unwrap();
    let call = shadowed
        .instructions
        .iter_mut()
        .find_map(|i| match i {
            BytecodeInstruction::Call { callee, args, .. } if args.len() == 2 => Some(callee),
            _ => None,
        })
        .unwrap();
    *call = CallTarget::RuntimePrimitive(RuntimePrimitive::ForwardErrOrigin);
    assert!(matches!(
        verify_program(&forged, decoded.definitions(), &cancel),
        Err(BytecodeVerificationError::InvalidOperation {
            reason: "invalid Result origin transfer",
            ..
        })
    ));
    let (mut runtime, installed) = load_bytecode_program("authored-result-origin", decoded);
    let loaded = runtime.select_program(&installed).unwrap();
    let success = runtime
        .execute_module(&loaded, "success", runtime.execution_options())
        .unwrap();
    assert_eq!(success.return_value.as_bool(), Some(true));
    for (entry, expected) in [
        ("propagated", Some(("43", "origin"))),
        ("shadowed", Some(("42", "rebuilt"))),
        ("ordinary", None),
    ] {
        let report = runtime
            .execute_module(&loaded, entry, runtime.execution_options())
            .unwrap();
        assert_eq!(
            report.failure.as_ref().map(|failure| (
                failure.message.as_str(),
                failure.trace.frames[0].function_name.as_str()
            )),
            expected,
            "{entry}"
        );
    }
}
