use kagari_common::{
    cancellation::CancellationToken,
    identity::{
        DefinitionPath,
        map::DefinitionContext,
        mapping::DefinitionRecord,
        metadata::{DefinitionMetadata, scope_record},
        table::DefinitionId,
    },
};

use kagari_bytecode::{
    instruction::ModuleSlot,
    program::{BytecodeProgram, ModuleRef},
};
use kagari_runtime::{
    Runtime,
    error::RuntimeError,
    host::{HostFunction, HostFunctionId},
    value::Value,
};
use kagari_types::{
    collection::CollectionAccess,
    host_interface::{
        HostFunctionDeclaration, HostInterface, HostInterfaceError, HostParameter,
        HostPassingStyle, value_type::HostValueType,
    },
};
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

fn scoped<T: DefinitionRecord<DefinitionPath>>(
    value: &T,
) -> DefinitionMetadata<T::Rebind<DefinitionId>>
where
    T::Rebind<DefinitionId>: DefinitionRecord<DefinitionId>,
{
    scope_record(value, &CancellationToken::default()).unwrap()
}

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

fn encode(interface: &HostInterface) -> Result<Vec<u8>, HostInterfaceError> {
    let input = scoped(interface);
    input.records().to_bytes(input.definitions())
}

fn fingerprint(declaration: &HostFunctionDeclaration) -> u64 {
    let input = scoped(declaration);
    input.records().fingerprint(input.definitions()).unwrap()
}

fn validate(declaration: &HostFunctionDeclaration) -> Result<(), HostInterfaceError> {
    let input = scoped(declaration);
    input.records().validate(input.definitions())
}

fn link(runtime: &Runtime, interface: &HostInterface) -> Result<Vec<HostFunctionId>, RuntimeError> {
    let input = scoped(interface);
    runtime
        .host()
        .link_interface(input.records(), input.definitions())
}

#[test]
fn module_load_and_reload_require_matching_bindings_before_publication() {
    use kagari_bytecode::{
        artifact::{ArtifactBuildOptions, ArtifactCompatibility, KbcArtifact},
        module::BytecodeModule,
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
                scoped(&BytecodeProgram {
                    root: ModuleRef::new(0),
                    modules: vec![bytecode.clone()]
                })
            )
            .is_err()
    );
    assert_eq!(runtime.modules().loaded_count(), 0);
    runtime
        .register_host_function(HostFunction::new(scoped(&required), |_, _| {
            panic!("linking must not invoke host")
        }))
        .unwrap();
    let loaded = runtime
        .load_program(
            "host",
            scoped(&BytecodeProgram {
                root: ModuleRef::new(0),
                modules: vec![bytecode.clone()],
            }),
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
                scoped(&BytecodeProgram {
                    root: ModuleRef::new(0),
                    modules: vec![mismatch.clone()]
                })
            )
            .is_err()
    );
    let context = DefinitionContext::new().unwrap();
    let program = scoped(&BytecodeProgram {
        root: ModuleRef::new(0),
        modules: vec![mismatch],
    })
    .import_into(&context, &CancellationToken::default())
    .unwrap();
    let artifact = KbcArtifact::from_program(
        program.into_records(),
        ArtifactBuildOptions::default(),
        &context,
    )
    .unwrap();
    let artifact =
        DefinitionMetadata::checked(context.snapshot(), artifact, &CancellationToken::default())
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
    use kagari_bytecode::module::BytecodeModule;
    let mut first = Runtime::default();
    let mut second = Runtime::default();
    let a = first
        .register_host_function(HostFunction::new(
            scoped(&declaration()),
            |_, _| unreachable!(),
        ))
        .unwrap();
    let b = second
        .register_host_function(HostFunction::new(
            scoped(&declaration()),
            |_, _| unreachable!(),
        ))
        .unwrap();
    assert_eq!(a.index(), b.index());
    assert_ne!(a, b);
    assert!(second.invoke_bound_host(a, &[Value::I32(1)]).is_err());
    let a = first
        .load_program(
            "same",
            scoped(&BytecodeProgram {
                root: ModuleRef::new(0),
                modules: vec![BytecodeModule::<DefinitionPath>::default()],
            }),
        )
        .unwrap();
    let b = second
        .load_program(
            "same",
            scoped(&BytecodeProgram {
                root: ModuleRef::new(0),
                modules: vec![BytecodeModule::<DefinitionPath>::default()],
            }),
        )
        .unwrap();
    assert_eq!(a.key(), b.key());
    assert!(second.validate_loaded_module(&a).is_err());
    assert!(second.module_instance_snapshot(&a).is_none());
    assert!(second.read_module_slot(&a, ModuleSlot::new(0)).is_err());
    assert!(
        second
            .stage_reload_program(
                &a,
                "same",
                scoped(&BytecodeProgram {
                    root: ModuleRef::new(0),
                    modules: vec![BytecodeModule::<DefinitionPath>::default()]
                })
            )
            .is_err()
    );
    assert!(second.validate_loaded_module(&b).is_ok());
}

#[test]
fn callback_arguments_and_result_obey_the_declared_representation() {
    let calls = Arc::new(AtomicUsize::new(0));
    let called = calls.clone();
    let function = HostFunction::new(scoped(&declaration()), move |_, _| {
        called.fetch_add(1, Ordering::SeqCst);
        Ok(Value::Bool(true))
    });
    let mut runtime = Runtime::new(kagari_runtime::RuntimeConfig {
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
    let encoded = encode(&interface).unwrap();
    let decoded = HostInterface::from_bytes(&encoded).unwrap();
    let context = DefinitionContext::new().unwrap();
    let source = scoped(&interface)
        .import_into(&context, &CancellationToken::default())
        .unwrap();
    let imported = decoded
        .import_into(&context, &CancellationToken::default())
        .unwrap();
    assert_eq!(imported.records(), source.records());
    let calls = Arc::new(AtomicUsize::new(0));
    let called = calls.clone();
    let mut runtime = Runtime::default();
    let prior = runtime.host().definitions();
    let id = runtime
        .register_host_function(HostFunction::new(scoped(&declaration), move |_, _| {
            called.fetch_add(1, Ordering::SeqCst);
            Ok(Value::I32(7))
        }))
        .unwrap();
    assert_eq!(
        runtime
            .host()
            .link_interface(decoded.records(), decoded.definitions())
            .unwrap(),
        vec![id]
    );
    let installed = runtime.host().interface();
    assert_ne!(
        decoded.records().functions[0].id,
        installed.records().functions[0].id
    );
    assert!(
        runtime
            .host()
            .link_interface(installed.records(), decoded.definitions())
            .is_err()
    );
    assert!(
        runtime
            .host()
            .link_interface(installed.records(), &prior)
            .is_err()
    );
    assert_eq!(
        installed
            .records()
            .to_bytes(installed.definitions())
            .unwrap(),
        encoded
    );
    assert_eq!(calls.load(Ordering::SeqCst), 0);
}

#[test]
fn linking_checks_identity_signature_borrow_effects_permissions_and_cost() {
    let declaration = declaration();
    let mut runtime = Runtime::default();
    runtime
        .register_host_function(HostFunction::new(scoped(&declaration), |_, _| {
            panic!("linking must not invoke callbacks")
        }))
        .unwrap();
    let changes: [fn(&mut HostFunctionDeclaration); 5] = [
        |d| d.id.module.package.0 = "another-provider".into(),
        |d| d.params[0].ty = HostValueType::Bool,
        |d| d.return_type = HostValueType::String,
        |d| {
            d.params[0].ty = HostValueType::opaque("game.Player");
            d.params[0].passing = HostPassingStyle::UniqueBorrow;
        },
        |d| d.effects.may_mutate_host_state = true,
    ];
    for change in changes {
        let mut expected = declaration.clone();
        change(&mut expected);
        assert_ne!(fingerprint(&expected), fingerprint(&declaration));
        assert!(
            link(
                &runtime,
                &HostInterface {
                    paths: vec![],
                    types: Vec::new(),
                    functions: vec![expected]
                }
            )
            .is_err()
        );
    }
    let mut docs = declaration.clone();
    docs.documentation = "Updated help text".into();
    assert_eq!(fingerprint(&docs), fingerprint(&declaration));
    link(
        &runtime,
        &HostInterface {
            paths: vec![],
            types: Vec::new(),
            functions: vec![docs],
        },
    )
    .unwrap();
    let absent = HostFunctionDeclaration::new("missing.echo", vec![], HostValueType::Unit);
    assert!(
        link(
            &runtime,
            &HostInterface {
                paths: vec![],
                types: Vec::new(),
                functions: vec![absent]
            }
        )
        .is_err()
    );
}

#[test]
fn offline_encoding_is_canonical_and_rejects_invalid_input() {
    let a = declaration();
    let b = HostFunctionDeclaration::new("game.other", vec![], HostValueType::Unit);
    let first = encode(&HostInterface {
        paths: vec![],
        types: Vec::new(),
        functions: vec![a.clone(), b.clone()],
    })
    .unwrap();
    let second = encode(&HostInterface {
        paths: vec![],
        types: Vec::new(),
        functions: vec![b, a.clone()],
    })
    .unwrap();
    assert_eq!(first, second);
    let mut trailing = first.clone();
    trailing.push(0);
    assert_eq!(
        HostInterface::from_bytes(&trailing).unwrap_err(),
        HostInterfaceError::Encoding
    );
    let mut old = first.clone();
    old[4..6].copy_from_slice(&0_u16.to_le_bytes());
    assert_eq!(
        HostInterface::from_bytes(&old).unwrap_err(),
        HostInterfaceError::Version
    );
    assert!(HostInterface::from_bytes(&first[..8]).is_err());
    assert_eq!(
        HostInterface::from_bytes(&vec![0; 4 * 1024 * 1024 + 1]).unwrap_err(),
        HostInterfaceError::TooLarge
    );
    let duplicate = HostInterface {
        paths: vec![],
        types: Vec::new(),
        functions: vec![a.clone(), a],
    };
    assert_eq!(
        encode(&duplicate),
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
            .register_host_function(HostFunction::new(scoped(&bad), |_, _| unreachable!()))
            .is_err()
    );
    assert!(runtime.host().interface().records().functions.is_empty());
    let a = declaration();
    runtime
        .register_host_function(HostFunction::new(scoped(&a), |_, _| unreachable!()))
        .unwrap();
    let mut duplicate = a;
    duplicate.symbol = "different-label".into();
    assert!(
        runtime
            .register_host_function(HostFunction::new(scoped(&duplicate), |_, _| unreachable!()))
            .is_err()
    );
    assert_eq!(runtime.host().interface().records().functions.len(), 1);
}

#[test]
fn immutable_configuration_contracts_are_portable_and_reject_shared_objects() {
    let pure = declaration();
    let mut configuration = pure.clone();
    configuration.effects.may_read_immutable_configuration = true;
    assert_ne!(fingerprint(&pure), fingerprint(&configuration));
    assert!(!pure.matches_binding(&configuration));
    let interface = HostInterface {
        functions: vec![configuration.clone()],
        ..Default::default()
    };
    let bytes = encode(&interface).unwrap();
    let context = DefinitionContext::new().unwrap();
    let decoded = HostInterface::from_bytes(&bytes)
        .unwrap()
        .import_into(&context, &CancellationToken::default())
        .unwrap();
    let source = scoped(&interface)
        .import_into(&context, &CancellationToken::default())
        .unwrap();
    assert_eq!(decoded.records(), source.records());
    let mut old = bytes.clone();
    old[4..6].copy_from_slice(&5u16.to_le_bytes());
    assert_eq!(
        HostInterface::from_bytes(&old).unwrap_err(),
        HostInterfaceError::Version
    );
    for result in [
        HostValueType::Array(Box::new(HostValueType::I32)),
        HostValueType::Tuple(vec![HostValueType::Set(
            Box::new(HostValueType::I32),
            CollectionAccess::Mutable,
        )]),
        HostValueType::Option(
            kagari_types::language::binding::option_declaration(),
            Box::new(HostValueType::opaque("game.Object")),
        ),
    ] {
        let mut invalid = configuration.clone();
        invalid.return_type = result;
        assert_eq!(
            validate(&invalid),
            Err(HostInterfaceError::InvalidDeclaration)
        );
    }
    let mut invalid = configuration.clone();
    invalid.params[0].ty = HostValueType::Array(Box::new(HostValueType::I32));
    assert_eq!(
        validate(&invalid),
        Err(HostInterfaceError::InvalidDeclaration)
    );
    configuration.return_type = HostValueType::Option(
        kagari_types::language::binding::option_declaration(),
        Box::new(HostValueType::Tuple(vec![
            HostValueType::String,
            HostValueType::I32,
        ])),
    );
    validate(&configuration).unwrap();
}
