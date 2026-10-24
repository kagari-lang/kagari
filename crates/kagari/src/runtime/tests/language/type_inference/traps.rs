use crate::{context::ExecutionContext, engine::KagariEngine, program::PreparedProgram};
use kagari_runtime::module::entry::ProgramEntry;
use kagari_source::source::SourceFile;

#[test]
fn narrow_and_unsigned_arithmetic_trap_on_overflow() {
    for expression in [
        "255u8 + 1u8",
        "127i8 + 1i8",
        "18446744073709551615u64 + 1u64",
        "0u64 - 1u64",
    ] {
        let engine = KagariEngine::default();
        let artifact = engine
            .compile_to_artifact(
                SourceFile::new(
                    "numeric-overflow.kgr",
                    format!("fn main() {{ val x = {expression}; }}"),
                ),
                Default::default(),
            )
            .unwrap();
        let context = ExecutionContext::default();
        let mut runtime = engine.runtime(context.clone());
        let loaded_program =
            PreparedProgram::from_artifact(artifact, &Default::default(), &Default::default())
                .unwrap();
        let loaded = runtime
            .load_program(&loaded_program, Default::default())
            .unwrap();
        let error = runtime
            .execute(&loaded, ProgramEntry::Main, &[], &context)
            .unwrap_err();
        assert!(
            format!("{error:?}").contains("overflow"),
            "{expression}: {error:?}"
        );
    }
}
