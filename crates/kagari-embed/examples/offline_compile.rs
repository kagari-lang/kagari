//! Compile against declarations without registering callbacks or starting services.
use kagari_common::{
    host_interface::{
        HostFieldDeclaration, HostFunctionDeclaration, HostInterface, HostMethodDeclaration,
        HostParameter, HostPassingStyle, HostTypeDeclaration, HostTypeOwnership, HostValueType,
        PathAccess,
    },
    identity::{ModuleIdentity, PackageId},
    source_database::SourceLayer,
};
use kagari_embed::{ArtifactOptions, CompileOptions, KagariEngine};
use kagari_runtime::LanguageProfile;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut player = HostTypeDeclaration::new("demo.Player");
    player.ownership = HostTypeOwnership::HostRoot;
    player.path_access = PathAccess::ReadWrite;
    player.fields.push(HostFieldDeclaration::new(
        &player.id,
        "score",
        HostValueType::I32,
    ));
    player.fields[0].path_access = PathAccess::ReadWrite;
    player.fields[0].writable = true;
    player.documentation =
        "Host-owned player metadata, available without business services.".into();
    player.methods.push(HostMethodDeclaration::new(
        &player.id,
        "read_score",
        vec![],
        HostValueType::I32,
    ));
    let path_declaration = kagari_common::host_interface::HostFieldPathDeclaration {
        root: player.id.clone(),
        fields: vec![player.fields[0].id.clone()],
        access: PathAccess::ReadWrite,
        schema_epoch: 0,
        capabilities: Default::default(),
    };
    let declarations = HostInterface {
        field_paths: vec![path_declaration],
        types: vec![player],
        functions: vec![HostFunctionDeclaration::new(
            "demo.echo",
            vec![HostParameter {
                name: "value".into(),
                ty: HostValueType::Array(Box::new(HostValueType::I32)),
                passing: HostPassingStyle::Owned,
            }],
            HostValueType::Array(Box::new(HostValueType::I32)),
        )],
    };
    // A build process may read these bytes from the binding provider's interface file.
    let offline_bytes = declarations.to_bytes()?;
    let offline = HostInterface::from_bytes(&offline_bytes)?;
    let path = offline.field_paths[0].contract(&offline)?;
    println!(
        "offline field path fingerprint: {:016x}",
        path.fingerprint()?
    );
    println!(
        "offline member: {}.{}",
        offline.types[0].symbol, offline.types[0].fields[0].name
    );
    let engine = KagariEngine::default();
    engine.set_host_interface(offline)?;
    let mut root = None;
    for (name, text) in [
        (
            "api",
            "pub use demo::echo as echo; pub use demo::Player; pub use demo as service;",
        ),
        (
            "main",
            "use build::api::echo; use build::api as api; pub fn direct_set(value: api::Player, next: i32) { value.score = next; } pub fn add_score(value: api::Player, amount: i32) { value.score += amount; } pub fn direct_score(value: api::Player) -> i32 { value.score } pub fn score(value: api::Player) -> i32 { value.read_score() } pub fn pass(value: api::Player) -> api::service::Player { value } fn main() -> [i32] { echo(api::service::echo([42])) }",
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
    let query = engine
        .analyze(
            engine.source_snapshot(),
            LanguageProfile {
                allow_host_calls: true,
                allow_path_mutation: true,
                ..Default::default()
            },
            &Default::default(),
        )
        .expect("offline source should be queryable");
    let source = query
        .file(root.expect("entry source"))
        .expect("query source");
    let text = source.source().text();
    let field = text.find("value.score = next").expect("field write");
    assert!(source.host_field_at(field).is_none());
    assert!(source.host_field_at(field + "value".len()).is_none());
    assert_eq!(
        source
            .host_field_at(field + "value.".len())
            .map(|member| member.name.as_str()),
        Some("score")
    );
    println!("offline field navigation selects the member name");
    let checked = engine
        .compile_snapshot(
            engine.source_snapshot(),
            root.expect("entry source"),
            CompileOptions {
                language_profile: LanguageProfile {
                    allow_host_calls: true,
                    allow_path_mutation: true,
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
    let required = &artifact.program.modules[artifact.program.root.index()].host_interface;
    assert_eq!(required.types.len(), 1);
    assert_eq!(required.field_paths.len(), 1);
    println!(
        "public signature requires {} without registering a runtime",
        required.types[0].symbol
    );
    Ok(())
}
