//! Verified authoring input for the immutable core sources bundled with this frontend.
use crate::{bytecode::lower_program_to_bytecode, source::program::lower_program_to_mir};
use kagari_bytecode::program::verified::VerifiedBytecodeProgram;
use kagari_hir::{
    analysis::{AnalysisDatabase, core_source::packaged_modules},
    compile_input::program::CheckedCompilation,
};
use std::sync::LazyLock;

/// Compile the packaged core in isolation, with no application inputs or Native providers.
/// All builders in this process share one verified product and hence one authoring origin.
/// Failures are terminal for these immutable resources; no partial program is published.
pub fn verified() -> Result<&'static VerifiedBytecodeProgram, &'static str> {
    static CORE: LazyLock<Result<VerifiedBytecodeProgram, String>> = LazyLock::new(compile);
    CORE.as_ref().map_err(String::as_str)
}

fn compile() -> Result<VerifiedBytecodeProgram, String> {
    let modules = packaged_modules();
    // A checked package's module closure includes its package siblings. Use an actual
    // packaged module as the root, not an artificial entry function or a Native facade.
    let root = modules.first().ok_or("empty packaged core")?.id();
    let cancel = Default::default();
    let mut db = AnalysisDatabase::new("/").map_err(|error| error.to_string())?;

    let snapshot = db
        .snapshot(Default::default(), &cancel)
        .map_err(|e| format!("analyze packaged core: {e:?}"))?;
    let checked = snapshot
        .check_program(root, &cancel)
        .map_err(|e| format!("check packaged core: {e:?}"))?;
    let input = CheckedCompilation::from_program(&checked, &cancel)
        .map_err(|e| format!("admit packaged core: {e:?}"))?;
    let mir = lower_program_to_mir(&input, &Default::default())
        .map_err(|e| format!("lower packaged core: {e:?}"))?;
    lower_program_to_bytecode(&mir, &cancel)
        .map_err(|e| format!("verify packaged core bytecode: {e:?}"))
}
