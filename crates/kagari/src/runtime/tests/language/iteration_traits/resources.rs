/* Temporarily paused: stdlib restoration (NC01-NC06).
#[test]
fn native_iterators_validate_revisions_and_survive_gc_and_failure() {
    let mut config = EngineConfig::default();
    config.default_runtime.gc.collection_threshold = Some(1);
    let engine = KagariEngine::with_native_modules(config, stdlib::modules().unwrap()).unwrap();
    let artifact = engine
        .compile_to_artifact(
            SourceFile::new(
                "iterator-resources.kgr",
                r#"
pub fn fail()->i32{val a=Vec::from([20]);for x in a {a.push(1);}0}
pub fn nested()->i32{val a=Vec::from([20]);for x in a {for y in a {break;}a.push(1);}0}
pub fn manual()->i32{val a=Vec::from([20]);val c=a.iter();c.next();a.push(1);c.next();0}
pub fn trap()->i32{val a=Vec::from([20]);for x in a {return x/0;}0}
pub fn exhaust()->i32{val a=Vec::from([20]);for x in a {while true {}}0}
"#,
            ),
            Default::default(),
        )
        .unwrap();
    let artifact =
        BytecodeArtifact::from_bytes(&artifact.records().to_bytes(artifact.definitions()).unwrap())
            .unwrap();
    let context = ExecutionContext::default();
    let mut runtime = engine.runtime(context.clone());
    let loaded_program =
        Program::from_artifact(artifact.clone(), &Default::default(), &Default::default()).unwrap();
    let loaded = runtime
        .load_program(&loaded_program, Default::default())
        .unwrap();
    let _selected_loaded = runtime.select_program(&loaded).unwrap();
    runtime.collect_garbage().unwrap();
    let roots = runtime.gc_stats().root_registrations;
    for entry in ["fail", "nested", "manual", "trap", "exhaust"] {
        let options = context.clone();
        let error = if entry == "exhaust" {
            let selected = runtime.select_program(&loaded).unwrap();
            let mut cancellation = support::cancel_after(&mut runtime, &selected, 30);
            cancellation
                .runtime_mut()
                .execute(&loaded, entry, &[], &options)
                .unwrap_err()
        } else {
            runtime.execute(&loaded, entry, &[], &options).unwrap_err()
        };
        assert!(
            !format!("{error:?}").contains("MissingFunction"),
            "{entry}: {error:?}"
        );
        assert!(!format!("{error:?}").contains("UnsupportedExecution"));
        assert!(!runtime.is_quarantined());
        runtime.collect_garbage().unwrap();
        assert_eq!(runtime.gc_stats().root_registrations, roots, "{entry}");
    }
}
*/
