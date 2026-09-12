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
fn offline_roundtrip_and_binding_link_do_not_run_callbacks() {
    let declaration = declaration();
    let interface = HostInterface {
        functions: vec![declaration.clone()],
    };
    let encoded = interface.to_bytes().unwrap();
    let decoded = HostInterface::from_bytes(&encoded).unwrap();
    assert_eq!(decoded, interface);
    let calls = Arc::new(AtomicUsize::new(0));
    let called = calls.clone();
    let mut runtime = Runtime::default();
    let id = runtime
        .register_host_function(HostFunction::new(declaration, move |_| {
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
        .register_host_function(HostFunction::new(declaration.clone(), |_| {
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
            functions: vec![docs],
        })
        .unwrap();
    let absent = HostFunctionDeclaration::new("missing.echo", vec![], HostValueType::Unit);
    assert!(
        runtime
            .host()
            .link_interface(&HostInterface {
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
        functions: vec![a.clone(), b.clone()],
    }
    .to_bytes()
    .unwrap();
    let second = HostInterface {
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
            .register_host_function(HostFunction::new(bad, |_| unreachable!()))
            .is_err()
    );
    assert!(runtime.host().interface().functions.is_empty());
    let a = declaration();
    runtime
        .register_host_function(HostFunction::new(a.clone(), |_| unreachable!()))
        .unwrap();
    let mut duplicate = a;
    duplicate.symbol = "different-label".into();
    assert!(
        runtime
            .register_host_function(HostFunction::new(duplicate, |_| unreachable!()))
            .is_err()
    );
    assert_eq!(runtime.host().interface().functions.len(), 1);
}
