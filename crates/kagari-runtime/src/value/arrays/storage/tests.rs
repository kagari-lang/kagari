//! Physical storage proofs; nominal element and Runtime admission live above here.
use crate::value::{
    arrays::storage::{ArrayValues, PackedStorage, ScopedBuffer},
    carriers::{RootedValue, ScopedValue, StoredValue},
    scalar::elements::Primitive,
    strings::StringObject,
};
use kagari_gc::{
    access::Scope,
    error::GcError,
    heap::{Heap, Phase},
    object::{Gc, GcObject, ObjectCharge},
    scan::{EdgeVisitor, ScanBudget, ScanKind, ScanStatus, WorkBudget},
};
use kagari_types::{scalar::BuiltinType, ty::Ty};

/// Only owns the production storage kernel. It has no nominal type or Runtime
/// behavior and does not replace ArrayObject in production or integration tests.
struct Buffer(PackedStorage);

impl Buffer {
    fn alloc<'gc>(cx: &mut Scope<'gc>, values: ArrayValues<'gc>) -> Result<Gc<'gc, Self>, GcError> {
        // SAFETY: every initialized cell belongs to this fully scanned buffer.
        unsafe { cx.alloc_with(|mut init| Ok(Self(values.initialize(&mut init)?))) }
    }

    fn cursor<'gc>(cx: &mut Scope<'gc>, owner: Gc<'gc, Self>) -> Result<Option<usize>, GcError> {
        // SAFETY: copies pointer-free diagnostic state without editing any field.
        unsafe {
            cx.with_object_mut(owner, |_, mutation| {
                let cursor = mutation.with_scan_state(|state| state.copied());
                mutation.continue_scan();
                Ok(cursor)
            })
        }
    }
}

// SAFETY: the retained production buffer owns all cells, enumerates them through
// its real scanner, and charges its actual capacity. No peer-aware destructor.
unsafe impl GcObject for Buffer {
    type ScanState = usize;

    const SCAN_KIND: ScanKind = ScanKind::Resumable;

    fn begin_scan(&self) -> usize {
        0
    }

    fn scan(
        &self,
        next: &mut usize,
        budget: &mut ScanBudget,
        visitor: &mut EdgeVisitor<'_>,
    ) -> Result<ScanStatus, GcError> {
        // SAFETY: this admitted provider retains the entire initialized buffer.
        unsafe { self.0.scan(next, budget, visitor) }
    }

    fn charge(&self) -> ObjectCharge {
        ObjectCharge {
            bytes: self.0.retained_bytes(),
            units: self.0.len(),
        }
    }
}

#[test]
fn primitive_buffers_check_domains_and_preserve_bits() -> Result<(), GcError> {
    let mut heap = Heap::default();
    heap.scope(|cx| {
        let mut narrow = ArrayValues::empty(&Ty::Builtin(BuiltinType::I8));
        narrow.push(cx, ScopedValue::from(-128i32))?;
        narrow.push(cx, ScopedValue::from(127i32))?;
        assert_eq!(
            narrow.push(cx, ScopedValue::from(128i32)),
            Err(GcError::Representation)
        );
        assert_eq!(narrow.len(), 2);
        let owner = Buffer::alloc(cx, narrow)?;
        let first = PackedStorage::get(cx, owner, |owner| &owner.0, 0)?;
        assert_eq!(i8::decode(first)?, -128);
        assert!(matches!(
            PackedStorage::get(cx, owner, |owner| &owner.0, 2),
            Err(GcError::Index)
        ));

        let mut wide = ArrayValues::empty(&Ty::Builtin(BuiltinType::U64));
        wide.push(cx, ScopedValue::from(u64::MAX))?;
        let owner = Buffer::alloc(cx, wide)?;
        assert_eq!(
            u64::decode(PackedStorage::get(cx, owner, |owner| &owner.0, 0)?)?,
            u64::MAX
        );

        let mut floats = ArrayValues::empty(&Ty::Builtin(BuiltinType::F32));
        // Negative zero and a noncanonical NaN payload must survive compact storage.
        for bits in [0x8000_0000, 0x7fc0_0123] {
            floats.push(cx, ScopedValue::from(f32::from_bits(bits)))?;
        }
        let owner = Buffer::alloc(cx, floats)?;
        for (index, bits) in [0x8000_0000, 0x7fc0_0123].into_iter().enumerate() {
            assert_eq!(
                f32::decode(PackedStorage::get(cx, owner, |owner| &owner.0, index)?)?.to_bits(),
                bits
            );
        }

        let mut units = ArrayValues::empty(&Ty::Builtin(BuiltinType::Unit));
        assert_eq!(
            units.reserve(usize::MAX),
            Err(GcError::Limit("array length"))
        );
        assert_eq!(units.len(), 0);
        units.repeat(cx, ScopedValue::unit(), 3)?;
        let owner = Buffer::alloc(cx, units)?;
        assert_eq!(cx.read(owner, |owner| owner.0.retained_bytes()), 0);
        assert_eq!(cx.read(owner, |owner| owner.0.len()), 3);
        Ok(())
    })
}

#[test]
fn staged_copy_has_memmove_semantics_and_rejects_before_mutation() -> Result<(), GcError> {
    let mut heap = Heap::default();
    heap.scope(|cx| {
        let mut values = ArrayValues::empty(&Ty::Builtin(BuiltinType::I32));
        for value in 0..5i32 {
            values.push(cx, ScopedValue::from(value))?;
        }
        let owner = Buffer::alloc(cx, values)?;
        let mut copy = ArrayValues::empty(&Ty::Builtin(BuiltinType::I32));
        // SAFETY: this actual retained provider owns all initialized source cells.
        unsafe {
            PackedStorage::copy_into(cx, owner, |owner| &owner.0, 0..4, &mut copy)?;
        }
        // SAFETY: checked full range/kind; publication precedes the fixed write.
        unsafe {
            cx.with_object_mut(owner, |owner, mutation| {
                let staged = copy.publish(mutation)?;
                owner.0.overwrite(1, staged);
                mutation.continue_scan();
                Ok(())
            })?;
        }
        let mut rejected = ArrayValues::empty(&Ty::Builtin(BuiltinType::I32));
        // SAFETY: same retained provider; the invalid range must fail before copying.
        assert_eq!(
            unsafe { PackedStorage::copy_into(cx, owner, |owner| &owner.0, 0..6, &mut rejected) },
            Err(GcError::Index)
        );
        assert_eq!(rejected.len(), 0);
        for (index, expected) in [0, 0, 1, 2, 3].into_iter().enumerate() {
            assert_eq!(
                i32::decode(PackedStorage::get(cx, owner, |owner| &owner.0, index)?)?,
                expected
            );
        }
        Ok(())
    })
}

#[test]
fn published_array_cell_survives_its_unscanned_source_replacement() -> Result<(), GcError> {
    let mut heap = Heap::default();
    let original_target = heap.scope(|cx| {
        let mut values = ArrayValues::empty(&Ty::Builtin(BuiltinType::String));
        values.push(cx, ScopedValue::unit())?;
        let target = Buffer::alloc(cx, values)?;
        cx.retain(target)
    })?;
    let source = heap.scope(|cx| {
        let mut values = ArrayValues::empty(&Ty::Builtin(BuiltinType::String));
        for index in 0..96 {
            let text = StringObject::alloc(cx, format!("element {index}"))?;
            values.push(cx, ScopedValue::managed(text))?;
        }
        let source = Buffer::alloc(cx, values)?;
        cx.retain(source)
    })?;
    // Register the target last, after allocating it in the earliest slot. This
    // makes it black before the source's suspended scan (including shared pages).
    let target = heap.scope(|cx| {
        let target = cx.get(&original_target)?;
        cx.retain(target)
    })?;
    drop(original_target);
    let mut partial = false;
    for _ in 0..1000 {
        heap.step(WorkBudget::new(16))?;
        let state = heap.scope(|cx| {
            let source = cx.get(&source)?;
            Buffer::cursor(cx, source)
        })?;
        if state.is_some_and(|next| next > 2 && next < 90) {
            partial = true;
            break;
        }
    }
    assert!(partial, "expected a suspended production storage scan");
    heap.scope(|cx| {
        let target = cx.get(&target)?;
        let source = cx.get(&source)?;
        let tail = PackedStorage::get(cx, source, |source| &source.0, 95)?;
        let incoming = cx.read(target, |target| target.0.prepare(tail))?;
        // SAFETY: actual retained target, initialized fixed cell, protected input.
        unsafe {
            cx.with_object_mut(target, |target, mutation| {
                target.0.write(mutation, 0, incoming)?;
                mutation.continue_scan();
                Ok(())
            })?;
        }
        let mut cleared = ArrayValues::empty(&Ty::Builtin(BuiltinType::String));
        cleared.repeat(cx, ScopedValue::unit(), 96)?;
        // SAFETY: publish all replacement cells, admit actual owned capacity,
        // then commit one unchanged-length buffer with stable scan coordinates.
        unsafe {
            cx.with_object_mut(source, |source, mutation| {
                let replacement = cleared.publish(mutation)?;
                mutation.prepare_charge(ObjectCharge {
                    bytes: replacement.retained_bytes(),
                    units: replacement.len(),
                })?;
                source.0 = replacement;
                mutation.continue_scan();
                Ok(())
            })?;
        }
        Ok(())
    })?;
    for _ in 0..10000 {
        if heap.stats().phase == Phase::Idle {
            break;
        }
        heap.step(WorkBudget::new(16))?;
    }
    assert_eq!(heap.stats().phase, Phase::Idle);
    heap.collect()?;
    // Check counts before following a potentially lost edge. Only the two
    // buffers and the published tail may remain after floating garbage clears.
    assert_eq!(heap.stats().objects, 3);
    assert_eq!(
        heap.stats().provider_units,
        1 + 96 + "element 95".len() as u128
    );
    heap.scope(|cx| {
        let target = cx.get(&target)?;
        let value = PackedStorage::get(cx, target, |target| &target.0, 0)?;
        let text = cx.downcast::<StringObject>(value.object().unwrap())?;
        assert!(cx.read(text, |text| text.as_str() == "element 95"));
        Ok(())
    })?;
    drop((target, source));
    heap.collect()?;
    assert_eq!(heap.stats().objects, 0);
    assert_eq!(heap.stats().provider_units, 0);
    assert_eq!(heap.stats().backing_bytes, 0);
    Ok(())
}

#[test]
fn packed_capacities_charge_element_widths_and_ignore_zst_sentinels() -> Result<(), GcError> {
    for (kind, value, width) in [
        (BuiltinType::Unit, RootedValue::unit(), 0),
        (
            BuiltinType::Char,
            RootedValue::from('🙂'),
            size_of::<char>(),
        ),
        (BuiltinType::I8, RootedValue::from(1i32), size_of::<i8>()),
        (BuiltinType::I64, RootedValue::from(1i64), size_of::<i64>()),
    ] {
        let mut heap = Heap::default();
        let (root, expected) = heap.scope(|cx| {
            let mut values = ArrayValues::empty(&Ty::Builtin(kind));
            values.reserve(37)?;
            values.repeat(cx, value.get(cx)?, 3)?;
            // This oracle reads Rust Vec capacities before publication, without
            // calling either the production retained_bytes or charge hook.
            let capacity = match &values.buffer {
                ScopedBuffer::Unit(values) => values.capacity(),
                ScopedBuffer::Char(values) => values.capacity(),
                ScopedBuffer::I8(values) => values.capacity(),
                ScopedBuffer::I64(values) => values.capacity(),
                _ => unreachable!("selected physical storage"),
            };
            let expected = capacity.checked_mul(width).unwrap();
            let object = Buffer::alloc(cx, values)?;
            Ok((cx.retain(object)?, expected))
        })?;
        assert_eq!(heap.stats().objects, 1);
        assert_eq!(heap.stats().provider_units, 3);
        assert_eq!(heap.stats().indirect_bytes, expected);
        drop(root);
        heap.collect()?;
        assert_eq!(heap.stats().objects, 0);
        assert_eq!(heap.stats().indirect_bytes, 0);
    }
    let mut heap = Heap::default();
    let mut fields = Vec::with_capacity(37);
    fields.push(StoredValue::unit());
    let expected = fields.capacity() * size_of::<StoredValue>();
    let root = heap.scope(|cx| {
        // SAFETY: the only initialized field is inline Unit. The production
        // storage scanner owns all fields and the capacity is transferred whole.
        let object = unsafe { cx.alloc_with(|_| Ok(Buffer(PackedStorage::Traced(fields)))) }?;
        cx.retain(object)
    })?;
    assert_eq!(heap.stats().indirect_bytes, expected);
    assert_eq!(heap.stats().provider_units, 1);
    drop(root);
    heap.collect()?;
    assert_eq!(heap.stats().objects, 0);
    assert_eq!(heap.stats().indirect_bytes, 0);
    Ok(())
}
