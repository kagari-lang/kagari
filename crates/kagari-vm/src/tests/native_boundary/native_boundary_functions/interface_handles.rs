use super::{fixture, fixture_without_foundation};
use crate::tests::native_boundary::compile_without_foundation;
use kagari_runtime::native::{
    binding::NativeResult, builder::ModuleBuilder, catalog::DeclarationCatalog,
    conversion::KagariType, declarations::FunctionDecl, interfaces::Interface,
    typed::NativeContext, types::Type,
};
use kagari_stdlib::declarations::StandardDeclarations;
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

const SOURCE: &str = r#"
    pub trait Read<T> { fn get(self) -> T; }
    pub struct Holder<T> { pub var value: T }
    impl<T> Read<T> for Holder<T> { fn get(self) -> T { self.value } }
    pub fn make() -> Read<i32> { Holder { value: 42 } }
    pub fn read(value: Read<i32>) -> i32 { value.get() }
    pub fn read_string(value: Read<String>) -> String { value.get() }
    pub fn wrap(value: Read<i32>) -> Option<Read<i32>> { Some(value) }
"#;

#[test]
fn retained_interface_round_trips_preserve_application_and_aliases() {
    let (vm, owner) = fixture(SOURCE, None);
    let _owner_program = owner;
    let owner = vm.runtime().select_program(&_owner_program).unwrap();
    let make = vm
        .runtime()
        .bind_function::<(), Interface>(&_owner_program.root(), "make")
        .unwrap();
    let read = vm
        .runtime()
        .bind_function::<(Interface,), i32>(&_owner_program.root(), "read")
        .unwrap();
    let wrong = vm
        .runtime()
        .bind_function::<(Interface,), String>(&_owner_program.root(), "read_string")
        .unwrap();
    let value = vm.call(&make, ()).unwrap();
    vm.runtime().collect_garbage().unwrap();
    assert_eq!(vm.call(&read, (value.clone(),)).unwrap(), 42);
    assert!(vm.call(&wrong, (value.clone(),)).is_err());
    let wrapped: Option<Interface> = vm.execute_typed(&owner, "wrap", (value.clone(),)).unwrap();
    drop(value);
    assert_eq!(vm.call(&read, (wrapped.unwrap(),)).unwrap(), 42);
    let (foreign, foreign_owner) = fixture(SOURCE, None);
    let _foreign_owner_program = foreign_owner;
    let foreign_read = foreign
        .runtime()
        .bind_function::<(Interface,), i32>(&_foreign_owner_program.root(), "read")
        .unwrap();
    let value = vm.call(&make, ()).unwrap();
    assert!(foreign.call(&foreign_read, (value.clone(),)).is_err());
    drop(value);
    assert_eq!(vm.runtime().collect_garbage().unwrap().live_objects, 0);
    assert_eq!(vm.runtime().gc().active_roots(), 0);
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
    let (vm, owner) = fixture(
        r#"
        use example::interface_handles::retain;
        use std::collections::{List, MutableList};
        pub fn make() -> List<i32> { val value: List<i32> = Vec::from([42]); retain(value) }
        pub fn read(value: List<i32>) -> i32 { value[0] }
        pub fn write(value: MutableList<i32>) { value[0] = 0; }
    "#,
        Some(&module),
    );
    let _owner_program = owner;
    let owner = vm.runtime().select_program(&_owner_program).unwrap();
    let list: Interface<IntList> = vm.execute_typed(&owner, "make", ()).unwrap();
    assert_eq!(calls.load(Ordering::Relaxed), 1);
    vm.runtime().collect_garbage().unwrap();
    assert!(
        vm.execute_typed::<_, ()>(&owner, "write", (list.clone(),))
            .is_err()
    );
    assert_eq!(
        vm.execute_typed::<_, i32>(&owner, "read", (list,)).unwrap(),
        42
    );
    let dynamic: Interface = vm.execute_typed(&owner, "make", ()).unwrap();
    assert!(
        vm.execute_typed::<_, ()>(&owner, "write", (dynamic.clone(),))
            .is_err()
    );
    assert_eq!(
        vm.execute_typed::<_, i32>(&owner, "read", (dynamic,))
            .unwrap(),
        42
    );
    assert_eq!(vm.runtime().collect_garbage().unwrap().live_objects, 0);
}

#[test]
fn retained_interfaces_select_current_dispatch_after_reload_and_thread_transfer() {
    let source =
        include_str!("../../../../tests/fixtures/native/retained-interface-current-dispatch.kgr");
    let (vm, old) = fixture_without_foundation(source, None);
    let _old_program = old;
    let old = vm.runtime().select_program(&_old_program).unwrap();
    let value: Interface = vm.execute_typed(&old, "make", ()).unwrap();
    let alias = value.clone();
    // Change both code and constructor data while preserving the public contract.
    for pattern in ["Holder { value: 41 }", "self.value + 1"] {
        assert_eq!(source.matches(pattern).count(), 1, "{pattern}");
    }
    let source = source
        .replace("Holder { value: 41 }", "Holder { value: 7 }")
        .replace("self.value + 1", "self.value + 2");
    vm.reload_program(
        &_old_program,
        Some("functions".into()),
        compile_without_foundation(&source, None),
    )
    .unwrap();
    let _new_program = _old_program.clone();
    let new = vm.runtime().select_program(&_new_program).unwrap();
    std::thread::spawn(move || {
        vm.runtime().collect_garbage().unwrap();
        assert_eq!(
            vm.execute_typed::<_, i32>(&new, "read", (value,)).unwrap(),
            43
        );
        let new_value: Interface = vm.execute_typed(&new, "make", ()).unwrap();
        assert_eq!(
            vm.execute_typed::<_, i32>(&new, "read", (new_value,))
                .unwrap(),
            9
        );
        assert!(
            !vm.runtime()
                .collect_garbage()
                .unwrap()
                .reclaimed_modules
                .contains(&old.key())
        );
        drop(alias);
        assert!(
            vm.runtime()
                .collect_garbage()
                .unwrap()
                .reclaimed_modules
                .contains(&old.key())
        );
        assert_eq!(vm.runtime().gc().active_roots(), 0);
    })
    .join()
    .unwrap();
}
