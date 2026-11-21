//! Regenerate the verified core input used by downstream tests without a compiler dependency.
use kagari_bytecode::artifact::KbcArtifact;
use kagari_compiler::source::core_library;
use std::{env, fs};

fn main() {
    let path = env::args_os().nth(1).expect("output .kbc path");
    let program = core_library::verified().expect("checked packaged core");
    let artifact =
        KbcArtifact::from_verified(program.clone(), Default::default(), &Default::default())
            .expect("export core");
    let bytes = artifact
        .records()
        .to_bytes(artifact.definitions())
        .expect("encode core");
    fs::write(path, bytes).expect("write core fixture");
}
