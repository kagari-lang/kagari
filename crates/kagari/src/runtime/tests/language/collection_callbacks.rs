/* Temporarily paused: stdlib restoration (NC01-NC06).
#[test]
fn content_callbacks_preserve_effects_and_release_roots_on_all_exits() {
    let source = include_str!(
        "../../../../tests/fixtures/language/collection_callbacks/content_callbacks.kgr"
    );
    let engine =
        KagariEngine::with_native_modules(Default::default(), stdlib::modules().unwrap()).unwrap();
    let artifact = engine
        .compile_to_artifact(
            SourceFile::new("collection-callbacks.kgr", source),
            Default::default(),
        )
        .unwrap();
    let artifact =
        BytecodeArtifact::from_bytes(&artifact.records().to_bytes(artifact.definitions()).unwrap())
            .unwrap();
    let (definitions, artifact) = artifact.into_parts();
    let program =
        DefinitionMetadata::checked(definitions, artifact.program, &Default::default()).unwrap();
    let mut config = RuntimeConfig::default();
    config.gc.collection_threshold = Some(1);
    let mut runtime = Runtime::new(config);
    NativeModule::install_all(&stdlib::modules().unwrap(), &mut runtime).unwrap();
    let loaded = load_bytecode(&mut runtime, "collection-callbacks", program).unwrap();
    let _loaded_program = loaded;
    let loaded = runtime.select_program(&_loaded_program).unwrap();
    let declaration = loaded
        .bytecode
        .trait_contracts
        .iter()
        .find(|contract| contract.abi.name == "Test")
        .unwrap()
        .declaration;
    let method = |name| ModuleDecl::method_id(declaration, name, loaded.definitions()).unwrap();
    let mut vm = runtime;
    vm.collect_garbage().unwrap();
    let baseline = vm.gc_stats();
    for helper in [false, true] {
        for mode in 0..=if helper { 4 } else { 6 } {
            let root = vm
                .execute_module(&loaded, "make", vm.execution_options())
                .unwrap()
                .return_value;

            let _ = vm
                .invoke_interface_method(&root, &method("mode"), &[RootedValue::from(mode)])
                .unwrap();
            let result = if mode >= 5 {
                vm.invoke_interface_method(&root, &method("hash_items"), &[])
            } else {
                vm.invoke_interface_method(&root, &method("compare"), &[RootedValue::from(helper)])
            };
            match mode {
                0 => assert!(result.unwrap().as_bool().unwrap()),
                2 => assert!(!result.unwrap().as_bool().unwrap()),
                _ => {
                    let expected = match mode {
                        1 | 5 => "structurally modified iterator",
                        3 => "division by zero",
                        4 | 6 => "call depth",
                        _ => unreachable!(),
                    };
                    let error = format!("{:?}", result.unwrap_err());
                    assert!(
                        error.contains(expected),
                        "helper={helper}, mode={mode}: {error}"
                    );
                }
            }
            vm.collect_garbage().unwrap();
            assert_eq!(
                vm.gc_stats().root_registrations,
                baseline.root_registrations + 1
            );
            assert!(!vm.is_quarantined());
            let calls = vm
                .invoke_interface_method(&root, &method("calls"), &[])
                .unwrap();
            if mode == 4 || mode == 6 {
                assert!(calls.as_i32().unwrap() > 1);
            } else {
                assert_eq!(calls.as_i32().unwrap(), 1);
            }
            assert_eq!(
                vm.invoke_interface_method(&root, &method("size"), &[])
                    .unwrap()
                    .as_u64()
                    .unwrap(),
                if matches!(mode, 1 | 2 | 5) { 0 } else { 1 },
            );
            drop(root);
            vm.collect_garbage().unwrap();
            assert_eq!(
                vm.gc_stats().root_registrations,
                baseline.root_registrations
            );
            vm.collect_garbage().unwrap();
            assert_eq!(vm.gc_stats().objects, baseline.objects);
        }
    }
    for limit in [1, 16, 32] {
        let root = vm
            .execute_module(&loaded, "make", vm.execution_options())
            .unwrap()
            .return_value;

        let _ = vm
            .invoke_interface_method(&root, &method("mode"), &[RootedValue::from(4)])
            .unwrap();
        let mut session = support::cancel_after(&mut vm, &loaded, limit);
        let error = session
            .runtime_mut()
            .invoke_interface_method(&root, &method("compare"), &[RootedValue::from(true)])
            .unwrap_err();
        drop(session);
        assert!(format!("{error:?}").contains("cancel"), "{error:?}");
        vm.collect_garbage().unwrap();
        assert_eq!(
            vm.gc_stats().root_registrations,
            baseline.root_registrations + 1
        );
        assert!(!vm.is_quarantined());
        let _ = vm
            .invoke_interface_method(&root, &method("mode"), &[RootedValue::from(0)])
            .unwrap();
        assert!(
            vm.invoke_interface_method(&root, &method("compare"), &[RootedValue::from(true)])
                .unwrap()
                .as_bool()
                .unwrap()
        );
        drop(root);
        vm.collect_garbage().unwrap();
        assert_eq!(vm.gc_stats().objects, baseline.objects);
    }
}
*/
