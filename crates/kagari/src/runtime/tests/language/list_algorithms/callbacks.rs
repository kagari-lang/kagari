/* Temporarily paused: stdlib restoration (NC01-NC06).
#[test]
fn cancellation_during_callbacks_restores_storage_and_releases_roots() {
    for (operation, retained_values, preserve_order) in [
        (
            "sort_by(|a,b| { visit(); a.cmp(b) })",
            vec![0, 1, 2, 3],
            false,
        ),
        (
            "retain(|value| { visit(); value != 1 })",
            vec![0, 2, 3],
            false,
        ),
        (
            "filter_map(|value| { visit(); if value == 99 { Some(value) } else { None } }).count()",
            vec![1, 3, 2, 0],
            true,
        ),
    ] {
        let context = ExecutionContext::default();
        let mut module = ModuleBuilder::new(
            "test::cancellation",
            &StandardDeclarations::default()
                .catalog()
                .expect("explicit standard providers"),
        )
        .unwrap();
        let calls = Arc::new(AtomicUsize::new(0));
        let count = calls.clone();
        let token = context.cancellation.clone();
        let visit = module.define_function(FunctionDecl::new("visit")).unwrap();
        module
            .bind(visit, move |_cx: &mut NativeCall<'_>| -> NativeResult<()> {
                count.fetch_add(1, Ordering::SeqCst);
                if count.load(Ordering::SeqCst) == 3 {
                    token.cancel();
                }
                Ok(())
            })
            .unwrap();
        let retained: Arc<Mutex<Option<RootedValue>>> = Arc::new(Mutex::new(None));
        let capture = retained.clone();
        let keep = module.define_function(FunctionDecl::new("keep")).unwrap();
        module
            .function(&keep, |function| {
                let parameter = function.type_parameter("T")?.ty();
                function.parameter("value", parameter)?;
                Ok(())
            })
            .unwrap();
        module
            .bind(
                keep,
                move |cx: &mut NativeCall<'_>, value: ValueHandle<'_>| -> NativeResult<()> {
                    *capture.lock().unwrap() = Some(value.root(cx)?);
                    Ok(())
                },
            )
            .unwrap();
        let mut builder = KagariEngine::builder().unwrap();
        builder.install_all(stdlib::modules().unwrap()).unwrap();
        builder.install(module.finish().unwrap()).unwrap();
        let engine = builder.build().unwrap();
        let fields = (0..retained_values.len())
            .map(|index| format!("values[{index}]"))
            .collect::<Vec<_>>()
            .join(", ");
        let length = retained_values.len();
        let source = format!(
            "use test::cancellation::{{keep, visit}}; fn main() {{ val values = Vec::from([1,3,2,0]); keep(values); values.{operation}; }} pub fn inspect(values: Vec<i32>) -> [i32] {{ if values.len() != {length}u64 {{ val fail = 1 / 0; }} val snapshot = [{fields}]; values.push(99); snapshot }}"
        );
        let artifact = engine
            .compile_to_artifact(SourceFile::new("cancel.kgr", source), Default::default())
            .unwrap();
        let program =
            Program::from_artifact(artifact, &Default::default(), &Default::default()).unwrap();
        let mut runtime = engine.runtime(context.clone());
        let loaded = runtime.load_program(&program, Default::default()).unwrap();
        runtime.collect_garbage().unwrap();
        let baseline = runtime.gc_stats();
        assert_eq!(
            runtime
                .execute(&loaded, ProgramEntry::Main, &[], &context)
                .unwrap_err()
                .code(),
            "KG_RUNTIME_CANCELLED"
        );
        assert_eq!(calls.load(Ordering::SeqCst), 3);
        let value = retained.lock().unwrap().as_ref().unwrap().clone();
        let owner = runtime.select_program(&loaded).unwrap();
        let execution = runtime
            .start(
                &owner,
                "inspect",
                slice::from_ref(&value),
                Default::default(),
            )
            .unwrap();
        let mut output = None;
        for _ in 0..1000 {
            match runtime
                .drive(&execution, NonZeroUsize::new(256).unwrap())
                .unwrap()
            {
                DriveResult::Complete(value) => {
                    output = Some(value.unwrap());
                    break;
                }
                DriveResult::Runnable => {}
                DriveResult::Waiting => panic!("synchronous inspection cannot wait"),
            }
        }
        let snapshot = output.expect("bounded inspection completion");
        drop(execution);
        let mut actual = {
            let mut cx = ConversionContext::new(&mut runtime, &owner).unwrap();
            let array = cx.import(&snapshot).unwrap();
            assert_eq!(cx.array_len(&array).unwrap(), retained_values.len());
            (0..retained_values.len())
                .map(|index| {
                    cx.array_element(&array, index)
                        .unwrap()
                        .unwrap()
                        .as_i32()
                        .unwrap()
                })
                .collect::<Vec<_>>()
        };
        if !preserve_order {
            actual.sort();
        }
        assert_eq!(actual, retained_values);
        drop((snapshot, value));
        retained.lock().unwrap().take();
        runtime.collect_garbage().unwrap();
        assert_eq!(
            runtime.gc_stats().root_registrations,
            baseline.root_registrations
        );
        let replacement = runtime
            .program(&loaded)
            .unwrap()
            .prepare_replacement(&program, &Default::default())
            .unwrap();
        runtime
            .replace_program(&loaded, &replacement, Default::default())
            .unwrap();
        runtime.collect_garbage().unwrap();
        assert_eq!(runtime.gc_stats().objects, baseline.objects);
        assert!(!runtime.is_quarantined());
    }
}
*/
