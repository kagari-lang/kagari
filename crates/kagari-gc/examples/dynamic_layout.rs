//! Safe runtime-defined records: no custom Rust payload, scanner or unsafe access.

use kagari_gc::{
    access::Access,
    dynamic::{DynamicLayout, DynamicLayoutBuilder, ReferenceField, ScalarField},
    error::GcError,
    heap::Heap,
    object::AnyObject,
};

struct PlayerLayout {
    layout: DynamicLayout,
    hp: ScalarField<i64>,
    target: ReferenceField,
}

impl PlayerLayout {
    fn register(access: &mut Access<'_>) -> Result<Self, GcError> {
        let mut builder = DynamicLayoutBuilder::new();
        let hp = builder.scalar::<i64>();
        let target = builder.reference();
        let layout = access.register_dynamic(builder)?;
        Ok(Self {
            hp: layout.scalar(hp)?,
            target: layout.reference(target)?,
            layout,
        })
    }
}

fn main() -> Result<(), GcError> {
    let mut heap = Heap::default();
    let fields = PlayerLayout::register(&mut heap.access()?)?;
    let frame = heap.access()?.create_roots(1)?;
    heap.scope(|cx| {
        let player = cx.alloc_dynamic(&fields.layout)?;
        let enemy = cx.alloc_dynamic(&fields.layout)?;
        fields.hp.set(cx, player, 100)?;
        fields.hp.set(cx, enemy, 25)?;
        fields.target.set(cx, player, Some(enemy))?;
        fields.target.set(cx, enemy, Some(player))?;
        cx.set_root(&frame, 0, Some(player))
    })?;
    heap.collect()?;
    assert_eq!(heap.stats().root_registrations, 1);
    heap.scope(|cx| {
        let player = cx.get_root::<AnyObject>(&frame, 0)?;
        let enemy = fields.target.get(cx, player)?.expect("target");
        *fields.hp.borrow_mut(cx, enemy)? -= 5;
        assert_eq!(fields.hp.get(cx, enemy)?, 20);
        Ok(())
    })?;
    drop(frame);
    heap.collect()?;
    assert_eq!(heap.stats().objects, 0);
    assert_eq!(heap.stats().backing_bytes, 0);
    println!("Dynamic layout, scoped fields, frame roots and cyclic reclamation passed.");
    Ok(())
}
