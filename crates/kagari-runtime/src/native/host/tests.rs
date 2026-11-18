//! Exercise Host passing and heap-storage checks through source-free admission.
use crate::{
    Runtime,
    error::RuntimeErrorKind,
    host::{HostObjectId, HostSchemaEpoch},
    native::{
        builder::ModuleBuilder, conversion::context::ConversionContext, declarations::FunctionDecl,
        host::callback, types::Type,
    },
    test_support::{
        bytecode::load_bytecode,
        native::{invoke::invoke, program::free_function_program},
    },
    value::{aggregates::tuples::TupleObject, carriers::ScopedValue, view::ValueView},
};
use kagari_types::declaration::host::{
    HostCallPolicy, HostPassingStyle, HostReflectionPolicy, HostTypeOwnership, HostTypePolicy,
    PathAccess,
};
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

#[test]
fn host_roots_match_nominal_arguments_but_nested_roots_are_not_heap_data() {
    let mut runtime = Runtime::default();
    let mut builder = ModuleBuilder::new("fixtures::host_values", &Default::default()).unwrap();
    let [object, _] = ["Object", "Other"].map(|name| {
        let mut ty = builder.define_type(name).unwrap();
        ty.external(HostTypePolicy {
            ownership: HostTypeOwnership::HostRoot,
            reflection: HostReflectionPolicy::Hidden,
            path_access: PathAccess::ReadOnly,
        })
        .unwrap();
        ty.finish().unwrap().apply([]).unwrap()
    });
    let nested = Type::tuple([Type::tuple([object.clone()]).unwrap()]).unwrap();
    let consume = builder
        .define_function(
            FunctionDecl::new("consume")
                .parameter("value", nested)
                .host_policy(HostCallPolicy {
                    passing: vec![HostPassingStyle::Owned],
                    effects: Default::default(),
                }),
        )
        .unwrap();
    builder
        .bind(consume, callback(|_, _| Ok(ValueView::unit())))
        .unwrap();
    let shared = builder
        .define_function(
            FunctionDecl::new("shared")
                .parameter("value", object.clone())
                .host_policy(HostCallPolicy {
                    passing: vec![HostPassingStyle::SharedBorrow],
                    effects: Default::default(),
                }),
        )
        .unwrap();
    let calls = Arc::new(AtomicUsize::new(0));
    let observed = calls.clone();
    builder
        .bind(
            shared,
            callback(move |_, _| {
                observed.fetch_add(1, Ordering::SeqCst);
                Ok(ValueView::unit())
            }),
        )
        .unwrap();
    let provider = builder.finish().unwrap();
    let program = free_function_program(&provider.to_declaration());
    provider.install(&mut runtime).unwrap();
    let installed = load_bytecode(&mut runtime, "host-values", program).unwrap();
    let owner = runtime.select_program(&installed).unwrap();
    let [root, other] = [(1, "Object"), (2, "Other")].map(|(id, name)| {
        let ty = runtime
            .types()
            .get_by_name(&format!("fixtures::host_values::{name}"))
            .unwrap()
            .id;
        let handle = runtime
            .register_host_root(HostObjectId(id), ty, HostSchemaEpoch::new(0))
            .unwrap();
        runtime.host_root_value(handle).unwrap()
    });
    let nested = runtime
        .memory
        .scope(&runtime.resources, |cx, _| {
            let value = root.get(cx)?;
            let inner = TupleObject::alloc(cx, &[value])?;
            let outer = TupleObject::alloc(cx, &[ScopedValue::managed(inner)])?;
            ScopedValue::managed(outer).retain(cx)
        })
        .unwrap();
    let _ = invoke(&mut runtime, &owner, 0, std::slice::from_ref(&nested)).unwrap();
    let expected = runtime
        .resolve_type_arguments(&owner, &owner.bytecode.native_imports[0].signature.params)
        .unwrap()
        .remove(0);
    {
        let mut cx = ConversionContext::new(&mut runtime, &owner).unwrap();
        let nested = cx.import(&nested).unwrap();
        cx.check_value(&expected, &nested).unwrap();
        let error = cx.allocate_array(&expected, &[nested]).unwrap_err();
        assert_eq!(error.kind(), RuntimeErrorKind::ModuleValidation);
        assert!(error.to_string().contains("closed script-heap type"));
    }
    let _ = invoke(&mut runtime, &owner, 1, &[root]).unwrap();
    let error = invoke(&mut runtime, &owner, 1, &[other]).unwrap_err();
    assert_eq!(error.kind(), RuntimeErrorKind::HostCallFailure);
    assert!(error.to_string().contains("host arguments do not match"));
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert!(!runtime.is_quarantined());
}
