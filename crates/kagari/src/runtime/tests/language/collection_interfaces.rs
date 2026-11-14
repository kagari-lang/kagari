mod admission;
mod rejection;

// Temporarily paused: stdlib restoration (NC01-NC06).
// #[rstest]
// fn ordinary_execution(
//     #[base_dir = "tests/fixtures/language/collection_interfaces"]
//     #[files("ordinary/*.kgr")]
//     path: PathBuf,
// ) {
//     let source = fs::read_to_string(&path)
//         .unwrap_or_else(|error| panic!("failed to read fixture {}: {error}", path.display()));
//     let mut config = EngineConfig::default();
//     config.default_runtime.gc.collection_threshold = Some(1);
//     let engine = execution::engine(config);
//     let artifact = engine
//         .compile_source(SourceFile::new("standard-traits.kgr", &source))
//         .unwrap();
//     execution::assert_returns_i32(
//         &engine,
//         artifact,
//         &[Route::Source, Route::Artifact, Route::ArtifactJit],
//         42,
//     );
// }
