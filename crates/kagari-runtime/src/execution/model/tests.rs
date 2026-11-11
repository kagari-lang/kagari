use super::{
    RootedValue, ScopedValue, StoredValue,
    objects::{Kind, Object},
    services::{Control, NativeCall, NativeMark, NativeServices, NativeStatus, PendingReturn},
    source::{Bank, ExecutionId, ExecutionRoots, Location},
};
use crate::{
    Runtime,
    error::{RuntimeError, RuntimeErrorKind},
    interpreter_tests::common::compile_test_bytecode,
    module::VerifiedProgram,
    native::function_handle::Function,
    program::Program,
};
use kagari_common::identity::map::DefinitionContext;
use kagari_gc::{
    error::GcError,
    heap::{HeapConfig, Phase},
    object::{AnyObject, Gc},
    root_source::RootDriver,
    scan::WorkBudget,
};
use std::{
    mem::size_of,
    panic::{AssertUnwindSafe, catch_unwind},
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
};

fn location(execution: ExecutionId, bank: Bank, slot: usize) -> Location {
    Location {
        execution,
        bank,
        slot,
    }
}

fn managed(value: ScopedValue<'_>) -> Gc<'_, AnyObject> {
    match value {
        ScopedValue::Managed(value) => value,
        _ => panic!("expected managed value"),
    }
}

#[test]
fn tagged_transfers_publish_before_releasing_sources_and_survive_cursor_reuse()
-> Result<(), GcError> {
    let drops = Arc::new(AtomicUsize::new(0));
    let mut memory = RootDriver::<ExecutionRoots>::new(HeapConfig::default())?;
    let (id, reused) = memory.scope(|cx, roots| {
        ExecutionRoots::with(roots, cx, |slots| {
            let id = slots.enter(64)?;
            let reused = slots.enter(1)?;
            let a = Object::alloc(slots.scope(), Kind::Payload, 1, None, &drops)?;
            let b = Object::alloc(slots.scope(), Kind::Payload, 2, None, &drops)?;
            Object::set_next(slots.scope(), a, Kind::Payload, Some(b))?;
            slots.set(
                location(id, Bank::Managed, 63),
                ScopedValue::Managed(a.erase()),
            )?;
            slots.set(
                location(reused, Bank::Managed, 0),
                ScopedValue::Managed(b.erase()),
            )?;
            Ok((id, reused))
        })
    })?;
    assert_eq!(memory.stats().root_registrations, 0);
    memory.step(WorkBudget::new(8))?;
    assert_eq!(memory.stats().phase, Phase::Roots);
    let (host, replacement) = memory.scope(|cx, roots| {
        ExecutionRoots::with(roots, cx, |slots| {
            slots.copy(
                location(id, Bank::Managed, 0),
                location(id, Bank::Managed, 63),
            )?;
            slots.set(location(id, Bank::Managed, 63), ScopedValue::Unit)?;
            slots.grow(id, Bank::Managed, 8192)?;
            slots.leave(reused)?;
            let replacement = slots.enter(1)?;
            assert_ne!(replacement, reused);
            assert!(matches!(
                slots.get(location(reused, Bank::Managed, 0)),
                Err(GcError::Index)
            ));
            let a = slots.get(location(id, Bank::Managed, 0))?;
            let a = slots.scope().downcast::<Object>(managed(a))?;
            let c = Object::alloc(slots.scope(), Kind::Payload, 3, None, &drops)?;
            // A failed checked write preserves the old field and source.
            assert_eq!(
                Object::set_next(slots.scope(), a, Kind::Static, Some(c)),
                Err(GcError::Representation)
            );
            let old = Object::next(slots.scope(), a).unwrap();
            assert_eq!(slots.scope().read(old, |old| old.value), 2);
            Object::set_next(slots.scope(), a, Kind::Payload, Some(c))?;
            slots.set(
                location(replacement, Bank::Managed, 0),
                ScopedValue::Managed(c.erase()),
            )?;
            // Snapshot both sides before overlapping argument permutation.
            let first = slots.get(location(id, Bank::Managed, 0))?;
            let second = slots.get(location(replacement, Bank::Managed, 0))?;
            slots.set(location(id, Bank::Managed, 0), second)?;
            slots.set(location(replacement, Bank::Managed, 0), first)?;
            let host = first.retain(slots.scope())?;
            slots.set(location(id, Bank::Pending, 0), second)?;
            slots.leave(replacement)?;
            Ok((host, replacement))
        })
    })?;
    memory.collect()?;
    assert_eq!(memory.stats().objects, 2);
    assert_eq!(drops.load(Ordering::Relaxed), 1);
    assert_eq!(memory.stats().root_registrations, 1);
    let mut foreign = RootDriver::<ExecutionRoots>::new(HeapConfig::default())?;
    foreign.scope(|cx, _| {
        assert!(matches!(host.get(cx), Err(GcError::ForeignHeap)));
        Ok(())
    })?;
    memory.scope(|cx, roots| {
        ExecutionRoots::with(roots, cx, |slots| {
            assert!(matches!(
                slots.get(location(replacement, Bank::Managed, 0)),
                Err(GcError::Index)
            ));
            slots.set(location(id, Bank::Managed, 0), ScopedValue::I64(42))?;
            assert!(matches!(
                slots.get(location(id, Bank::Managed, 0))?,
                ScopedValue::I64(42)
            ));
            let rooted = host.get(slots.scope())?;
            let object = slots.scope().downcast::<Object>(managed(rooted))?;
            assert_eq!(slots.scope().read(object, |object| object.value), 1);
            assert!(matches!(
                ScopedValue::Unit
                    .retain(slots.scope())?
                    .get(slots.scope())?,
                ScopedValue::Unit
            ));
            assert!(matches!(
                ScopedValue::I64(-7)
                    .retain(slots.scope())?
                    .get(slots.scope())?,
                ScopedValue::I64(-7)
            ));
            Ok(())
        })
    })?;
    drop(host);
    memory.collect()?;
    assert_eq!(
        memory.stats().objects,
        1,
        "pending completion retains its result"
    );
    assert_eq!(drops.load(Ordering::Relaxed), 2);
    memory.scope(|cx, roots| ExecutionRoots::with(roots, cx, |slots| slots.leave(id)))?;
    memory.collect()?;
    assert_eq!(memory.stats().objects, 0);
    assert_eq!(drops.load(Ordering::Relaxed), 3);
    Ok(())
}

fn nested_native(call: &mut NativeCall<'_, '_>) -> NativeStatus {
    let call_argument = call.arguments[0];
    call.access(|slots, arguments| {
        let value = slots.get(arguments[0])?;
        let object = slots.scope().downcast::<Object>(managed(value))?;
        assert_eq!(slots.scope().read(object, |object| object.value), 41);
        Ok(())
    })
    .unwrap();
    call.services.control.effects += 1;
    call.stage(|slots| {
        let value = slots.get(call_argument)?;
        let object = slots.scope().downcast::<Object>(managed(value))?;
        let drops = slots.scope().read(object, Object::counter);
        let result = Object::alloc(slots.scope(), Kind::Payload, 42, None, &drops)?;
        Ok(ScopedValue::Managed(result.erase()))
    })
    .unwrap();
    call.collect().unwrap();
    Ok(())
}

/// Lend the real interpreter's scalar service, then complete through a nested
/// Native entry. No managed value crosses between the two independent heaps.
fn script_service(
    services: &mut NativeServices<'_>,
    id: ExecutionId,
    argument: Location,
    destination: Location,
) -> NativeStatus {
    let value = services
        .memory
        .scope(|cx, roots| {
            ExecutionRoots::with(roots, cx, |slots| {
                let value = slots.get(argument)?;
                let object = slots.scope().downcast::<Object>(managed(value))?;
                Ok(slots.scope().read(object, |object| object.value))
            })
        })
        .unwrap();
    assert_eq!((services.script)(value).map_err(Box::new)?, 42);
    services
        .memory
        .scope(|cx, roots| {
            ExecutionRoots::with(roots, cx, |slots| {
                slots.grow(id, Bank::Managed, 4096)?;
                slots.storage(id)?.scalars.resize(1024, 0);
                slots.storage(id)?.initialized.resize(1024, false);
                Ok(())
            })
        })
        .unwrap();
    let arguments = [argument];
    let mut nested = NativeCall::enter(services, id, &arguments, destination).unwrap();
    let outcome = nested_native(&mut nested);
    nested.finish(outcome)
}

#[test]
fn exclusive_native_services_reenter_collect_stage_and_retire_only_their_suffix()
-> Result<(), GcError> {
    let drops = Arc::new(AtomicUsize::new(0));
    let mut memory = RootDriver::<ExecutionRoots>::new(HeapConfig {
        max_roots: 0,
        ..HeapConfig::default()
    })?;
    let id = memory.scope(|cx, roots| {
        ExecutionRoots::with(roots, cx, |slots| {
            let id = slots.enter(3)?;
            let argument = Object::alloc(slots.scope(), Kind::Payload, 41, None, &drops)?;
            slots.set(
                location(id, Bank::Managed, 0),
                ScopedValue::Managed(argument.erase()),
            )?;
            Ok(id)
        })
    })?;
    let argument = location(id, Bank::Managed, 0);
    let destination = location(id, Bank::Managed, 1);
    let mut control = Control::default();
    let mut script_runtime = Runtime::default();
    let code = program(include_str!("../../../tests/fixtures/rr01_old.kgr"));
    let installed = script_runtime
        .load_program(&code, Default::default())
        .unwrap();
    let function = script_runtime
        .bind_function::<(i64,), i64>(&installed.root(), "increment")
        .unwrap();
    let mut script_calls = 0;
    let mut script = |value| {
        script_calls += 1;
        script_runtime.call(&function, (value,))
    };
    let mut services = NativeServices {
        memory: &mut memory,
        control: &mut control,
        script: &mut script,
    };
    let arguments = [argument];
    {
        let mut outer = NativeCall::enter(&mut services, id, &arguments, destination)?;
        outer.stage_argument(0)?;
        let outcome = script_service(outer.services, id, argument, location(id, Bank::Managed, 2));
        outer.stage_from(location(id, Bank::Managed, 2))?;
        outer.finish(outcome).unwrap();
        assert_eq!(
            outer.finish(Ok(())).unwrap_err().kind(),
            RuntimeErrorKind::ModuleValidation
        );
        outer.access(|slots, _| {
            let result = slots.get(destination)?;
            let result = slots.scope().downcast::<Object>(managed(result))?;
            assert_eq!(slots.scope().read(result, |result| result.value), 42);
            assert_eq!(
                slots.len(id, Bank::Local)?,
                2,
                "inner cleanup preserves outer protection"
            );
            assert_eq!(slots.storage(id)?.logical_calls.len(), 1);
            assert_eq!(slots.storage(id)?.scalars.len(), 1024);
            println!(
                "RR01 execution backing_bytes={}",
                slots.storage(id)?.backing_bytes()
            );
            Ok(())
        })?;
    }
    assert_eq!(services.control.effects, 1);
    for cancelled in [false, true] {
        services.memory.scope(|cx, roots| {
            ExecutionRoots::with(roots, cx, |slots| {
                slots.set(destination, ScopedValue::I64(-1))
            })
        })?;
        let mut call = NativeCall::enter(&mut services, id, &arguments, destination)?;
        call.services.control.effects += 1;
        call.stage_argument(0)?;
        call.services.control.cancelled = cancelled;
        let error = call
            .finish(Err(Box::new(RuntimeError::module_validation(
                "converter failed after effects",
            ))))
            .unwrap_err();
        assert_eq!(
            error.kind(),
            if cancelled {
                RuntimeErrorKind::Cancelled
            } else {
                RuntimeErrorKind::ModuleValidation
            }
        );
        drop(call);
        services.control.cancelled = false;
    }
    assert!(
        catch_unwind(AssertUnwindSafe(|| {
            let mut call = NativeCall::enter(&mut services, id, &arguments, destination).unwrap();
            call.stage_argument(0).unwrap();
            call.services.control.effects += 1;
            panic!("callback unwind after staging");
        }))
        .is_err()
    );
    services.memory.scope(|cx, roots| {
        ExecutionRoots::with(roots, cx, |slots| {
            assert_eq!(slots.len(id, Bank::Local)?, 0);
            assert!(slots.storage(id)?.logical_calls.is_empty());
            assert!(matches!(slots.get(destination)?, ScopedValue::I64(-1)));
            slots.leave(id)
        })
    })?;
    assert_eq!(services.control.effects, 4);
    assert_eq!(services.control.loans, 0);
    services.memory.collect()?;
    assert_eq!(services.memory.stats().root_registrations, 0);
    assert_eq!(services.memory.stats().objects, 0);
    assert_eq!(drops.load(Ordering::Relaxed), 2);
    assert_eq!(script_calls, 1);
    println!(
        "RR01 sizes scoped={} stored={} host={} pending={} mark={} call={} source={} scalar={} status={} location={}",
        size_of::<ScopedValue<'_>>(),
        size_of::<StoredValue>(),
        size_of::<RootedValue>(),
        size_of::<PendingReturn>(),
        size_of::<NativeMark>(),
        size_of::<NativeCall<'_, '_>>(),
        size_of::<ExecutionRoots>(),
        size_of::<u64>(),
        size_of::<NativeStatus>(),
        size_of::<Location>()
    );
    Ok(())
}

fn program(source: &str) -> Program {
    let context = DefinitionContext::new().unwrap();
    Program::from_verified(
        VerifiedProgram::new(compile_test_bytecode(source), &context).unwrap(),
        &Default::default(),
    )
    .unwrap()
}

#[test]
fn managed_code_static_cycle_dies_after_real_computation_and_publication_roots_leave()
-> Result<(), GcError> {
    let old = program(include_str!("../../../tests/fixtures/rr01_old.kgr"));
    let next = old
        .prepare_replacement(
            &program(include_str!("../../../tests/fixtures/rr01_new.kgr")),
            &Default::default(),
        )
        .unwrap();
    let drops = Arc::new(AtomicUsize::new(0));
    let mut memory = RootDriver::<ExecutionRoots>::new(HeapConfig::default())?;
    let (id, publication) = memory.scope(|cx, roots| {
        ExecutionRoots::with(roots, cx, |slots| {
            let id = slots.enter(0)?;
            let object = Object::alloc(slots.scope(), Kind::Payload, 1, None, &drops)?;
            let closure = Object::alloc(slots.scope(), Kind::Closure, 2, None, &drops)?;
            let revision =
                Object::alloc(slots.scope(), Kind::Revision, 10, Some(old.clone()), &drops)?;
            let state = Object::alloc(slots.scope(), Kind::Static, 3, None, &drops)?;
            for (owner, kind, target) in [
                (object, Kind::Payload, closure),
                (closure, Kind::Closure, revision),
                (revision, Kind::Revision, state),
                (state, Kind::Static, object),
            ] {
                Object::set_next(slots.scope(), owner, kind, Some(target))?;
            }
            slots.grow(id, Bank::Dependency, 1)?;
            slots.set(
                location(id, Bank::Dependency, 0),
                ScopedValue::Managed(closure.erase()),
            )?;
            Ok((
                id,
                ScopedValue::Managed(revision.erase()).retain(slots.scope())?,
            ))
        })
    })?;
    drop(old);
    memory.collect()?;
    assert_eq!(memory.stats().objects, 4);
    let current = memory.scope(|cx, _| {
        let revision = Object::alloc(cx, Kind::Revision, 20, Some(next), &drops)?;
        ScopedValue::Managed(revision.erase()).retain(cx)
    })?;
    drop(publication);
    memory.collect()?;
    assert_eq!(
        drops.load(Ordering::Relaxed),
        0,
        "retained computation keeps its entire cycle"
    );
    let (old_code, current_code) = memory.scope(|cx, roots| {
        ExecutionRoots::with(roots, cx, |slots| {
            let closure = slots.get(location(id, Bank::Dependency, 0))?;
            let closure = slots.scope().downcast::<Object>(managed(closure))?;
            let revision = Object::next(slots.scope(), closure).unwrap();
            let old_code = slots
                .scope()
                .read(revision, |revision| revision.code.clone().unwrap());
            let revision = current.get(slots.scope())?;
            let revision = slots.scope().downcast::<Object>(managed(revision))?;
            let current_code = slots
                .scope()
                .read(revision, |revision| revision.code.clone().unwrap());
            Ok((old_code, current_code))
        })
    })?;
    // Execute the retained immutable code products using the real interpreter;
    // only scalar results cross these independent heaps. The full single-heap
    // old-body/current-inner-call production route remains RR03 acceptance.
    let mut runtime = Runtime::default();
    let old_installation = runtime.load_program(&old_code, Default::default()).unwrap();
    let factory = runtime
        .bind_function::<(), Function<(), i64>>(&old_installation.root(), "computation")
        .unwrap();
    let retained = runtime.call(&factory, ()).unwrap();
    assert_eq!(
        runtime
            .execute_typed::<(), i64>(&old_installation, "body", (), &Default::default())
            .unwrap(),
        1
    );
    runtime
        .replace_program(&old_installation, &current_code, Default::default())
        .unwrap();
    runtime.collect_garbage().unwrap();
    assert_eq!(
        runtime.call(&retained, ()).unwrap(),
        120,
        "old closure body makes a current inner call"
    );
    let fresh = runtime.call(&factory, ()).unwrap();
    assert_eq!(runtime.call(&fresh, ()).unwrap(), 220);
    drop(retained);
    drop(fresh);
    drop(factory);
    drop(runtime);
    drop(old_code);
    drop(current_code);
    memory.scope(|cx, roots| ExecutionRoots::with(roots, cx, |slots| slots.leave(id)))?;
    memory.collect()?;
    assert_eq!(drops.load(Ordering::Relaxed), 4);
    assert_eq!(
        memory.stats().objects,
        1,
        "only current publication remains"
    );
    drop(current);
    memory.collect()?;
    assert_eq!(drops.load(Ordering::Relaxed), 5);
    assert_eq!(memory.stats().objects, 0);
    Ok(())
}
