use kagari_common::host_interface::{
    HostFunctionDeclaration, HostParameter, HostPassingStyle, HostValueType as Type,
};
use kagari_runtime::{
    Runtime, RuntimeConfig,
    host::HostFunction,
    value::{EnumTag, Value},
};
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

fn runtime() -> Runtime {
    Runtime::new(RuntimeConfig {
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
    })
}

fn echo(name: &str, ty: Type) -> HostFunctionDeclaration {
    HostFunctionDeclaration::new(
        name,
        vec![HostParameter {
            name: "value".into(),
            ty: ty.clone(),
            passing: HostPassingStyle::Owned,
        }],
        ty,
    )
}

#[test]
fn nested_arguments_and_results_obey_the_complete_host_signature() {
    let mut runtime = runtime();
    let array = Value::Array(runtime.alloc_array(vec![Value::I32(7)]).unwrap());
    let wrong_array = Value::Array(runtime.alloc_array(vec![Value::Bool(true)]).unwrap());
    let some = |value| {
        Value::Enum(
            runtime
                .alloc_enum(EnumTag::OptionSome, vec![value])
                .unwrap(),
        )
    };
    let result = |tag, value| Value::Enum(runtime.alloc_enum(tag, vec![value]).unwrap());
    let cases = [
        (
            Type::Tuple(vec![Type::Array(Box::new(Type::I32)), Type::String]),
            Value::Tuple(vec![array.clone(), Value::Str("ok".into())]),
            Value::Tuple(vec![wrong_array.clone(), Value::Str("ok".into())]),
        ),
        (
            Type::Array(Box::new(Type::I32)),
            array.clone(),
            wrong_array.clone(),
        ),
        (
            Type::Map {
                key: Box::new(Type::String),
                value: Box::new(Type::Array(Box::new(Type::I32))),
            },
            Value::Map(
                runtime
                    .alloc_map(vec![(Value::Str("k".into()), array.clone())])
                    .unwrap(),
            ),
            Value::Map(
                runtime
                    .alloc_map(vec![(Value::Str("k".into()), wrong_array)])
                    .unwrap(),
            ),
        ),
        (
            Type::Set(Box::new(Type::String)),
            Value::Set(runtime.alloc_set(vec![Value::Str("ok".into())]).unwrap()),
            Value::Set(runtime.alloc_set(vec![Value::I32(7)]).unwrap()),
        ),
        (
            Type::Option(Box::new(Type::I32)),
            some(Value::I32(7)),
            some(Value::Bool(true)),
        ),
        (
            Type::Option(Box::new(Type::I32)),
            Value::Enum(runtime.alloc_enum(EnumTag::OptionNone, vec![]).unwrap()),
            result(EnumTag::ResultOk, Value::I32(7)),
        ),
        (
            Type::Result {
                ok: Box::new(Type::I32),
                error: Box::new(Type::String),
            },
            result(EnumTag::ResultOk, Value::I32(7)),
            result(EnumTag::ResultErr, Value::I32(7)),
        ),
        (
            Type::Result {
                ok: Box::new(Type::I32),
                error: Box::new(Type::String),
            },
            result(EnumTag::ResultErr, Value::Str("error".into())),
            some(Value::I32(7)),
        ),
    ];
    for (index, (ty, good, bad)) in cases.into_iter().enumerate() {
        let calls = Arc::new(AtomicUsize::new(0));
        let called = calls.clone();
        let id = runtime
            .register_host_function(HostFunction::new(
                echo(&format!("host.echo{index}"), ty.clone()),
                move |_, args| {
                    called.fetch_add(1, Ordering::SeqCst);
                    Ok(args[0].clone())
                },
            ))
            .unwrap();
        assert_eq!(
            runtime
                .invoke_bound_host(id, std::slice::from_ref(&good))
                .unwrap(),
            good
        );
        assert!(
            runtime
                .invoke_bound_host(id, std::slice::from_ref(&bad))
                .is_err()
        );
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        let called = calls.clone();
        let output = runtime
            .register_host_function(HostFunction::new(
                HostFunctionDeclaration::new(format!("host.bad{index}"), vec![], ty),
                move |_, _| {
                    called.fetch_add(1, Ordering::SeqCst);
                    Ok(bad.clone())
                },
            ))
            .unwrap();
        assert!(runtime.invoke_bound_host(output, &[]).is_err());
        assert_eq!(calls.load(Ordering::SeqCst), 2);
    }
}

#[test]
fn composite_arguments_are_rooted_during_callbacks_and_reject_foreign_or_stale_handles() {
    let mut runtime = runtime();
    let calls = Arc::new(AtomicUsize::new(0));
    let called = calls.clone();
    let id = runtime
        .register_host_function(HostFunction::new(
            echo(
                "host.echo",
                Type::Tuple(vec![Type::Array(Box::new(Type::I32))]),
            ),
            move |context, args| {
                called.fetch_add(1, Ordering::SeqCst);
                context.runtime().collect_garbage().unwrap();
                Ok(args[0].clone())
            },
        ))
        .unwrap();
    let array = runtime.alloc_array(vec![Value::I32(7)]).unwrap();
    let value = Value::Tuple(vec![Value::Array(array)]);
    assert_eq!(
        runtime
            .invoke_bound_host(id, std::slice::from_ref(&value))
            .unwrap(),
        value
    );
    assert!(runtime.gc().array_snapshot(array).is_some());
    let other = Runtime::default();
    let foreign = Value::Tuple(vec![Value::Array(
        other.alloc_array(vec![Value::I32(7)]).unwrap(),
    )]);
    assert!(runtime.invoke_bound_host(id, &[foreign]).is_err());
    runtime.collect_garbage().unwrap();
    assert!(runtime.gc().array_snapshot(array).is_none());
    assert!(runtime.invoke_bound_host(id, &[value]).is_err());
    assert_eq!(calls.load(Ordering::SeqCst), 1);
}

#[test]
fn owned_composites_cannot_hide_frame_scoped_host_borrows() {
    use kagari_runtime::{HostObjectId, RuntimeErrorKind, TypeId};
    let mut runtime = runtime();
    let id = runtime
        .register_host_function(HostFunction::new(
            HostFunctionDeclaration::new(
                "host.consume",
                vec![HostParameter {
                    name: "value".into(),
                    ty: Type::Tuple(vec![Type::Tuple(vec![Type::opaque("host.Object")])]),
                    passing: HostPassingStyle::Owned,
                }],
                Type::Unit,
            ),
            |_, _| panic!("an owned parameter must reject nested borrows before the callback"),
        ))
        .unwrap();
    for unique in [false, true] {
        let scope = runtime.host_scope(&[]).unwrap();
        let value = if unique {
            Value::host_mut(
                scope
                    .borrows()
                    .borrow_unique(HostObjectId(1), TypeId::new(0))
                    .unwrap(),
            )
        } else {
            Value::host_ref(
                scope
                    .borrows()
                    .borrow_shared(HostObjectId(1), TypeId::new(0))
                    .unwrap(),
            )
        };
        let value = Value::Tuple(vec![Value::Tuple(vec![value])]);
        assert_eq!(
            runtime
                .invoke_bound_host(id, std::slice::from_ref(&value))
                .unwrap_err()
                .kind(),
            RuntimeErrorKind::HostBorrowEscape
        );
        assert!(runtime.alloc_array(vec![value.clone()]).is_err());
        drop(scope);
        assert!(runtime.invoke_bound_host(id, &[value]).is_err());
    }
    assert!(!runtime.is_quarantined());
}
