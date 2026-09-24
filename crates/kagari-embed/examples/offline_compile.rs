//! Compile against declarations without registering callbacks or starting services.
use kagari_common::{
    host_interface::{
        HostFieldDeclaration, HostFunctionDeclaration, HostIndexSegmentDeclaration, HostInterface,
        HostMethodDeclaration, HostParameter, HostPassingStyle, HostPathDeclaration,
        HostPathSegmentDeclaration, HostTypeDeclaration, HostTypeOwnership, HostValueType,
        HostVirtualSegmentDeclaration, PathAccess,
    },
    identity::{ModuleIdentity, PackageId},
    source_database::SourceLayer,
};
use kagari_embed::{ArtifactOptions, CompileOptions, KagariEngine};
use kagari_ir::bytecode::{ArtifactSectionId, KBC_ARTIFACT_FORMAT_VERSION};
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
    let mut scores = HostFieldDeclaration::new(
        &player.id,
        "scores",
        HostValueType::Array(Box::new(HostValueType::I32)),
    );
    scores.path_access = PathAccess::ReadOnly;
    player.fields.push(scores);
    player.documentation =
        "Host-owned player metadata, available without business services.".into();
    player.methods.push(HostMethodDeclaration::new(
        &player.id,
        "read_score",
        vec![],
        HostValueType::I32,
    ));
    let mut child = HostTypeDeclaration::new("demo.Child");
    let mut child_score = HostFieldDeclaration::new(&child.id, "score", HostValueType::I32);
    child_score.path_access = PathAccess::ReadOnly;
    child.fields.push(child_score.clone());
    let path_declaration = kagari_common::host_interface::HostPathDeclaration {
        root: player.id.clone(),
        segments: vec![
            kagari_common::host_interface::HostPathSegmentDeclaration::Field(
                player.fields[0].id.clone(),
            ),
        ],
        access: PathAccess::ReadWrite,
        schema_epoch: 0,
        capabilities: Default::default(),
    };
    let index_declaration = HostPathDeclaration {
        root: player.id.clone(),
        segments: vec![HostPathSegmentDeclaration::Index(
            HostIndexSegmentDeclaration {
                slot: 0,
                collection: HostValueType::Opaque(player.id.clone()),
                index: HostValueType::I32,
                result: HostValueType::I32,
                access: PathAccess::ReadOnly,
            },
        )],
        access: PathAccess::ReadOnly,
        schema_epoch: 0,
        capabilities: Default::default(),
    };
    let field_index_declaration = HostPathDeclaration {
        root: player.id.clone(),
        segments: vec![
            HostPathSegmentDeclaration::Field(player.fields[1].id.clone()),
            HostPathSegmentDeclaration::Index(HostIndexSegmentDeclaration {
                slot: 0,
                collection: HostValueType::Array(Box::new(HostValueType::I32)),
                index: HostValueType::I32,
                result: HostValueType::I32,
                access: PathAccess::ReadOnly,
            }),
        ],
        access: PathAccess::ReadOnly,
        schema_epoch: 0,
        capabilities: Default::default(),
    };
    let nested_declaration = HostPathDeclaration {
        root: player.id.clone(),
        segments: vec![
            HostPathSegmentDeclaration::Index(HostIndexSegmentDeclaration {
                slot: 0,
                collection: HostValueType::Opaque(player.id.clone()),
                index: HostValueType::I32,
                result: HostValueType::Opaque(child.id.clone()),
                access: PathAccess::ReadOnly,
            }),
            HostPathSegmentDeclaration::Index(HostIndexSegmentDeclaration {
                slot: 1,
                collection: HostValueType::Opaque(child.id.clone()),
                index: HostValueType::I32,
                result: HostValueType::Opaque(child.id.clone()),
                access: PathAccess::ReadOnly,
            }),
            HostPathSegmentDeclaration::Virtual(HostVirtualSegmentDeclaration {
                name: "selected".into(),
                result: HostValueType::Opaque(child.id.clone()),
                access: PathAccess::ReadOnly,
            }),
            HostPathSegmentDeclaration::Field(child_score.id.clone()),
        ],
        access: PathAccess::ReadOnly,
        schema_epoch: 0,
        capabilities: Default::default(),
    };
    let declarations = HostInterface {
        paths: vec![
            path_declaration,
            index_declaration,
            field_index_declaration,
            nested_declaration,
        ],
        types: vec![player, child],
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
    let player_type = offline
        .types
        .iter()
        .find(|ty| ty.symbol == "demo.Player")
        .expect("player declaration");
    let path = offline
        .paths
        .iter()
        .find(|path| {
            path.root == player_type.id
                && path.segments
                    == [HostPathSegmentDeclaration::Field(
                        player_type.fields[0].id.clone(),
                    )]
        })
        .expect("declared score field path")
        .contract(&offline)?;
    println!(
        "offline field path fingerprint: {:016x}",
        path.fingerprint()?
    );
    println!(
        "offline member: {}.{}",
        player_type.symbol, player_type.fields[0].name
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
            "use build::api::echo; use build::api as api; pub fn direct_set(value: api::Player, next: i32) { value.score = next; } pub fn add_score(value: api::Player, amount: i32) { value.score += amount; } pub fn direct_score(value: api::Player) -> i32 { value.score } pub fn indexed_score(value: api::Player, index: i32) -> i32 { value[index] } pub fn indexed_scores(value: api::Player, index: i32) -> i32 { value.scores[index] } pub fn nested_score(value: api::Player, first: i32, second: i32) -> i32 { value[first][second].selected.score } pub fn score(value: api::Player) -> i32 { value.read_score() } pub fn pass(value: api::Player) -> api::service::Player { value } fn main() -> [i32] { echo(api::service::echo([42])) }",
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
    let signatures = engine
        .signatures(engine.source_snapshot(), &Default::default())
        .expect("offline signatures should be queryable");
    let signature = signatures.file(root.expect("entry source")).unwrap();
    let signature_text = signature.source().text();
    let host_annotation = signature_text.find("value: api::Player").unwrap() + "value: api::".len();
    assert_eq!(
        signature.host_type_at(host_annotation).unwrap().symbol,
        "demo.Player"
    );
    assert!(signature.host_type_at(host_annotation - 2).is_none());
    println!("offline host type navigation is available before body analysis");
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
    let indexed = text.find("value.scores[index]").expect("host field index");
    assert_eq!(
        source
            .host_field_at(indexed + "value.".len())
            .map(|member| member.name.as_str()),
        Some("scores")
    );
    println!("offline field navigation selects the member name");
    let method_dot = text.find(".read_score()").expect("host method call");
    assert!(source.host_function_at(method_dot).is_none());
    assert_eq!(
        source
            .host_function_at(method_dot + 1)
            .map(|function| function.symbol.as_str()),
        Some("demo.Player.read_score")
    );
    println!("offline method navigation selects the method name");
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
    let function_records: usize = artifact
        .program
        .modules
        .iter()
        .map(|module| module.functions.len())
        .sum();
    assert_eq!(
        artifact
            .tables
            .sections
            .iter()
            .find(|section| section.id == ArtifactSectionId::Functions)
            .unwrap()
            .record_count,
        function_records
    );
    println!(
        "artifact function directory matches {} executable records",
        function_records
    );
    let decoded = kagari_embed::BytecodeArtifact::from_bytes(&artifact.to_bytes()?)?;
    decoded.validate_for_loader(&Default::default())?;
    assert_eq!(decoded.header.format_version, KBC_ARTIFACT_FORMAT_VERSION);
    assert_eq!(
        decoded.header.module_identity,
        decoded.program.modules[decoded.program.root.index()].identity
    );
    assert_eq!(decoded.tables.sections, artifact.tables.sections);
    println!(
        "bounded artifact decoding retained {} modules and the checked section directory",
        decoded.program.modules.len()
    );
    println!(
        "compiled {} required host function; artifact is {} bytes",
        artifact.program.modules[artifact.program.root.index()]
            .host_interface
            .functions
            .len(),
        artifact.to_bytes()?.len()
    );
    let required = &artifact.program.modules[artifact.program.root.index()].host_interface;
    assert_eq!(required.types.len(), 2);
    assert_eq!(required.paths.len(), 4);
    println!(
        "public signature requires {} without registering a runtime",
        required
            .types
            .iter()
            .find(|ty| ty.symbol == "demo.Player")
            .unwrap()
            .symbol
    );
    Ok(())
}
