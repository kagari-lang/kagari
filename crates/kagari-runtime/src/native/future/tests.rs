use crate::{
    Runtime,
    execution::roots::ExecutionRoots,
    execution_metadata::managed::sources::MetadataError,
    module::managed::source::RevisionSource,
    native::{
        binding::NativeResult,
        completion::CompletionRegistry,
        conversion::{arguments::ArgumentValues, context::ConversionContext},
        future::{
            ColdFuture, NativeFuture, PendingNative, Producer,
            storage::{ColdObject, FuturePayload},
        },
    },
    test_support::layouts::allocation_owner,
    value::{carriers::ScopedValue, strings::StringObject},
};
use kagari_gc::{error::GcError, object::GcObject, root::Root};
use kagari_types::{scalar::BuiltinType, ty::Ty};
use std::{sync::Arc, task::Waker};

#[derive(Debug)]
struct Unstarted;

impl Producer for Unstarted {
    fn start<'call>(
        &self,
        _: &mut ConversionContext<'call>,
        _: &NativeFuture,
        _: ArgumentValues<'call>,
        _: &Root<ColdObject>,
        _: &CompletionRegistry,
        _: &Waker,
    ) -> NativeResult<Box<dyn PendingNative>> {
        panic!("claiming must not convert or submit a producer")
    }
}

fn storage_error(error: MetadataError) -> GcError {
    match error {
        MetadataError::Memory(error) => error,
        MetadataError::Execution(error) => panic!("unexpected execution error: {error}"),
    }
}

#[test]
fn cold_claim_and_capture_handoff_keep_their_distinct_lifetimes() {
    let mut runtime = Runtime::default();
    let owner = allocation_owner(&mut runtime);
    runtime.collect_garbage().unwrap();
    let baseline = runtime.gc_stats();
    let types = runtime
        .resolve_type_arguments(
            &owner,
            &[
                Ty::Builtin(BuiltinType::String),
                Ty::Builtin(BuiltinType::Unit),
            ],
        )
        .unwrap();
    let mut parameters = Vec::with_capacity(8);
    parameters.extend(types.iter().cloned());
    let recipe = ColdFuture::Native(NativeFuture {
        origin: None,
        owner: owner.clone(),
        parameters,
        output: types[1].clone(),
        producer: Arc::new(Unstarted),
    });
    let Runtime {
        memory,
        modules,
        resources,
        ..
    } = &mut runtime;
    let future = memory
        .scope(resources, |cx, roots| {
            ExecutionRoots::with(roots, cx, |slots| {
                let body = RevisionSource::Current.resolve(modules, slots, &owner)?;
                let text = StringObject::alloc(slots.scope(), "retained cold input".into())?;
                let cold = ColdObject::alloc(
                    slots.scope(),
                    recipe,
                    body,
                    &[ScopedValue::managed(text), ScopedValue::unit()],
                    &[],
                    resources,
                )
                .map_err(storage_error)?;
                let payload = FuturePayload::alloc(slots.scope(), cold)?;
                slots.scope().retain(payload)
            })
        })
        .unwrap();
    runtime.collect_garbage().unwrap();
    let before = runtime.gc_stats();
    assert_eq!(before.objects, baseline.objects + 3);
    assert!(before.indirect_bytes > baseline.indirect_bytes);
    let claim = runtime
        .memory
        .scope(&runtime.resources, |cx, _| {
            let payload = cx.get(&future)?;
            FuturePayload::claim(cx, payload)?.ok_or(GcError::Representation)
        })
        .unwrap();
    assert_eq!(
        runtime.gc_stats().root_registrations,
        before.root_registrations + 1
    );
    runtime.collect_garbage().unwrap();
    assert_eq!(runtime.gc_stats().objects, before.objects);
    // Claim transfers ownership, not backing into an untraced Rust value pack.
    assert_eq!(runtime.gc_stats().indirect_bytes, before.indirect_bytes);
    runtime
        .memory
        .scope(&runtime.resources, |cx, _| {
            let payload = cx.get(&future)?;
            assert!(FuturePayload::cold(cx, payload).is_none());
            assert!(FuturePayload::claim(cx, payload)?.is_none());
            let cold = cx.get(&claim)?;
            assert_eq!(cx.read(cold, ColdObject::len), 2);
            assert!(matches!(
                cx.read(cold, |cold| cold.recipe().clone()).as_ref(),
                ColdFuture::Native(native) if native.origin.is_none()
            ));
            let captured = ColdObject::capture(cx, cold, 0)?;
            StringObject::read(cx, captured, |text| assert_eq!(text, "retained cold input"))?;
            assert!(ColdObject::capture(cx, cold, 1)?.is_unit());
            assert!(cx.read(cold, GcObject::charge).units >= 3);
            Ok(())
        })
        .unwrap();
    runtime
        .memory
        .scope(&runtime.resources, |cx, _| {
            let cold = cx.get(&claim)?;
            ColdObject::release_captures(cx, cold)
        })
        .unwrap();
    runtime.collect_garbage().unwrap();
    assert_eq!(runtime.gc_stats().objects, baseline.objects + 2);
    assert!(runtime.gc_stats().indirect_bytes < before.indirect_bytes);
    assert!(runtime.gc_stats().indirect_bytes > baseline.indirect_bytes);
    drop(claim);
    runtime.collect_garbage().unwrap();
    assert_eq!(runtime.gc_stats().objects, baseline.objects + 1);
    assert_eq!(runtime.gc_stats().indirect_bytes, baseline.indirect_bytes);
    drop(future);
    runtime.collect_garbage().unwrap();
    assert_eq!(runtime.gc_stats().objects, baseline.objects);
    assert_eq!(
        runtime.gc_stats().root_registrations,
        baseline.root_registrations
    );
}

#[test]
fn pair_claim_admits_both_states_before_either_transition() {
    let mut runtime = Runtime::default();
    let owner = allocation_owner(&mut runtime);
    runtime.collect_garbage().unwrap();
    let baseline = runtime.gc_stats();
    let output = runtime
        .resolve_type_arguments(&owner, &[Ty::Builtin(BuiltinType::Unit)])
        .unwrap()
        .remove(0);
    let Runtime {
        memory,
        modules,
        resources,
        ..
    } = &mut runtime;
    let [left, consumed, right] = memory
        .scope(resources, |cx, roots| {
            ExecutionRoots::with(roots, cx, |slots| {
                let body = RevisionSource::Current.resolve(modules, slots, &owner)?;
                let mut make = || {
                    let cold = ColdObject::alloc(
                        slots.scope(),
                        ColdFuture::Native(NativeFuture {
                            origin: None,
                            owner: owner.clone(),
                            parameters: Vec::new(),
                            output: output.clone(),
                            producer: Arc::new(Unstarted),
                        }),
                        body,
                        &[],
                        &[],
                        resources,
                    )
                    .map_err(storage_error)?;
                    let future = FuturePayload::alloc(slots.scope(), cold)?;
                    slots.scope().retain(future)
                };
                Ok([make()?, make()?, make()?])
            })
        })
        .unwrap();
    let claims = runtime
        .memory
        .scope(&runtime.resources, |cx, _| {
            let consumed = cx.get(&consumed)?;
            let previous = FuturePayload::claim(cx, consumed)?.unwrap();
            let left = cx.get(&left)?;
            assert!(FuturePayload::claim_pair(cx, [left, consumed])?.is_none());
            assert!(FuturePayload::cold(cx, left).is_some());
            assert!(matches!(
                FuturePayload::claim_pair(cx, [left, left]),
                Err(GcError::Representation)
            ));
            assert!(FuturePayload::cold(cx, left).is_some());
            let right = cx.get(&right)?;
            let pair = FuturePayload::claim_pair(cx, [left, right])?.unwrap();
            assert!(FuturePayload::cold(cx, left).is_none());
            assert!(FuturePayload::cold(cx, right).is_none());
            Ok((previous, pair))
        })
        .unwrap();
    runtime.collect_garbage().unwrap();
    assert_eq!(runtime.gc_stats().objects, baseline.objects + 6);
    drop(claims);
    runtime.collect_garbage().unwrap();
    assert_eq!(runtime.gc_stats().objects, baseline.objects + 3);
    drop((left, consumed, right));
    runtime.collect_garbage().unwrap();
    assert_eq!(runtime.gc_stats().objects, baseline.objects);
    assert_eq!(
        runtime.gc_stats().root_registrations,
        baseline.root_registrations
    );
}
