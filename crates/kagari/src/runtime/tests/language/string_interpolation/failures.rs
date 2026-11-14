/* Temporarily paused: stdlib restoration (NC01-NC06).
#[test]
fn formatter_traps_keep_the_origin_and_release_execution_roots() {
    let engine =
        KagariEngine::with_native_modules(Default::default(), stdlib::modules().unwrap()).unwrap();
    let source = include_str!(
        "../../../../../tests/fixtures/language/string_interpolation/traps/formatter_traps_keep_the_origin_and_release_execution_roots.kgr"
    );
    let artifact = engine
        .compile_to_artifact(
            SourceFile::new("formatter-trap.kgr", source),
            Default::default(),
        )
        .unwrap();
    let context = ExecutionContext::default();
    let mut runtime = engine.runtime(context.clone());
    let loaded_program =
        Program::from_artifact(artifact, &Default::default(), &Default::default()).unwrap();
    let loaded = runtime
        .load_program(&loaded_program, Default::default())
        .unwrap();
    runtime.collect_garbage().unwrap();
    let roots = runtime.gc_stats().root_registrations;
    let error = runtime
        .execute(&loaded, ProgramEntry::Main, &[], &context)
        .unwrap_err();
    assert_eq!(error.code(), "KG_RUNTIME_SCRIPT_TRAP");
    assert!(format!("{error:?}").contains("division by zero"));
    assert!(error.trace().unwrap().frames.len() >= 2);
    assert!(runtime.execution_root().is_none());
    runtime.collect_garbage().unwrap();
    assert_eq!(runtime.gc_stats().root_registrations, roots);
    assert_eq!(
        runtime
            .execute(&loaded, "healthy", &[], &context)
            .unwrap()
            .return_value
            .as_i32()
            .unwrap(),
        42
    );
}
*/
