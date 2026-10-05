use super::fixture;
use crate::compile_program;
use kagari_runtime::native::{
    binding::NativeResult, builder::ModuleBuilder, catalog::DeclarationCatalog,
    conversion::KagariType, interfaces::Interface, registration::FunctionSpec,
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
    let make = vm
        .runtime()
        .bind_function::<(), Interface>(&owner, "make")
        .unwrap();
    let read = vm
        .runtime()
        .bind_function::<(Interface,), i32>(&owner, "read")
        .unwrap();
    let wrong = vm
        .runtime()
        .bind_function::<(Interface,), String>(&owner, "read_string")
        .unwrap();
    let value = vm.call(&make, ()).unwrap();
    vm.runtime().collect_garbage().unwrap();
    assert_eq!(vm.call(&read, (value.clone(),)).unwrap(), 42);
    assert!(vm.call(&wrong, (value.clone(),)).is_err());
    let wrapped: Option<Interface> = vm.execute_typed(&owner, "wrap", (value.clone(),)).unwrap();
    drop(value);
    assert_eq!(vm.call(&read, (wrapped.unwrap(),)).unwrap(), 42);
    let (foreign, foreign_owner) = fixture(SOURCE, None);
    let foreign_read = foreign
        .runtime()
        .bind_function::<(Interface,), i32>(&foreign_owner, "read")
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
    );
    builder
        .add_function(
            FunctionSpec::new("retain").parameter_names(["value"]),
            move |cx: &mut NativeContext<'_>,
                  (value,): (Interface<IntList>,)|
                  -> NativeResult<Interface<IntList>> {
                observed.fetch_add(1, Ordering::Relaxed);
                cx.collect_garbage()?;
                Ok(value)
            },
        )
        .unwrap();
    let module = builder.finish().unwrap();
    let (vm, owner) = fixture(
        r#"
        use example::interface_handles::retain;
        use std::collections::{List, MutableList};
        pub fn make() -> List<i32> { val value: List<i32> = [42]; retain(value) }
        pub fn read(value: List<i32>) -> i32 { value[0] }
        pub fn write(value: MutableList<i32>) { value[0] = 0; }
    "#,
        Some(&module),
    );
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
fn retained_interfaces_keep_the_original_dispatch_after_reload_and_thread_transfer() {
    let source = SOURCE
        .replace(
            "impl<T> Read<T> for Holder<T> { fn get(self) -> T { self.value } }",
            "impl Read<i32> for Holder<i32> { fn get(self) -> i32 { self.value + 1 } }",
        )
        .replace("Holder { value: 42 }", "Holder { value: 41 }");
    let (vm, old) = fixture(&source, None);
    let value: Interface = vm.execute_typed(&old, "make", ()).unwrap();
    let alias = value.clone();
    // Change both code and constructor data while preserving the public contract.
    let source = source
        .replace("Holder { value: 41 }", "Holder { value: 7 }")
        .replace("self.value + 1", "self.value + 2");
    let new = vm
        .reload_program(&old, "functions", compile_program(&source, None))
        .unwrap();
    std::thread::spawn(move || {
        vm.runtime().collect_garbage().unwrap();
        assert_eq!(
            vm.execute_typed::<_, i32>(&new, "read", (value,)).unwrap(),
            42
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
