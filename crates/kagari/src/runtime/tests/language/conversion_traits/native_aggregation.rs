use crate::{context::ExecutionContext, engine::KagariEngine, program::PreparedProgram};
use kagari_runtime::module::entry::ProgramEntry;
use kagari_source::source::SourceFile;

#[test]
fn native_numeric_aggregation_checks_declared_width_and_releases_roots() {
    let engine = KagariEngine::builder().unwrap().build().unwrap();
    for source in [
        "fn main()->i8 {[127i8,1i8].sum()}",
        "fn main()->u8 {[128u8,2u8].product()}",
        "fn main()->i8 {Vec::from([127i8,1i8]).sum()}",
        "fn main()->u8 {Vec::from([128u8,2u8]).product()}",
        "fn main()->i8 {Vec::from([127i8,1i8]).map(|x| x).sum()}",
        "fn main()->u8 {Vec::from([128u8,2u8]).map(|x| x).product()}",
    ] {
        let artifact = engine
            .compile_to_artifact(SourceFile::new("overflow.kgr", source), Default::default())
            .unwrap();
        let program =
            PreparedProgram::from_artifact(artifact, &Default::default(), &Default::default())
                .unwrap();
        let context = ExecutionContext::default();
        let mut runtime = engine.runtime(context.clone());
        let loaded = runtime.load_program(&program, Default::default()).unwrap();
        let error = runtime
            .execute(&loaded, ProgramEntry::Main, &[], &context)
            .unwrap_err();
        assert!(
            format!("{error:?}").contains("integer overflow"),
            "{error:?}"
        );
        assert_eq!(runtime.runtime().gc().active_roots(), 0);
    }
}
