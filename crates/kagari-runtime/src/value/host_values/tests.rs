use crate::{
    host::{HostObjectId, HostPathDescriptorId, HostRootHandle, HostSchemaEpoch},
    metadata::{AbiFingerprint, TypeId},
    value::{
        aggregates::tuples::TupleObject,
        carriers::ScopedValue,
        host_values::{
            HostRootObject,
            paths::{HostPathObject, PathFacts},
        },
        properties::ValueProperties,
        strings::StringObject,
    },
};
use kagari_gc::{error::GcError, heap::Heap};
use kagari_types::declaration::host::PathAccess;

#[test]
fn nested_tuple_fields_keep_gc_edges_and_host_boundary_facts() -> Result<(), GcError> {
    let mut heap = Heap::default();
    let root = heap.scope(|cx| {
        let text = StringObject::alloc(cx, "owned UTF-8 🙂".to_owned())?;
        let ordinary = TupleObject::alloc(
            cx,
            &[ScopedValue::managed(text), ScopedValue::from(u64::MAX)],
        )?;
        let host = HostRootObject::alloc(
            cx,
            HostRootHandle::new(
                Default::default(),
                HostObjectId(7),
                TypeId::new(0),
                HostSchemaEpoch::new(0),
                AbiFingerprint(1),
            ),
        )?;
        let handle = cx.read(host, HostRootObject::handle);
        let host = TupleObject::alloc(cx, &[ScopedValue::managed(host)])?;
        let path_text = StringObject::alloc(cx, "dynamic path source".to_owned())?;
        // These physical labels are inert. Registry/declared-access admission is
        // checked by the Host owner; here only actual graph edges establish life.
        let facts = PathFacts {
            root: handle,
            descriptor: HostPathDescriptorId::new(0),
            result: TypeId::new(2),
            access: PathAccess::ReadWrite,
            epoch: handle.schema_epoch(),
        };
        let base = HostPathObject::alloc(
            cx,
            facts,
            None,
            &[(TypeId::new(3), ScopedValue::managed(path_text))],
        )?;
        let path = HostPathObject::alloc(
            cx,
            PathFacts {
                descriptor: HostPathDescriptorId::new(1),
                access: PathAccess::ReadOnly,
                ..facts
            },
            Some(base),
            &[(TypeId::new(4), ScopedValue::from(u64::MAX))],
        )?;
        let path = TupleObject::alloc(cx, &[ScopedValue::managed(path)])?;
        assert!(cx.read(ordinary, TupleObject::properties).storable);
        assert!(!cx.read(host, TupleObject::properties).storable);
        assert!(!cx.read(path, TupleObject::properties).storable);
        let outer = TupleObject::alloc(
            cx,
            &[
                ScopedValue::managed(ordinary),
                ScopedValue::managed(host),
                ScopedValue::managed(path),
            ],
        )?;
        let facts = ValueProperties::of(cx, ScopedValue::managed(outer))?;
        assert!(!facts.storable);
        cx.retain(outer)
    })?;
    let shared = root.clone();
    assert_eq!(heap.stats().root_registrations, 1);
    heap.collect()?;
    // Three leaves, three inner tuples, two path nodes and the outer tuple. Check
    // retained count independently before following fields, so missing base or
    // dynamic-field edges cannot be hidden by a read admission failure.
    assert_eq!(heap.stats().objects, 9);
    heap.scope(|cx| {
        let outer = cx.get(&root)?;
        let ordinary = TupleObject::get(cx, outer, 0)?.object().unwrap();
        let ordinary = cx.downcast::<TupleObject>(ordinary)?;
        let text = TupleObject::get(cx, ordinary, 0)?.object().unwrap();
        let text = cx.downcast::<StringObject>(text)?;
        assert!(cx.read(text, |text| text.as_str() == "owned UTF-8 🙂"));
        let number = TupleObject::get(cx, ordinary, 1)?;
        assert_eq!(number.scalar_payload().unwrap().1, u64::MAX);
        assert!(matches!(
            TupleObject::get(cx, ordinary, 2),
            Err(GcError::Index)
        ));
        let tuple = TupleObject::get(cx, outer, 2)?.object().unwrap();
        let tuple = cx.downcast::<TupleObject>(tuple)?;
        let path = TupleObject::get(cx, tuple, 0)?.object().unwrap();
        let path = cx.downcast::<HostPathObject>(path)?;
        assert_eq!(
            cx.read(path, |path| path.facts().access),
            PathAccess::ReadOnly
        );
        assert_eq!(
            cx.read(path, |path| path.argument_type(0)),
            Some(TypeId::new(4))
        );
        assert_eq!(
            HostPathObject::argument(cx, path, 0)?
                .scalar_payload()
                .unwrap()
                .1,
            u64::MAX
        );
        let base = HostPathObject::base(cx, path).unwrap();
        assert_eq!(
            cx.read(base, |base| base.argument_type(0)),
            Some(TypeId::new(3))
        );
        let text = HostPathObject::argument(cx, base, 0)?.object().unwrap();
        let text = cx.downcast::<StringObject>(text)?;
        assert!(cx.read(text, |text| text.as_str() == "dynamic path source"));
        Ok(())
    })?;
    drop(root);
    heap.collect()?;
    assert_eq!(heap.stats().objects, 9);
    drop(shared);
    assert_eq!(heap.collect()?.retired_owners, 10);
    assert_eq!(heap.stats().objects, 0);
    assert_eq!(heap.stats().provider_units, 0);
    assert_eq!(heap.stats().root_registrations, 0);
    Ok(())
}
