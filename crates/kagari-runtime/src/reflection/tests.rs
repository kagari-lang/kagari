use crate::{
    Runtime,
    error::RuntimeErrorKind,
    host::HostObjectId,
    native::conversion::context::ConversionContext,
    reflection::type_name,
    test_support::{
        layouts,
        update_programs::{load_empty, source_code},
    },
    value::{
        EphemeralValue,
        aggregates::tuples::TupleObject,
        carriers::{RootedValue, ScopedValue},
        host_values::EphemeralObject,
        scalar,
        strings::StringObject,
    },
};
use kagari_types::{scalar::BuiltinType, ty::Ty};

#[test]
fn wide_indices_do_not_mutate_arrays_or_tuples() {
    let mut runtime = Runtime::default();
    let installed = load_empty(&mut runtime, "reflection");
    let owner = runtime.select_program(&installed).unwrap();
    let array = runtime
        .alloc_array(
            &owner,
            Ty::Builtin(BuiltinType::I32),
            &[RootedValue::from(7i32)],
        )
        .unwrap();
    let tuple = runtime
        .memory
        .scope(&runtime.resources, |cx, _| {
            let tuple = TupleObject::alloc(cx, &[ScopedValue::from(7i32)])?;
            ScopedValue::managed(tuple).retain(cx)
        })
        .unwrap();
    let before = runtime.gc_stats();
    for index in [
        RootedValue::from(1u64 << 32),
        RootedValue::from(u64::MAX),
        RootedValue::from(1i64 << 32),
    ] {
        for value in [&array, &tuple] {
            let error = runtime
                .reflect_set_index(value, &index, &RootedValue::from(9i32))
                .unwrap_err();
            assert_eq!(error.kind(), RuntimeErrorKind::InvalidReflectiveWrite);
        }
    }
    let after = runtime.gc_stats();
    assert_eq!(
        (
            after.objects,
            after.root_registrations,
            after.backing_bytes,
            after.indirect_bytes
        ),
        (
            before.objects,
            before.root_registrations,
            before.backing_bytes,
            before.indirect_bytes
        )
    );
    runtime
        .memory
        .scope(&runtime.resources, |cx, _| {
            let tuple = cx.downcast::<TupleObject>(tuple.get(cx)?.object().unwrap())?;
            assert_eq!(
                scalar::read_integer(BuiltinType::I32, TupleObject::get(cx, tuple, 0)?).unwrap(),
                7
            );
            Ok(())
        })
        .unwrap();
    // The mutable array setter returns the same Host lease; a tuple write returns
    // new immutable data. Both outcomes remain readable after the originals drop.
    let changed_array = runtime
        .reflect_set_index(&array, &RootedValue::from(0i32), &RootedValue::from(9i32))
        .unwrap();
    assert_eq!(
        runtime.gc_stats().root_registrations,
        before.root_registrations
    );
    let changed_tuple = runtime
        .reflect_set_index(&tuple, &RootedValue::from(0i32), &RootedValue::from(11i32))
        .unwrap();
    runtime
        .memory
        .scope(&runtime.resources, |cx, _| {
            let old = cx.downcast::<TupleObject>(tuple.get(cx)?.object().unwrap())?;
            let new = cx.downcast::<TupleObject>(changed_tuple.get(cx)?.object().unwrap())?;
            assert!(!old.ptr_eq(new));
            assert_eq!(
                scalar::read_integer(BuiltinType::I32, TupleObject::get(cx, old, 0)?).unwrap(),
                7
            );
            assert_eq!(
                scalar::read_integer(BuiltinType::I32, TupleObject::get(cx, new, 0)?).unwrap(),
                11
            );
            Ok(())
        })
        .unwrap();
    drop(array);
    drop(tuple);
    runtime.collect_garbage().unwrap();
    let mut conversion = ConversionContext::new(&mut runtime, &owner).unwrap();
    let array = conversion.import(&changed_array).unwrap();
    assert_eq!(
        conversion
            .array_element(&array, 0usize)
            .unwrap()
            .unwrap()
            .as_i32(),
        Some(9)
    );
}

#[test]
fn reports_production_value_category_names() {
    let mut runtime = Runtime::default();
    let (host, path) = layouts::host_values(&mut runtime, 2);
    let ty = runtime.types().get_by_name("fixtures::Player").unwrap().id;
    let program = runtime
        .load_verified_program(
            "reflection-interface",
            source_code(include_str!(
                "../../tests/fixtures/metadata/interface-witnesses.kgr"
            )),
        )
        .unwrap();
    let owner = runtime.select_program(&program).unwrap();
    let interface = runtime
        .make_interface(&owner, 0, &RootedValue::from(7i32))
        .unwrap();
    for (value, expected) in [
        (&interface, "interface"),
        (&host, "host_root"),
        (&path, "host_path_view"),
    ] {
        let name = runtime.reflect_type_of(value).unwrap();
        let actual = runtime
            .memory
            .scope(&runtime.resources, |cx, _| {
                let value = name.get(cx)?;
                StringObject::read(cx, value, str::to_owned)
            })
            .unwrap();
        assert_eq!(actual, expected);
    }
    let Runtime {
        resources,
        memory,
        host_borrows,
        ..
    } = &mut runtime;
    let loan = host_borrows.enter_frame().unwrap();
    let token = loan.borrow_shared(HostObjectId(2), ty).unwrap();
    memory
        .scope(resources, |cx, _| {
            let value = EphemeralObject::alloc(cx, EphemeralValue::HostRef(token))?;
            assert_eq!(type_name(cx, value)?, "ephemeral");
            Ok(())
        })
        .unwrap();
}
