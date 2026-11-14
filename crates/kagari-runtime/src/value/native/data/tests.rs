use crate::{native::payload::data::NativeData, value::native::data::DataObject};
use kagari_gc::{
    error::GcError,
    heap::{Heap, Phase},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct State {
    hp: i32,
    flags: u64,
}

// SAFETY: both fields are fixed integers, with no edge, pointer or backing.
unsafe impl NativeData for State {}

#[test]
fn completed_data_edits_survive_errors_and_user_panics_without_faulting_gc() -> Result<(), GcError>
{
    let mut heap = Heap::default();
    let root = heap.scope(|cx| {
        let object = DataObject::alloc(cx, State { hp: 7, flags: 0 })?;
        cx.retain(object)
    })?;
    let units = heap.stats().provider_units;
    let bytes = heap.stats().indirect_bytes;
    heap.scope(|cx| {
        let object = cx.get(&root)?;
        let result = DataObject::edit(cx, object, |value| {
            value.hp = -3;
            Err::<(), _>("user error")
        })?;
        assert_eq!(result.expect("nonpanicking user edit"), Err("user error"));
        Ok(())
    })?;
    let panic = heap.scope(|cx| {
        let object = cx.get(&root)?;
        DataObject::edit(cx, object, |value| {
            value.flags = 36;
            panic!("user panic after write")
        })
    })?;
    assert!(panic.is_err());
    drop(panic);
    assert_eq!(heap.stats().phase, Phase::Idle);
    assert_eq!(heap.stats().provider_units, units);
    assert_eq!(heap.stats().indirect_bytes, bytes);
    heap.collect()?;
    heap.scope(|cx| {
        let object = cx.get(&root)?;
        assert_eq!(
            cx.read(object, |object| *object.value()),
            State { hp: -3, flags: 36 }
        );
        Ok(())
    })?;
    drop(root);
    heap.collect()?;
    assert_eq!(heap.stats().objects, 0);
    assert_eq!(heap.stats().provider_units, 0);
    assert_eq!(heap.stats().backing_bytes, 0);
    Ok(())
}
