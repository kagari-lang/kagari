//! Compile against declarations without registering callbacks or starting services.
use kagari_common::{
    SourceFile,
    host_interface::{
        HostFunctionDeclaration, HostInterface, HostParameter, HostPassingStyle, HostValueType,
    },
};
use kagari_embed::{ArtifactOptions, CompileOptions, KagariEngine};
use kagari_runtime::LanguageProfile;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let declarations = HostInterface {
        functions: vec![HostFunctionDeclaration::new(
            "demo.echo",
            vec![HostParameter {
                name: "value".into(),
                ty: HostValueType::I32,
                passing: HostPassingStyle::Owned,
            }],
            HostValueType::I32,
        )],
    };
    // A build process may read these bytes from the binding provider's interface file.
    let offline_bytes = declarations.to_bytes()?;
    let engine = KagariEngine::default();
    engine.set_host_interface(HostInterface::from_bytes(&offline_bytes)?)?;
    let artifact = engine
        .compile_to_artifact(
            SourceFile::new(
                "offline.kgr",
                "use demo::echo; fn main() -> i32 { echo(42) }",
            ),
            CompileOptions {
                language_profile: LanguageProfile {
                    allow_host_calls: true,
                    ..Default::default()
                },
            },
            ArtifactOptions::default(),
        )
        .expect("offline source should compile against its declaration");
    println!(
        "compiled {} required host function; artifact is {} bytes",
        artifact.program.modules[artifact.program.root.index()]
            .host_interface
            .functions
            .len(),
        artifact.to_bytes()?.len()
    );
    Ok(())
}
