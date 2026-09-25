use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

use kagari_common::host_interface::{
    HostFunctionDeclaration, HostInterface, HostInterfaceError, HostParameter, HostPassingStyle,
    HostValueType,
};
use kagari_runtime::{Runtime, host::HostFunction, value::Value};

fn declaration() -> HostFunctionDeclaration {
    HostFunctionDeclaration::new(
        "game.echo",
        vec![HostParameter {
            name: "value".into(),
            ty: HostValueType::I32,
            passing: HostPassingStyle::Owned,
        }],
        HostValueType::I32,
    )
}

#[test]
fn module_load_and_reload_require_matching_bindings_before_publication() {
    use kagari_ir::bytecode::{
        ArtifactBuildOptions, ArtifactCompatibility, BytecodeModule, KbcArtifact,
    };
    let mut runtime = Runtime::default();
    let required = declaration();
    let bytecode = BytecodeModule {
        host_interface: HostInterface {
            paths: vec![],
            types: Vec::new(),
            functions: vec![required.clone()],
        },
        ..Default::default()
    };
    assert!(
        runtime
            .load_program(
                "host",
                kagari_ir::bytecode::BytecodeProgram {
                    root: kagari_ir::bytecode::ModuleRef::new(0),
                    modules: vec![bytecode.clone()]
                }
            )
            .is_err()
    );
    assert_eq!(runtime.modules().loaded_count(), 0);
    assert_eq!(runtime.resources().counters().loaded_modules, 0);
    runtime
        .register_host_function(HostFunction::new(required, |_, _| {
            panic!("linking must not invoke host")
        }))
        .unwrap();
    let loaded = runtime
        .load_program(
            "host",
            kagari_ir::bytecode::BytecodeProgram {
                root: kagari_ir::bytecode::ModuleRef::new(0),
                modules: vec![bytecode.clone()],
            },
        )
        .unwrap();
    let shared = runtime.modules().latest("host").unwrap();
    assert!(std::ptr::eq(&loaded.bytecode, &shared.bytecode));
    assert!(std::ptr::eq(&loaded.bytecode, &loaded.clone().bytecode));
    let before = runtime.resources().counters();
    let mut mismatch = bytecode;
    mismatch.host_interface.functions[0]
        .effects
        .may_mutate_host_state = true;
    assert!(
        runtime
            .stage_reload_program(
                &loaded,
                "host",
                kagari_ir::bytecode::BytecodeProgram {
                    root: kagari_ir::bytecode::ModuleRef::new(0),
                    modules: vec![mismatch.clone()]
                }
            )
            .is_err()
    );
    let artifact = KbcArtifact::from_program(
        kagari_ir::bytecode::BytecodeProgram {
            root: kagari_ir::bytecode::ModuleRef::new(0),
            modules: vec![mismatch],
        },
        ArtifactBuildOptions::default(),
    )
    .unwrap();
    assert!(
        runtime
            .stage_reload_artifact(&loaded, "host", artifact, &ArtifactCompatibility::default())
            .is_err()
    );
    assert_eq!(
        runtime.modules().latest("host").unwrap().key(),
        loaded.key()
    );
    assert_eq!(runtime.resources().counters(), before);
}

#[test]
fn bound_slots_and_loaded_handles_reject_another_runtime() {
    use kagari_ir::bytecode::BytecodeModule;
    let mut first = Runtime::default();
    let mut second = Runtime::default();
    let a = first
        .register_host_function(HostFunction::new(declaration(), |_, _| unreachable!()))
        .unwrap();
    let b = second
        .register_host_function(HostFunction::new(declaration(), |_, _| unreachable!()))
        .unwrap();
    assert_eq!(a.index(), b.index());
    assert_ne!(a, b);
    assert!(second.invoke_bound_host(a, &[Value::I32(1)]).is_err());
    let a = first
        .load_program(
            "same",
            kagari_ir::bytecode::BytecodeProgram {
                root: kagari_ir::bytecode::ModuleRef::new(0),
                modules: vec![BytecodeModule::default()],
            },
        )
        .unwrap();
    let b = second
        .load_program(
            "same",
            kagari_ir::bytecode::BytecodeProgram {
                root: kagari_ir::bytecode::ModuleRef::new(0),
                modules: vec![BytecodeModule::default()],
            },
        )
        .unwrap();
    assert_eq!(a.key(), b.key());
    assert!(second.validate_loaded_module(&a).is_err());
    assert!(second.module_instance_snapshot(&a).is_none());
    assert!(second.module_instance_mut(&a).is_err());
    assert!(
        second
            .stage_reload_program(
                &a,
                "same",
                kagari_ir::bytecode::BytecodeProgram {
                    root: kagari_ir::bytecode::ModuleRef::new(0),
                    modules: vec![BytecodeModule::default()]
                }
            )
            .is_err()
    );
    assert!(second.validate_loaded_module(&b).is_ok());
}

#[test]
fn callback_arguments_and_result_obey_the_declared_representation() {
    let calls = Arc::new(AtomicUsize::new(0));
    let called = calls.clone();
    let function = HostFunction::new(declaration(), move |_, _| {
        called.fetch_add(1, Ordering::SeqCst);
        Ok(Value::Bool(true))
    });
    let mut runtime = Runtime::new(kagari_runtime::RuntimeConfig {
        security: kagari_runtime::SecurityContext {
            profile: kagari_runtime::LanguageProfile {
                allow_host_calls: true,
                ..Default::default()
            },
            capabilities: kagari_runtime::CapabilitySet {
                host_calls: true,
                ..Default::default()
            },
        },
        host_exposure: kagari_runtime::HostExposurePolicy {
            allow_host_functions: true,
            ..Default::default()
        },
        ..Default::default()
    });
    let id = runtime.register_host_function(function).unwrap();
    assert!(runtime.invoke_bound_host(id, &[]).is_err());
    assert!(runtime.invoke_bound_host(id, &[Value::Bool(true)]).is_err());
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    assert!(runtime.invoke_bound_host(id, &[Value::I32(1)]).is_err());
    assert_eq!(calls.load(Ordering::SeqCst), 1);
}

#[test]
fn offline_roundtrip_and_binding_link_do_not_run_callbacks() {
    let declaration = declaration();
    let interface = HostInterface {
        paths: vec![],
        types: Vec::new(),
        functions: vec![declaration.clone()],
    };
    let encoded = interface.to_bytes().unwrap();
    let decoded = HostInterface::from_bytes(&encoded).unwrap();
    assert_eq!(decoded, interface);
    let calls = Arc::new(AtomicUsize::new(0));
    let called = calls.clone();
    let mut runtime = Runtime::default();
    let id = runtime
        .register_host_function(HostFunction::new(declaration, move |_, _| {
            called.fetch_add(1, Ordering::SeqCst);
            Ok(Value::I32(7))
        }))
        .unwrap();
    assert_eq!(runtime.host().link_interface(&decoded).unwrap(), vec![id]);
    assert_eq!(runtime.host().interface(), decoded);
    assert_eq!(calls.load(Ordering::SeqCst), 0);
}

#[test]
fn linking_checks_identity_signature_borrow_effects_permissions_and_cost() {
    let declaration = declaration();
    let mut runtime = Runtime::default();
    runtime
        .register_host_function(HostFunction::new(declaration.clone(), |_, _| {
            panic!("linking must not invoke callbacks")
        }))
        .unwrap();
    let changes: [fn(&mut HostFunctionDeclaration); 7] = [
        |d| d.id.module.package.0 = "another-provider".into(),
        |d| d.params[0].ty = HostValueType::Bool,
        |d| d.return_type = HostValueType::String,
        |d| {
            d.params[0].ty = HostValueType::opaque("game.Player");
            d.params[0].passing = HostPassingStyle::UniqueBorrow;
        },
        |d| d.effects.may_mutate_host_state = true,
        |d| d.capability_requirements.net = true,
        |d| d.resource_cost_hint = Some(9),
    ];
    for change in changes {
        let mut expected = declaration.clone();
        change(&mut expected);
        assert_ne!(
            expected.fingerprint().unwrap(),
            declaration.fingerprint().unwrap()
        );
        assert!(
            runtime
                .host()
                .link_interface(&HostInterface {
                    paths: vec![],
                    types: Vec::new(),
                    functions: vec![expected]
                })
                .is_err()
        );
    }
    let mut docs = declaration.clone();
    docs.documentation = "Updated help text".into();
    assert_eq!(
        docs.fingerprint().unwrap(),
        declaration.fingerprint().unwrap()
    );
    runtime
        .host()
        .link_interface(&HostInterface {
            paths: vec![],
            types: Vec::new(),
            functions: vec![docs],
        })
        .unwrap();
    let absent = HostFunctionDeclaration::new("missing.echo", vec![], HostValueType::Unit);
    assert!(
        runtime
            .host()
            .link_interface(&HostInterface {
                paths: vec![],
                types: Vec::new(),
                functions: vec![absent]
            })
            .is_err()
    );
}

#[test]
fn offline_encoding_is_canonical_and_rejects_invalid_input() {
    let a = declaration();
    let b = HostFunctionDeclaration::new("game.other", vec![], HostValueType::Unit);
    let first = HostInterface {
        paths: vec![],
        types: Vec::new(),
        functions: vec![a.clone(), b.clone()],
    }
    .to_bytes()
    .unwrap();
    let second = HostInterface {
        paths: vec![],
        types: Vec::new(),
        functions: vec![b, a.clone()],
    }
    .to_bytes()
    .unwrap();
    assert_eq!(first, second);
    let mut trailing = first.clone();
    trailing.push(0);
    assert_eq!(
        HostInterface::from_bytes(&trailing),
        Err(HostInterfaceError::Encoding)
    );
    let mut old = first.clone();
    old[4..6].copy_from_slice(&0_u16.to_le_bytes());
    assert_eq!(
        HostInterface::from_bytes(&old),
        Err(HostInterfaceError::Version)
    );
    assert!(HostInterface::from_bytes(&first[..8]).is_err());
    assert_eq!(
        HostInterface::from_bytes(&vec![0; 4 * 1024 * 1024 + 1]),
        Err(HostInterfaceError::TooLarge)
    );
    let duplicate = HostInterface {
        paths: vec![],
        types: Vec::new(),
        functions: vec![a.clone(), a],
    };
    assert_eq!(
        duplicate.to_bytes(),
        Err(HostInterfaceError::DuplicateDeclaration)
    );
}

#[test]
fn invalid_registration_leaves_registry_unchanged() {
    let mut runtime = Runtime::default();
    let mut bad = declaration();
    bad.params[0].passing = HostPassingStyle::UniqueBorrow;
    assert!(
        runtime
            .register_host_function(HostFunction::new(bad, |_, _| unreachable!()))
            .is_err()
    );
    assert!(runtime.host().interface().functions.is_empty());
    let a = declaration();
    runtime
        .register_host_function(HostFunction::new(a.clone(), |_, _| unreachable!()))
        .unwrap();
    let mut duplicate = a;
    duplicate.symbol = "different-label".into();
    assert!(
        runtime
            .register_host_function(HostFunction::new(duplicate, |_, _| unreachable!()))
            .is_err()
    );
    assert_eq!(runtime.host().interface().functions.len(), 1);
}

#[test]
fn immutable_configuration_contracts_are_portable_and_reject_shared_objects() {
    let pure = declaration();
    let mut configuration = pure.clone();
    configuration.effects.may_read_immutable_configuration = true;
    assert_ne!(
        pure.fingerprint().unwrap(),
        configuration.fingerprint().unwrap()
    );
    assert!(!pure.matches_binding(&configuration));
    let interface = HostInterface {
        functions: vec![configuration.clone()],
        ..Default::default()
    };
    let bytes = interface.to_bytes().unwrap();
    assert_eq!(HostInterface::from_bytes(&bytes).unwrap(), interface);
    let mut old = bytes.clone();
    old[4..6].copy_from_slice(&5u16.to_le_bytes());
    assert_eq!(
        HostInterface::from_bytes(&old),
        Err(HostInterfaceError::Version)
    );
    for result in [
        HostValueType::Array(Box::new(HostValueType::I32)),
        HostValueType::Tuple(vec![HostValueType::Set(Box::new(HostValueType::I32))]),
        HostValueType::Option(Box::new(HostValueType::opaque("game.Object"))),
    ] {
        let mut invalid = configuration.clone();
        invalid.return_type = result;
        assert_eq!(
            invalid.validate(),
            Err(HostInterfaceError::InvalidDeclaration)
        );
    }
    let mut invalid = configuration.clone();
    invalid.params[0].ty = HostValueType::Array(Box::new(HostValueType::I32));
    assert_eq!(
        invalid.validate(),
        Err(HostInterfaceError::InvalidDeclaration)
    );
    configuration.return_type = HostValueType::Option(Box::new(HostValueType::Tuple(vec![
        HostValueType::String,
        HostValueType::I32,
    ])));
    configuration.validate().unwrap();
}
