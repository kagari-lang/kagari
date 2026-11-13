//! Carrier transport keeps independent scalar-bit, publication and destruction oracles.
mod payload;

use self::payload::Object;
use super::ScopedValue;
use crate::{
    execution::roots::{
        ExecutionRoots, SessionId as ExecutionId, ValueBank as Bank, ValueLocation as Location,
    },
    value::{aggregates::tuples::TupleObject, kind::ObjectKind, strings::StringObject},
};
use kagari_abi::representation::ValueType;
use kagari_gc::{
    error::GcError,
    heap::{HeapConfig, Phase},
    object::{AnyObject, Gc},
    root_source::RootDriver,
    scan::WorkBudget,
};
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

fn location(execution: ExecutionId, bank: Bank, slot: usize) -> Location {
    Location {
        execution,
        bank,
        slot,
    }
}

fn managed(value: ScopedValue<'_>) -> Gc<'_, AnyObject> {
    value.object().expect("expected managed value")
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
            let a = Object::alloc(slots.scope(), 1, &drops)?;
            let b = Object::alloc(slots.scope(), 2, &drops)?;
            Object::set_next(slots.scope(), a, Some(b));
            slots.set(location(id, Bank::Managed, 63), ScopedValue::managed(a))?;
            slots.set(location(reused, Bank::Managed, 0), ScopedValue::managed(b))?;
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
            slots.set(location(id, Bank::Managed, 63), ScopedValue::unit())?;
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
            let c = Object::alloc(slots.scope(), 3, &drops)?;
            let old = Object::next(slots.scope(), a).unwrap();
            assert_eq!(slots.scope().read(old, |old| old.value), 2);
            Object::set_next(slots.scope(), a, Some(c));
            slots.set(
                location(replacement, Bank::Managed, 0),
                ScopedValue::managed(c),
            )?;
            // Snapshot both sides before overlapping argument permutation.
            let first = slots.get(location(id, Bank::Managed, 0))?;
            let second = slots.get(location(replacement, Bank::Managed, 0))?;
            slots.set(location(id, Bank::Managed, 0), second)?;
            slots.set(location(replacement, Bank::Managed, 0), first)?;
            let host = first.retain(slots.scope())?;
            let text = StringObject::alloc(slots.scope(), "managed UTF-8 🙂".to_owned())?;
            let tuple = TupleObject::alloc(
                slots.scope(),
                &[second, ScopedValue::from(42i64), ScopedValue::managed(text)],
            )?;
            slots.set(location(id, Bank::Pending, 0), ScopedValue::managed(tuple))?;
            slots.leave(replacement)?;
            Ok((host, replacement))
        })
    })?;
    memory.collect()?;
    assert_eq!(memory.stats().objects, 4);
    assert_eq!(
        memory.stats().provider_units,
        21,
        "tuple fields plus UTF-8 bytes"
    );
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
            slots.set(location(id, Bank::Managed, 0), ScopedValue::from(42i64))?;
            assert_eq!(
                slots.get(location(id, Bank::Managed, 0))?.scalar_payload(),
                Some((ValueType::I64, 42))
            );
            let rooted = host.get(slots.scope())?;
            assert_eq!(rooted.kind(), Some(ObjectKind::Native));
            assert!(rooted.scalar_payload().is_none());
            let object = slots.scope().downcast::<Object>(managed(rooted))?;
            assert_eq!(slots.scope().read(object, |object| object.value), 1);
            assert!(
                ScopedValue::unit()
                    .retain(slots.scope())?
                    .get(slots.scope())?
                    .is_unit()
            );
            assert_eq!(
                ScopedValue::from(-7i64)
                    .retain(slots.scope())?
                    .get(slots.scope())?
                    .scalar_payload(),
                Some((ValueType::I64, 0xffff_ffff_ffff_fff9))
            );
            // Stored transport and Host reopening must preserve all ordinary
            // scalar tags, signed limits, NaN payloads and signed zero bits.
            let scalars = [
                (ScopedValue::unit(), (ValueType::Unit, 0)),
                (ScopedValue::from(true), (ValueType::Bool, 1)),
                (ScopedValue::from('🙂'), (ValueType::Char, 0x1f642)),
                (
                    ScopedValue::from(i32::MIN),
                    (ValueType::I32, 0xffff_ffff_8000_0000),
                ),
                (
                    ScopedValue::from(i64::MIN),
                    (ValueType::I64, 0x8000_0000_0000_0000),
                ),
                (ScopedValue::from(u64::MAX), (ValueType::U64, u64::MAX)),
                (
                    ScopedValue::from(f32::from_bits(0xffc0_1234)),
                    (ValueType::F32, 0xffc0_1234),
                ),
                (
                    ScopedValue::from(-0.0f64),
                    (ValueType::F64, 0x8000_0000_0000_0000),
                ),
            ];
            for (value, expected) in scalars {
                let at = location(id, Bank::Managed, 0);
                slots.set(at, value)?;
                let loaded = slots.get(at)?;
                assert_eq!(loaded.kind(), None);
                assert_eq!(loaded.scalar_payload(), Some(expected));
                let retained = loaded.retain(slots.scope())?;
                assert_eq!(
                    retained.clone().get(slots.scope())?.scalar_payload(),
                    Some(expected)
                );
            }
            Ok(())
        })
    })?;
    assert_eq!(
        memory.stats().root_registrations,
        1,
        "scalar retention registers no roots"
    );
    drop(host);
    memory.collect()?;
    assert_eq!(
        memory.stats().objects,
        3,
        "pending completion retains its tuple and children"
    );
    memory.scope(|cx, roots| {
        ExecutionRoots::with(roots, cx, |slots| {
            let pending = slots.get(location(id, Bank::Pending, 0))?;
            assert_eq!(pending.kind(), Some(ObjectKind::Tuple));
            assert!(matches!(
                slots.scope().downcast::<Object>(managed(pending)),
                Err(GcError::Representation)
            ));
            let tuple = slots.scope().downcast::<TupleObject>(managed(pending))?;
            let child = TupleObject::get(slots.scope(), tuple, 0)?;
            assert_eq!(child.kind(), Some(ObjectKind::Native));
            let child = slots.scope().downcast::<Object>(managed(child))?;
            assert_eq!(slots.scope().read(child, |child| child.value), 3);
            assert_eq!(
                TupleObject::get(slots.scope(), tuple, 1)?.scalar_payload(),
                Some((ValueType::I64, 42))
            );
            let text = TupleObject::get(slots.scope(), tuple, 2)?;
            assert_eq!(text.kind(), Some(ObjectKind::String));
            let text = slots.scope().downcast::<StringObject>(managed(text))?;
            assert!(
                slots
                    .scope()
                    .read(text, |text| text.as_str() == "managed UTF-8 🙂")
            );
            assert!(matches!(
                TupleObject::get(slots.scope(), tuple, 3),
                Err(GcError::Index)
            ));
            Ok(())
        })
    })?;
    assert_eq!(drops.load(Ordering::Relaxed), 2);
    memory.scope(|cx, roots| ExecutionRoots::with(roots, cx, |slots| slots.leave(id)))?;
    memory.collect()?;
    assert_eq!(memory.stats().objects, 0);
    assert_eq!(memory.stats().provider_units, 0);
    assert_eq!(drops.load(Ordering::Relaxed), 3);
    Ok(())
}
