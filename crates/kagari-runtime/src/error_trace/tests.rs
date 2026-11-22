use crate::interpreter_tests::common::{load_bytecode_program, round_trip_bytecode};
use kagari_bytecode::{
    instruction::{BytecodeInstruction, CallTarget},
    module::CallableTarget,
    program::verify_program,
    verifier::BytecodeVerificationError,
};
use kagari_compiler::{bytecode::lower_program_to_bytecode, source::program::lower_program_to_mir};
use kagari_contract::native_import::binding_id;
use kagari_hir::{analysis::AnalysisDatabase, compile_input::program::CheckedCompilation};
use kagari_source::source_database::{SourceDatabase, SourceLayer};
use kagari_types::language::binding::result_declaration;

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
    let program = decoded.records();
    let definitions = decoded.definitions();
    let path = binding_id(&result_declaration().module, "forward_err_origin");
    let binding = definitions
        .lookup(&path)
        .expect("core Result helper identity");
    let owner_index = program
        .modules
        .iter()
        .position(|module| module.identity == path.module)
        .expect("core Result module");
    let owner = &program.modules[owner_index];
    let (function_index, dst, import) = owner
        .functions
        .iter()
        .enumerate()
        .find_map(|(index, function)| {
            function.instructions.iter().find_map(|instruction| {
                let BytecodeInstruction::Call {
                    dst: Some(dst),
                    callee,
                    ..
                } = instruction
                else {
                    return None;
                };
                let id = match callee {
                    CallTarget::Native(id) => id,
                    CallTarget::Shared {
                        module,
                        target: CallableTarget::Native(id),
                        ..
                    } if module.index() == owner_index => id,
                    _ => return None,
                };
                let import = &owner.native_imports[id.index()];
                (import.instance.declaration == binding && import.binding == binding)
                    .then_some((index, *dst, import))
            })
        })
        .expect("core Result origin transfer call");

    // A copied, otherwise valid import cannot grant access to the user module.
    let mut foreign = program.clone();
    foreign.modules[program.root.index()]
        .native_imports
        .push(import.clone());
    assert!(matches!(
        verify_program(&foreign, definitions, &cancel),
        Err(BytecodeVerificationError::InvalidHostInterface(_))
    ));

    let mut missing_fact = program.clone();
    let function = &mut missing_fact.modules[owner_index].functions[function_index];
    assert!(
        function
            .metadata
            .semantic
            .slots
            .remove(&dst.index())
            .is_some()
    );
    assert!(matches!(
        verify_program(&missing_fact, definitions, &cancel),
        Err(BytecodeVerificationError::InvalidOperation {
            reason: "invalid foundation native call types",
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
