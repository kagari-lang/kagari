use crate::{
    Runtime,
    error::{RuntimeError, trap::TrapReason},
    execution::roots::ExecutionRoots,
    execution_metadata::managed::sources::{MetadataError, MetadataSource},
    frame::ExecutionStack,
    host::HostObjectId,
    module::managed::source::RevisionSource,
    native::{
        binding::NativeResult,
        completion::{Completion, CompletionRegistry},
        conversion::{arguments::ArgumentValues, context::ConversionContext},
        future::{
            ColdFuture, NativeFuture, NativeStart, PendingNative, Producer, TypedProducer,
            storage::{ColdObject, FuturePayload},
        },
        types::Type,
    },
    test_support::{
        future::isolated_provider,
        layouts::{allocation_owner, host_values},
        parking_graph::{Graphs, Probe},
    },
    value::{carriers::ScopedValue, host_values::HostRootObject, strings::StringObject},
};
use kagari_common::identity::{DefinitionKind, ModuleIdentity, PackageId};
use kagari_gc::{error::GcError, object::GcObject, root::Root};
use kagari_types::{scalar::BuiltinType, ty::Ty};
use std::{
    marker::PhantomData,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    task::{Poll, Waker},
};

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
    let id = HostObjectId(7);
    drop(host_values(&mut runtime, id.0));
    let root = runtime.host().root(id).unwrap();
    let owner = allocation_owner(&mut runtime);
    let definitions = owner.definitions();
    let module = definitions
        .lookup_root(&ModuleIdentity {
            package: PackageId("fixtures".into()),
            path: vec![],
        })
        .unwrap();
    let player = definitions
        .lookup_child(module, DefinitionKind::Struct, "Player", 0)
        .unwrap();
    runtime.collect_garbage().unwrap();
    let baseline = runtime.gc_stats();
    let types = runtime
        .resolve_type_arguments(
            &owner,
            &[
                Ty::Builtin(BuiltinType::String),
                Ty::Host(player),
                Ty::Builtin(BuiltinType::Unit),
            ],
        )
        .unwrap();
    let mut parameters = Vec::with_capacity(8);
    parameters.extend(types[..2].iter().cloned());
    let recipe = ColdFuture::Native(NativeFuture {
        origin: None,
        owner: owner.clone(),
        parameters,
        output: types[2].clone(),
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
                let host = HostRootObject::alloc(slots.scope(), root)?;
                let cold = ColdObject::alloc(
                    slots.scope(),
                    recipe,
                    body,
                    &[ScopedValue::managed(text), ScopedValue::managed(host)],
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
    assert_eq!(before.objects, baseline.objects + 4);
    assert!(before.indirect_bytes > baseline.indirect_bytes);
    runtime.retire_host_root(root).unwrap();
    let replacement = runtime
        .register_host_root(HostObjectId(8), root.type_id(), root.schema_epoch())
        .unwrap();
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
    let stale = runtime
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
            let stale = ColdObject::capture(cx, cold, 1)?;
            assert_eq!(
                cx.read(
                    cx.downcast::<HostRootObject>(stale.object().unwrap())?,
                    HostRootObject::handle
                ),
                root
            );
            assert!(cx.read(cold, GcObject::charge).units >= 3);
            stale.retain(cx)
        })
        .unwrap();
    assert!(runtime.host_scope(&[stale]).is_err());
    assert_eq!(
        runtime.host().root(replacement.object_id()),
        Some(replacement)
    );
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
fn pending_native_repark_rechecks_its_only_protected_receiver() {
    let mut runtime = Runtime::default();
    let provider = isolated_provider();
    let future_type = provider
        .catalog()
        .future_type()
        .unwrap()
        .apply([Type::i32()])
        .unwrap();
    provider.install(&mut runtime).unwrap();
    let graphs = Graphs::install(&mut runtime);
    let probe = Arc::new(Probe::default());
    let receiver = graphs.allocate(&mut runtime, 1, false, &probe);
    let interface = graphs.interface(&mut runtime, &receiver);
    let owner = runtime
        .interface_description(&interface)
        .unwrap()
        .implementation
        .clone();
    let applied = runtime.native_type_argument(&owner, &future_type).unwrap();
    let output = applied.parameter(&runtime, &owner, 0).unwrap();
    let (_, path) = host_values(&mut runtime, 7);
    let cancelled = Arc::new(AtomicUsize::new(0));
    let on_cancel = cancelled.clone();
    let recipe = ColdFuture::Native(NativeFuture {
        origin: None,
        owner: owner.clone(),
        parameters: vec![output.clone()],
        output,
        producer: Arc::new(TypedProducer {
            marker: PhantomData,
            entry: move |(input,): (i32,), _: Completion<i32>| {
                assert_eq!(input, 42);
                let on_cancel = on_cancel.clone();
                Ok(NativeStart::cancellable(move || {
                    on_cancel.fetch_add(1, Ordering::SeqCst);
                }))
            },
        }),
    });
    let source = runtime.memory.enter_execution(&runtime.resources).unwrap();
    let future = {
        let mut cx = ConversionContext::for_selected(
            &mut runtime,
            &owner,
            source,
            RevisionSource::Current,
            Some(MetadataSource::RootedInterface(&interface)),
        )
        .unwrap();
        let future = cx
            .allocate_future(&applied, recipe, &[42i32.into()])
            .unwrap();
        cx.retain(&future).unwrap()
    };
    runtime
        .memory
        .retire_execution(&runtime.resources, source)
        .unwrap();
    drop(interface);
    let execution = runtime
        .start_owned_future(&future, Default::default())
        .unwrap();
    drop(future);
    let mut activation = Some(runtime.resume_owned_execution(&execution).unwrap());
    activation
        .as_mut()
        .unwrap()
        .with(|runtime, stack| {
            assert!(matches!(stack.poll_await(runtime)?, Poll::Pending));
            Ok(())
        })
        .unwrap();
    ExecutionStack::park_waiting(&mut activation).unwrap();
    drop(activation);

    // Submission cleared the scalar capture and released the awaitable slot.
    // Only the pending result converter retains this interface and its receiver.
    graphs.set_child(&mut runtime, &receiver, &path);
    drop((receiver, path));
    runtime.collect_garbage().unwrap();
    assert_eq!(probe.drops.load(Ordering::Relaxed), 0);
    let mut activation = Some(runtime.resume_owned_execution(&execution).unwrap());
    activation
        .as_mut()
        .unwrap()
        .with(|runtime, stack| {
            assert!(matches!(stack.poll_await(runtime)?, Poll::Pending));
            Ok(())
        })
        .unwrap();
    assert_eq!(
        ExecutionStack::park_waiting(&mut activation).unwrap_err(),
        RuntimeError::from(TrapReason::AsyncStateRetainsAnInvalidOrScopedValue)
    );
    assert!(activation.is_some());
    drop(activation);
    runtime.finish_owned_execution(&execution).unwrap();
    assert_eq!(cancelled.load(Ordering::SeqCst), 1);
    runtime.collect_garbage().unwrap();
    assert_eq!(probe.drops.load(Ordering::Relaxed), 1);
    assert!(!runtime.is_quarantined());
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
