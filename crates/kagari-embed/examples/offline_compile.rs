//! Compile against declarations without registering callbacks or starting services.
use kagari_common::{
    host_interface::{
        HostFunctionDeclaration, HostInterface, HostParameter, HostPassingStyle, HostValueType,
    },
    identity::{ModuleIdentity, PackageId},
    source_database::SourceLayer,
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
    let mut root = None;
    for (name, text) in [
        (
            "api",
            "pub use demo::echo as echo; pub use demo as service;",
        ),
        (
            "main",
            "use build::api::echo; use build::api as api; fn main() -> i32 { echo(api::service::echo(42)) }",
        ),
    ] {
        let path = format!("mem://{name}");
        engine
            .bind_module(
                &path,
                ModuleIdentity {
                    package: PackageId("build".into()),
                    path: vec![name.into()],
                },
            )
            .expect("bind the example module");
        root = Some(
            engine
                .set_source(&path, text.into(), SourceLayer::Base)
                .expect("register example source"),
        );
    }
    let checked = engine
        .compile_snapshot(
            engine.source_snapshot(),
            root.expect("entry source"),
            CompileOptions {
                language_profile: LanguageProfile {
                    allow_host_calls: true,
                    ..Default::default()
                },
            },
            &Default::default(),
        )
        .expect("offline source should compile against its declaration");
    let artifact = engine
        .emit_bytecode(&checked, ArtifactOptions::default())
        .expect("emit checked program");
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
