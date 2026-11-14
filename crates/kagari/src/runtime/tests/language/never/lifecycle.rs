/* Temporarily paused: stdlib restoration (NC01-NC06).
#[test]
fn never_preserves_effects_and_releases_resources_on_traps_and_cancellation() {
    let engine =
        KagariEngine::with_native_modules(Default::default(), stdlib::modules().unwrap()).unwrap();
    let artifact = engine
        .compile_to_artifact(
            SourceFile::new(
                "never-effects.kgr",
                r#"
fn fail(values: Vec<i32>) -> ! { values.push(20); print("before"); val zero = 0; 1 / zero; loop {} }
fn later(values: Vec<i32>) -> i32 { print("after"); values.push(99); 99 }
fn consume(a: i32, b: i32) -> i32 { a + b }
fn main() -> i32 { val values = Vec::from([1]); consume(fail(values), later(values)) }
pub fn forever() -> ! { loop {} }
fn impossible(value: !) -> i32 { value }
pub fn healthy() -> i32 { 42 }
"#,
            ),
            Default::default(),
        )
        .unwrap();
    let effects = Arc::new(Mutex::new(Vec::new()));
    let captured = effects.clone();
    let mut builder = ModuleBuilder::from_declaration(
        logging::declaration().unwrap(),
        &kagari_stdlib::declarations::StandardDeclarations::default()
            .catalog()
            .unwrap(),
        Default::default(),
    )
    .unwrap();
    let log = builder.declared_function("log").unwrap();
    builder
        .bind(
            log,
            host::callback(move |cx, args| {
                let [message] = args else {
                    panic!("log string")
                };
                captured.lock().unwrap().push(
                    cx.read_string(message, str::to_owned)
                        .map_err(|error| HostError::new(error.to_string()))?,
                );
                Ok(ValueView::unit())
            }),
        )
        .unwrap();
    let installed = KagariEngine::with_native_modules(
        Default::default(),
        stdlib::modules()
            .unwrap()
            .into_iter()
            .chain(vec![builder.finish().unwrap()])
            .collect(),
    )
    .unwrap();
    for encoded in [false, true] {
        let artifact = if encoded {
            BytecodeArtifact::from_bytes(
                &artifact.records().to_bytes(artifact.definitions()).unwrap(),
            )
            .unwrap()
        } else {
            artifact.clone()
        };
        let context = ExecutionContext::default();

        let mut runtime = engine.runtime(context.clone());
        let prepared =
            Program::from_artifact(artifact, &Default::default(), &Default::default()).unwrap();
        assert!(runtime.load_program(&prepared, Default::default()).is_err());
        effects.lock().unwrap().clear();
        let mut runtime = installed.runtime(context.clone());
        let loaded = runtime.load_program(&prepared, Default::default()).unwrap();
        runtime.collect_garbage().unwrap();
        let roots = runtime.gc_stats().root_registrations;
        let error = runtime
            .execute(&loaded, ProgramEntry::Main, &[], &context)
            .unwrap_err();
        assert_eq!(error.code(), "KG_RUNTIME_SCRIPT_TRAP");
        assert_eq!(*effects.lock().unwrap(), vec!["before"]);
        assert!(runtime.execution_root().is_none());
        runtime.collect_garbage().unwrap();
        assert_eq!(runtime.gc_stats().root_registrations, roots);
        let limited = context.clone();

        let selected = runtime.select_program(&loaded).unwrap();
        let mut cancellation = support::cancel_after(&mut runtime, &selected, 20);
        let error = cancellation
            .runtime_mut()
            .execute(&loaded, "forever", &[], &limited)
            .unwrap_err();
        assert_eq!(error.code(), "KG_RUNTIME_CANCELLED");
        drop(cancellation);
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
}
*/
