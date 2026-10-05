use super::fixture;
use crate::compile_program;
use kagari_runtime::native::{
    binding::NativeResult, builder::ModuleBuilder, conversion::context::ConversionContext,
    function_handle::PinnedFunction, interfaces::Interface, objects::Object,
    registration::FunctionSpec, typed::NativeContext,
};
use kagari_stdlib::declarations::StandardDeclarations;
use std::{
    slice,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
};

const IDENTITY: &str = r#"
    pub trait Identity {
        fn identity<T>(self, value: T) -> T { value }
        fn delayed<T>(self, value: T) -> fn() -> T { || value }
    }
    pub trait Child: Identity {}
    struct Empty {}
    pub struct Item { pub var value: i32 }
    impl Identity for Empty {}
    impl Child for Empty {}
    pub fn make() -> Child { Empty {} }
    pub fn item() -> Item { Item { value: 42 } }
    pub fn scalar_evidence(target: Child) -> i32 { target.identity(1) }
    pub fn object_evidence(target: Child, value: Item) -> Item { target.identity(value) }
    pub fn closure_evidence(target: Child, value: Item) -> fn() -> Item { target.delayed(value) }
"#;

#[test]
fn inherited_generic_interface_bindings_require_checked_applications_and_keep_returns_alive() {
    let (vm, owner) = fixture(IDENTITY, None);
    let target: Interface = vm.execute_typed(&owner, "make", ()).unwrap();
    let item: Object = vm.execute_typed(&owner, "item", ()).unwrap();
    let cx = ConversionContext::new(vm.runtime(), &owner).unwrap();
    let integer = cx.type_for::<i32>().unwrap();
    let boolean = cx.type_for::<bool>().unwrap();
    let member = target.member(vm.runtime(), "identity").unwrap();
    assert!(
        vm.runtime()
            .bind_interface_method::<(i32,), i32>(&member)
            .is_err()
    );
    assert!(
        vm.runtime()
            .bind_interface_method_application::<(bool,), bool>(&member, &[boolean])
            .is_err()
    );
    assert!(
        vm.runtime()
            .bind_interface_method_application::<(i32,), bool>(&member, slice::from_ref(&integer))
            .is_err()
    );
    let scalar = vm
        .runtime()
        .bind_interface_method_application::<(i32,), i32>(&member, &[integer])
        .unwrap();
    let object = vm
        .runtime()
        .bind_interface_method_application::<(Object,), Object>(
            &member,
            slice::from_ref(item.object_type().type_argument()),
        )
        .unwrap();
    let delayed = vm
        .runtime()
        .bind_interface_method_application::<(Object,), PinnedFunction<(), Object>>(
            &target.member(vm.runtime(), "delayed").unwrap(),
            slice::from_ref(item.object_type().type_argument()),
        )
        .unwrap();
    let mut cx = vm.context(&owner).unwrap();
    vm.runtime().collect_garbage().unwrap();
    assert_eq!(target.call(&mut cx, &scalar, (42,)).unwrap(), 42);
    let returned = target.call(&mut cx, &object, (item.clone(),)).unwrap();
    let field = vm
        .runtime()
        .bind_field::<i32>(item.object_type(), "value")
        .unwrap();
    returned.set(&mut cx, &field, 43).unwrap();
    assert_eq!(item.get(&mut cx, &field).unwrap(), 43);
    let closure = target.call(&mut cx, &delayed, (item,)).unwrap();
    drop(returned);
    drop(target);
    vm.runtime().collect_garbage().unwrap();
    let returned = vm.call(&closure, ()).unwrap();
    assert_eq!(returned.get(&mut cx, &field).unwrap(), 43);
    drop(returned);
    drop(closure);
    drop(scalar);
    drop(object);
    drop(delayed);
    assert_eq!(vm.runtime().collect_garbage().unwrap().live_objects, 0);
    assert_eq!(vm.runtime().gc().active_roots(), 0);
}

#[test]
fn generic_interface_constraints_keep_selected_operations_alive_through_gc_and_reload() {
    let calls = Arc::new(AtomicUsize::new(0));
    let observed = calls.clone();
    let mut builder = ModuleBuilder::new(
        "example::interface_applications",
        &StandardDeclarations::default().catalog().unwrap(),
    );
    builder
        .add_function(
            FunctionSpec::new("collect"),
            move |cx: &mut NativeContext<'_>, (): ()| -> NativeResult<()> {
                cx.collect_garbage()?;
                observed.fetch_add(1, Ordering::Relaxed);
                Ok(())
            },
        )
        .unwrap();
    let module = builder.finish().unwrap();
    let source = r#"
        use example::interface_applications::collect;
        pub trait Read { fn read(self) -> i32; }
        pub struct Key<T> { pub val tag: T, pub val value: i32 }
        impl<T> Read for Key<T> { fn read(self) -> i32 { collect(); self.value } }
        pub trait Invoke { fn invoke<T: Read>(self, value: T) -> i32 { value.read() } }
        struct Worker {}
        impl Invoke for Worker {}
        pub fn make() -> Invoke { Worker {} }
        pub fn key() -> Key<i32> { Key { tag: 1, value: 42 } }
        pub fn evidence(target: Invoke, value: Key<i32>) -> i32 { target.invoke(value) }
    "#;
    let (vm, old) = fixture(source, Some(&module));
    let target: Interface = vm.execute_typed(&old, "make", ()).unwrap();
    let key: Object = vm.execute_typed(&old, "key", ()).unwrap();
    let member = target.member(vm.runtime(), "invoke").unwrap();
    let invoke = vm
        .runtime()
        .bind_interface_method_application::<(Object,), i32>(
            &member,
            slice::from_ref(key.object_type().type_argument()),
        )
        .unwrap();
    let duplicate = vm
        .runtime()
        .bind_interface_method_application::<(Object,), i32>(
            &member,
            slice::from_ref(key.object_type().type_argument()),
        )
        .unwrap();
    drop(member);
    vm.runtime().collect_garbage().unwrap();
    assert_eq!(calls.load(Ordering::Relaxed), 0);
    assert_eq!(
        target
            .call(&mut vm.context(&old).unwrap(), &invoke, (key.clone(),))
            .unwrap(),
        42
    );
    let new = vm
        .reload_program(
            &old,
            "functions",
            compile_program(
                &source.replace("collect(); self.value", "collect(); self.value + 1"),
                Some(&module),
            ),
        )
        .unwrap();
    let new_target: Interface = vm.execute_typed(&new, "make", ()).unwrap();
    let new_key: Object = vm.execute_typed(&new, "key", ()).unwrap();
    let new_invoke = vm
        .runtime()
        .bind_interface_method_application::<(Object,), i32>(
            &new_target.member(vm.runtime(), "invoke").unwrap(),
            slice::from_ref(new_key.object_type().type_argument()),
        )
        .unwrap();
    let mut cx = vm.context(&new).unwrap();
    assert!(
        new_target
            .call(&mut cx, &invoke, (new_key.clone(),))
            .is_err()
    );
    assert_eq!(calls.load(Ordering::Relaxed), 1);
    assert_eq!(target.call(&mut cx, &invoke, (key.clone(),)).unwrap(), 42);
    assert_eq!(
        new_target.call(&mut cx, &new_invoke, (new_key,)).unwrap(),
        43
    );
    assert_eq!(calls.load(Ordering::Relaxed), 3);
    drop(invoke);
    drop(target);
    drop(key);
    assert!(
        !vm.runtime()
            .collect_garbage()
            .unwrap()
            .reclaimed_modules
            .contains(&old.key())
    );
    drop(duplicate);
    assert!(
        vm.runtime()
            .collect_garbage()
            .unwrap()
            .reclaimed_modules
            .contains(&old.key())
    );
}
