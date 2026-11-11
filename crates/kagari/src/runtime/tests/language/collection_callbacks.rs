use crate::test_support::bytecode::load_bytecode;
use crate::test_support::cancellation as support;
use kagari_stdlib as stdlib;

use crate::{BytecodeArtifact, engine::KagariEngine};
use kagari_common::identity::metadata::DefinitionMetadata;
use kagari_runtime::{Runtime, RuntimeConfig, native::module::NativeModule, value::Value};
use kagari_source::source::SourceFile;
use kagari_types::declaration::module::ModuleDecl;

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
    config.limits.max_call_depth = Some(32);
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
    for helper in [false, true] {
        for mode in 0..=if helper { 4 } else { 6 } {
            let root = vm
                .execute_module(&loaded, "make", vm.execution_options())
                .unwrap()
                .return_value;
            let value = root.value(vm.gc()).unwrap();
            vm.invoke_interface_method(&value, &method("mode"), &[Value::I32(mode)])
                .unwrap();
            let result = if mode >= 5 {
                vm.invoke_interface_method(&value, &method("hash_items"), &[])
            } else {
                vm.invoke_interface_method(&value, &method("compare"), &[Value::Bool(helper)])
            };
            match mode {
                0 => assert_eq!(result.unwrap(), Value::Bool(true)),
                2 => assert_eq!(result.unwrap(), Value::Bool(false)),
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
            assert_eq!(vm.gc().active_roots(), 1);
            assert!(!vm.is_quarantined());
            let calls = vm
                .invoke_interface_method(&value, &method("calls"), &[])
                .unwrap();
            if mode == 4 || mode == 6 {
                assert!(matches!(calls, Value::I32(count) if count > 1));
            } else {
                assert_eq!(calls, Value::I32(1));
            }
            assert_eq!(
                vm.invoke_interface_method(&value, &method("size"), &[])
                    .unwrap(),
                Value::U64(if matches!(mode, 1 | 2 | 5) { 0 } else { 1 }),
            );
            drop(root);
            assert_eq!(vm.gc().active_roots(), 0);
            assert_eq!(vm.collect_garbage().unwrap().live_objects, 0);
        }
    }
    for limit in [1, 16, 32] {
        let root = vm
            .execute_module(&loaded, "make", vm.execution_options())
            .unwrap()
            .return_value;
        let value = root.value(vm.gc()).unwrap();
        vm.invoke_interface_method(&value, &method("mode"), &[Value::I32(4)])
            .unwrap();
        let mut session = support::cancel_after(&mut vm, &loaded, limit);
        let error = session
            .runtime_mut()
            .invoke_interface_method(&value, &method("compare"), &[Value::Bool(true)])
            .unwrap_err();
        drop(session);
        assert!(format!("{error:?}").contains("cancel"), "{error:?}");
        assert_eq!(vm.gc().active_roots(), 1);
        assert!(!vm.is_quarantined());
        vm.invoke_interface_method(&value, &method("mode"), &[Value::I32(0)])
            .unwrap();
        assert_eq!(
            vm.invoke_interface_method(&value, &method("compare"), &[Value::Bool(true)])
                .unwrap(),
            Value::Bool(true)
        );
        drop(root);
        assert_eq!(vm.collect_garbage().unwrap().live_objects, 0);
    }
}
