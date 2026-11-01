//! Safe runtime-defined records: no custom Rust payload, scanner or unsafe access.

use kagari_gc::{dynamic::FieldType, error::GcError, heap::Heap, object::AnyObject};

fn main() -> Result<(), GcError> {
    let mut heap = Heap::default();
    let player_layout = heap
        .access()?
        .register_dynamic(&[FieldType::I64, FieldType::Reference])?;
    let hp = player_layout.scalar::<i64>(0)?;
    let target = player_layout.reference(1)?;
    let frame = heap.access()?.create_roots(1)?;
    heap.scope(|cx| {
        let player = cx.alloc_dynamic(&player_layout)?;
        let enemy = cx.alloc_dynamic(&player_layout)?;
        hp.set(cx, player, 100)?;
        hp.set(cx, enemy, 25)?;
        target.set(cx, player, Some(enemy))?;
        target.set(cx, enemy, Some(player))?;
        cx.set_root(&frame, 0, Some(player))
    })?;
    heap.collect()?;
    assert_eq!(heap.stats().root_registrations, 1);
    heap.scope(|cx| {
        let player = cx.get_root::<AnyObject>(&frame, 0)?;
        let enemy = target.get(cx, player)?.expect("target");
        *hp.borrow_mut(cx, enemy)? -= 5;
        assert_eq!(hp.get(cx, enemy)?, 20);
        Ok(())
    })?;
    drop(frame);
    heap.collect()?;
    assert_eq!(heap.stats().objects, 0);
    assert_eq!(heap.stats().backing_bytes, 0);
    println!("Dynamic layout, scoped fields, frame roots and cyclic reclamation passed.");
    Ok(())
}
