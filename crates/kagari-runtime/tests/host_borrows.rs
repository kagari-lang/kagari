use kagari_runtime::{
    HostBorrowKind, HostBorrowTable, HostObjectId, Runtime, RuntimeErrorKind, TypeId, value::Value,
};

#[test]
fn shared_borrows_coexist_and_unique_conflicts_until_frame_exits() {
    let table = HostBorrowTable::default();
    let ty = TypeId::new(0);

    {
        let frame = table.enter_frame();
        let first = frame.borrow_shared(HostObjectId(1), ty).unwrap();
        let second = frame.borrow_shared(HostObjectId(1), ty).unwrap();

        assert_eq!(first.borrow_kind(), HostBorrowKind::Shared);
        assert_eq!(second.object_id(), HostObjectId(1));
        assert_eq!(
            frame.borrow_unique(HostObjectId(1), ty).unwrap_err().kind(),
            RuntimeErrorKind::HostBorrowConflict
        );
    }

    let next_frame = table.enter_frame();
    let unique = next_frame.borrow_unique(HostObjectId(1), ty).unwrap();
    next_frame.validate(unique, HostBorrowKind::Unique).unwrap();
}

#[test]
fn unique_borrow_blocks_shared_and_unique_aliases() {
    let table = HostBorrowTable::default();
    let ty = TypeId::new(0);
    let frame = table.enter_frame();

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
        let frame = table.enter_frame();
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
    let first = table.enter_frame();
    let token = first.borrow_shared(HostObjectId(1), ty).unwrap();
    let second = table.enter_frame();

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
    let runtime = Runtime::default();
    let frame = runtime.enter_host_call();
    let token = frame
        .borrow_shared(HostObjectId(1), TypeId::new(0))
        .unwrap();
    let borrow_value = Value::host_ref(token);

    assert!(borrow_value.contains_host_borrow());
    assert!(!borrow_value.is_storable());
    assert!(!borrow_value.is_default_heap_payload());
    assert_eq!(
        HostBorrowTable::validate_no_escape(&Value::Tuple(vec![borrow_value.clone()]))
            .unwrap_err()
            .kind(),
        RuntimeErrorKind::HostBorrowEscape
    );
    assert_eq!(
        kagari_runtime::HostCallGuard::validate_no_escape(&borrow_value)
            .unwrap_err()
            .kind(),
        RuntimeErrorKind::HostBorrowEscape
    );

    assert!(
        runtime
            .gc()
            .alloc_array(vec![borrow_value.clone()])
            .is_none()
    );
    assert!(runtime.root_value(borrow_value).is_none());
}
