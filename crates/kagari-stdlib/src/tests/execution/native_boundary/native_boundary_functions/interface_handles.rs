use super::{fixture, fixture_without_foundation};
use crate::{
    declarations::StandardDeclarations,
    tests::{
        execution::native_boundary::compile_without_foundation, support::bytecode::reload_bytecode,
    },
};
use kagari_runtime::native::{
    binding::NativeResult, builder::ModuleBuilder, catalog::DeclarationCatalog,
    conversion::KagariType, declarations::FunctionDecl, interfaces::Interface,
    typed::NativeContext, types::Type,
};

use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

const SOURCE: &str =
    include_str!("../../../../../tests/fixtures/native/interface-retained-handles.kgr");

#[test]
fn retained_interface_round_trips_preserve_application_and_aliases() {
    let (mut vm, owner) = fixture(SOURCE, None);
    let _owner_program = owner;
    let _owner = vm.select_program(&_owner_program).unwrap();
    vm.collect_garbage().unwrap();
    let baseline = vm.gc_stats();
    let make = vm
        .bind_function::<(), Interface>(&_owner_program.root(), "make")
        .unwrap();
    let read = vm
        .bind_function::<(Interface,), i32>(&_owner_program.root(), "read")
        .unwrap();
    let wrong = vm
        .bind_function::<(Interface,), String>(&_owner_program.root(), "read_string")
        .unwrap();
    let value = vm.call(&make, ()).unwrap();
    vm.collect_garbage().unwrap();
    assert_eq!(vm.call(&read, (value.clone(),)).unwrap(), 42);
    assert!(vm.call(&wrong, (value.clone(),)).is_err());
    let wrapped: Option<Interface> = vm
        .execute_typed(
            &_owner_program,
            "wrap",
            (value.clone(),),
            &Default::default(),
        )
        .unwrap();
    drop(value);
    assert_eq!(vm.call(&read, (wrapped.unwrap(),)).unwrap(), 42);
    let (mut foreign, foreign_owner) = fixture(SOURCE, None);
    let _foreign_owner_program = foreign_owner;
    let foreign_read = foreign
        .bind_function::<(Interface,), i32>(&_foreign_owner_program.root(), "read")
        .unwrap();
    let value = vm.call(&make, ()).unwrap();
    assert!(foreign.call(&foreign_read, (value.clone(),)).is_err());
    drop(value);
    vm.collect_garbage().unwrap();
    assert_eq!(vm.gc_stats().objects, baseline.objects);
    assert_eq!(
        vm.gc_stats().root_registrations,
        baseline.root_registrations
    );
}

struct IntList;

impl KagariType for IntList {
    fn kagari_type(catalog: &DeclarationCatalog) -> NativeResult<Type> {
        Ok(StandardDeclarations::default()
            .list()
            .apply([i32::kagari_type(catalog)?])
            .unwrap()
            .ty())
    }
}

#[test]
fn typed_native_interfaces_keep_readonly_views_and_survive_collection() {
    let calls = Arc::new(AtomicUsize::new(0));
    let observed = calls.clone();
    let mut builder = ModuleBuilder::new(
        "example::interface_handles",
        &StandardDeclarations::default().catalog().unwrap(),
    )
    .unwrap();
    {
        let registered_function = builder
            .define_function(
                FunctionDecl::new("retain")
                    .parameter(
                        "value",
                        <Interface<IntList>>::kagari_type(builder.declarations()).unwrap(),
                    )
                    .returns(<Interface<IntList>>::kagari_type(builder.declarations()).unwrap()),
            )
            .unwrap();
        builder
            .bind(
                registered_function.clone(),
                move |cx: &mut NativeContext<'_>,
                      (value,): (Interface<IntList>,)|
                      -> NativeResult<Interface<IntList>> {
                    observed.fetch_add(1, Ordering::Relaxed);
                    cx.collect_garbage()?;
                    Ok(value)
                },
            )
            .map(|()| registered_function)
    }
    .unwrap();
    let module = builder.finish().unwrap();
    let (mut vm, owner) = fixture(
        include_str!("../../../../../tests/fixtures/native/interface-readonly-handles.kgr"),
        Some(&module),
    );
    let _owner_program = owner;
    let _owner = vm.select_program(&_owner_program).unwrap();
    vm.collect_garbage().unwrap();
    let baseline = vm.gc_stats();
    let list: Interface<IntList> = vm
        .execute_typed(&_owner_program, "make", (), &Default::default())
        .unwrap();
    assert_eq!(calls.load(Ordering::Relaxed), 1);
    vm.collect_garbage().unwrap();
    assert!(
        vm.execute_typed::<_, ()>(
            &_owner_program,
            "write",
            (list.clone(),),
            &Default::default()
        )
        .is_err()
    );
    assert_eq!(
        vm.execute_typed::<_, i32>(&_owner_program, "read", (list,), &Default::default())
            .unwrap(),
        42
    );
    let dynamic: Interface = vm
        .execute_typed(&_owner_program, "make", (), &Default::default())
        .unwrap();
    assert!(
        vm.execute_typed::<_, ()>(
            &_owner_program,
            "write",
            (dynamic.clone(),),
            &Default::default()
        )
        .is_err()
    );
    assert_eq!(
        vm.execute_typed::<_, i32>(&_owner_program, "read", (dynamic,), &Default::default())
            .unwrap(),
        42
    );
    vm.collect_garbage().unwrap();
    assert_eq!(vm.gc_stats().objects, baseline.objects);
    assert_eq!(
        vm.gc_stats().root_registrations,
        baseline.root_registrations
    );
}

#[test]
fn retained_interfaces_select_current_dispatch_after_reload_and_thread_transfer() {
    let source = include_str!(
        "../../../../../tests/fixtures/native/retained-interface-current-dispatch.kgr"
    );
    let (mut vm, old) = fixture_without_foundation(source, None);
    let _old_program = old;
    let old = vm.select_program(&_old_program).unwrap();
    vm.collect_garbage().unwrap();
    let baseline = vm.gc_stats();
    let value: Interface = vm
        .execute_typed(&_old_program, "make", (), &Default::default())
        .unwrap();
    let alias = value.clone();
    // Change both code and constructor data while preserving the public contract.
    for pattern in ["Holder { value: 41 }", "self.value + 1"] {
        assert_eq!(source.matches(pattern).count(), 1, "{pattern}");
    }
    let source = source
        .replace("Holder { value: 41 }", "Holder { value: 7 }")
        .replace("self.value + 1", "self.value + 2");
    reload_bytecode(
        &mut vm,
        &_old_program,
        Some("functions".into()),
        compile_without_foundation(&source, None),
    )
    .unwrap();
    let _new_program = _old_program.clone();
    let _new = vm.select_program(&_new_program).unwrap();
    std::thread::spawn(move || {
        vm.collect_garbage().unwrap();
        assert_eq!(
            vm.execute_typed::<_, i32>(&_new_program, "read", (value,), &Default::default())
                .unwrap(),
            43
        );
        let new_value: Interface = vm
            .execute_typed(&_new_program, "make", (), &Default::default())
            .unwrap();
        assert_eq!(
            vm.execute_typed::<_, i32>(&_new_program, "read", (new_value,), &Default::default())
                .unwrap(),
            9
        );
        vm.collect_garbage().unwrap();
        assert!(vm.validate_executable_module(&old).is_ok());
        drop(alias);
        vm.collect_garbage().unwrap();
        assert!(vm.validate_executable_module(&old).is_err());
        assert_eq!(vm.gc_stats().objects, baseline.objects);
        assert_eq!(
            vm.gc_stats().root_registrations,
            baseline.root_registrations
        );
    })
    .join()
    .unwrap();
}
