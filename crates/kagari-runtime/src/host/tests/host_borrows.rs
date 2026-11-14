use crate::{
    Runtime,
    error::RuntimeErrorKind,
    host::{HostBorrowKind, HostBorrowTable, HostObjectId},
    metadata::TypeId,
    test_support::update_programs::load_empty,
    value::{
        EphemeralValue, aggregates::tuples::TupleObject, carriers::ScopedValue,
        host_values::EphemeralObject, properties::ValueProperties,
    },
};
use kagari_types::{scalar::BuiltinType, ty::Ty};
use std::slice;

#[test]
fn shared_borrows_coexist_and_unique_conflicts_until_frame_exits() {
    let table = HostBorrowTable::default();
    let ty = TypeId::new(0);

    {
        let frame = table.enter_frame().unwrap();
        let first = frame.borrow_shared(HostObjectId(1), ty).unwrap();
        let second = frame.borrow_shared(HostObjectId(1), ty).unwrap();

        assert_eq!(first.borrow_kind(), HostBorrowKind::Shared);
        assert_eq!(second.object_id(), HostObjectId(1));
        assert_eq!(
            frame.borrow_unique(HostObjectId(1), ty).unwrap_err().kind(),
            RuntimeErrorKind::HostBorrowConflict
        );
    }

    let next_frame = table.enter_frame().unwrap();
    let unique = next_frame.borrow_unique(HostObjectId(1), ty).unwrap();
    next_frame.validate(unique, HostBorrowKind::Unique).unwrap();
}

#[test]
fn unique_borrow_blocks_shared_and_unique_aliases() {
    let table = HostBorrowTable::default();
    let ty = TypeId::new(0);
    let frame = table.enter_frame().unwrap();

    frame.borrow_unique(HostObjectId(1), ty).unwrap();

    assert_eq!(
        frame.borrow_shared(HostObjectId(1), ty).unwrap_err().kind(),
        RuntimeErrorKind::HostBorrowConflict
    );
    assert_eq!(
        frame.borrow_unique(HostObjectId(1), ty).unwrap_err().kind(),
        RuntimeErrorKind::HostBorrowConflict
    );
    assert!(frame.borrow_shared(HostObjectId(2), ty).is_ok());
}

#[test]
fn tokens_validate_current_frame_kind_and_expire_on_drop() {
    let table = HostBorrowTable::default();
    let ty = TypeId::new(0);
    let shared;
    let unique;

    {
        let frame = table.enter_frame().unwrap();
        shared = frame.borrow_shared(HostObjectId(1), ty).unwrap();
        unique = frame.borrow_unique(HostObjectId(2), ty).unwrap();

        assert!(frame.validate(shared, HostBorrowKind::Shared).is_ok());
        assert!(frame.validate(unique, HostBorrowKind::Shared).is_ok());
        assert!(frame.validate(unique, HostBorrowKind::Unique).is_ok());
        assert_eq!(
            frame
                .validate(shared, HostBorrowKind::Unique)
                .unwrap_err()
                .kind(),
            RuntimeErrorKind::HostBorrowConflict
        );
    }

    assert_eq!(
        table
            .validate(shared, HostBorrowKind::Shared)
            .unwrap_err()
            .kind(),
        RuntimeErrorKind::ExpiredHostBorrow
    );
}

#[test]
fn guard_rejects_borrow_tokens_from_another_live_frame() {
    let table = HostBorrowTable::default();
    let ty = TypeId::new(0);
    let first = table.enter_frame().unwrap();
    let token = first.borrow_shared(HostObjectId(1), ty).unwrap();
    let second = table.enter_frame().unwrap();

    assert_eq!(
        second
            .validate(token, HostBorrowKind::Shared)
            .unwrap_err()
            .kind(),
        RuntimeErrorKind::ExpiredHostBorrow
    );
}

#[test]
fn borrow_values_are_non_storable_and_fail_no_escape_validation() {
    let mut runtime = Runtime::default();
    let program = load_empty(&mut runtime, "borrow-storage");
    let module = runtime.select_program(&program).unwrap();
    runtime.collect_garbage().unwrap();
    let before = runtime.gc_stats();
    let mut resources = runtime.host_scope(&[]).unwrap();
    let token = resources
        .borrows()
        .borrow_shared(HostObjectId(1), TypeId::new(0))
        .unwrap();
    let (direct, nested) = {
        let Runtime {
            memory, resources, ..
        } = resources.runtime_mut();
        memory.scope(resources, |cx, _| {
            let value = EphemeralObject::alloc(cx, EphemeralValue::HostRef(token))?;
            let facts = ValueProperties::of(cx, value)?;
            assert!(facts.host_borrow);
            assert!(facts.ephemeral);
            assert!(!facts.storable);
            let tuple = TupleObject::alloc(cx, &[value])?;
            let facts = cx.read(tuple, TupleObject::properties);
            assert!(facts.host_borrow);
            assert!(facts.ephemeral);
            assert!(!facts.storable);
            Ok((value.retain(cx)?, ScopedValue::managed(tuple).retain(cx)?))
        })
    }
    .unwrap();
    let roots = resources.runtime().gc_stats().root_registrations;
    for root in [&direct, &nested] {
        let value = resources.import(root).unwrap();
        assert_eq!(
            resources.validate_result(&value).unwrap_err().kind(),
            RuntimeErrorKind::HostBorrowEscape
        );
        assert_eq!(
            resources.retain(&value).unwrap_err().kind(),
            RuntimeErrorKind::HostBorrowEscape
        );
    }
    assert_eq!(resources.runtime().gc_stats().root_registrations, roots);
    assert_eq!(
        resources
            .runtime_mut()
            .alloc_array(
                &module,
                Ty::Builtin(BuiltinType::I32),
                slice::from_ref(&direct)
            )
            .unwrap_err()
            .kind(),
        RuntimeErrorKind::ScriptTrap
    );
    drop(resources);
    assert_eq!(
        runtime
            .host_scope(slice::from_ref(&direct))
            .err()
            .expect("expired capability")
            .kind(),
        RuntimeErrorKind::ExpiredHostBorrow
    );
    drop((direct, nested));
    runtime.collect_garbage().unwrap();
    assert_eq!(runtime.gc_stats().objects, before.objects);
    assert_eq!(
        runtime.gc_stats().root_registrations,
        before.root_registrations
    );
    assert!(!runtime.is_quarantined());
}
