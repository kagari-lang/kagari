use super::fixture;
use crate::publish_unused_version;
use kagari_runtime::native::{
    binding::NativeResult,
    builder::ModuleBuilder,
    collections::vector::ScriptVec,
    declarations::{CallableRequirement, FunctionDecl, MethodDecl},
    function_handle::PinnedFunction,
    interfaces::Interface,
    objects::Object,
    typed::NativeContext,
    value_handle::ScriptValue,
};
use kagari_stdlib::declarations::StandardDeclarations;
use std::slice;

#[test]
fn generic_value_handles_forward_selected_results_without_erasing_type_access_or_roots() {
    let mut module = ModuleBuilder::new(
        "example::script_values",
        &StandardDeclarations::default().catalog().unwrap(),
    );
    let mut contract = module.define_trait("Identity");
    let member = contract
        .define_method(MethodDecl::instance("identity"))
        .unwrap();
    contract
        .method(&member, |method| {
            let value = method.type_parameter("T")?.ty();
            method.parameter("value", value.clone());
            method.returns(value);
            Ok(())
        })
        .unwrap();
    let contract = contract.finish().unwrap();
    let invoke = module.define_function(FunctionDecl::new("invoke")).unwrap();
    let selected = module
        .function(&invoke, |function| {
            let source = function.type_parameter("S")?.ty();
            let value = function.type_parameter("T")?.ty();
            function.parameter("source", source.clone());
            function.parameter("value", value.clone());
            function.returns(value.clone());
            function.bound(source.clone(), contract.apply([]));
            Ok(function.requires(
                CallableRequirement::method(source, contract.method("identity")?)
                    .arguments([value]),
            ))
        })
        .unwrap();
    module
        .bind_typed(
            invoke,
            move |cx: &mut NativeContext<'_>,
                  (source, value): (ScriptValue, ScriptValue)|
                  -> NativeResult<ScriptValue> {
                let method =
                    cx.selected_method::<(ScriptValue, ScriptValue), ScriptValue>(&selected)?;
                cx.collect_garbage()?;
                let result = method.call(cx, (source, value))?;
                cx.collect_garbage()?;
                Ok(result)
            },
        )
        .unwrap();
    let module = module.finish().unwrap();
    let (vm, owner) = fixture(
        r#"
        use example::script_values::{Identity, invoke};
        use std::collections::{List, MutableList};
        impl Identity for i32 { fn identity<T>(self, value: T) -> T { value } }
        trait Relay { fn relay<T>(self, value: T) -> T { invoke(0, value) } }
        impl Relay for i32 {}
        pub struct Item { pub var value: i32 }
        pub fn number() -> i32 { val r: Relay = 0; r.relay(42) }
        pub fn text() -> String { val r: Relay = 0; r.relay("retained") }
        pub fn vector() -> Vec<Item> { val r: Relay = 0; r.relay([Item { value: 42 }]) }
        pub fn closure() -> fn() -> Item {
            val r: Relay = 0;
            val item = Item { value: 42 };
            val callback: fn() -> Item = || item;
            r.relay(callback)
        }
        pub fn readonly() -> List<i32> {
            val r: Relay = 0;
            val list: List<i32> = [42];
            r.relay(list)
        }
        pub fn read(list: List<i32>) -> i32 { list[0] }
        pub fn write(list: MutableList<i32>) { list[0] = 0; }
        pub fn string_argument(text: String) -> String { text }
    "#,
        Some(&module),
    );
    {
        let number = vm
            .runtime()
            .bind_function::<(), ScriptValue>(&owner, "number")
            .unwrap();
        let text = vm
            .runtime()
            .bind_function::<(), ScriptValue>(&owner, "text")
            .unwrap();
        let vector = vm
            .runtime()
            .bind_function::<(), ScriptValue>(&owner, "vector")
            .unwrap();
        let closure = vm
            .runtime()
            .bind_function::<(), ScriptValue>(&owner, "closure")
            .unwrap();
        let readonly = vm
            .runtime()
            .bind_function::<(), ScriptValue>(&owner, "readonly")
            .unwrap();
        let string_argument = vm
            .runtime()
            .bind_function::<(ScriptValue,), String>(&owner, "string_argument")
            .unwrap();
        let read = vm
            .runtime()
            .bind_function::<(ScriptValue,), i32>(&owner, "read")
            .unwrap();
        let write = vm
            .runtime()
            .bind_function::<(ScriptValue,), ()>(&owner, "write")
            .unwrap();
        let number = vm.call(&number, ()).unwrap();
        let text = vm.call(&text, ()).unwrap();
        let vector = vm.call(&vector, ()).unwrap();
        let closure = vm.call(&closure, ()).unwrap();
        let readonly = vm.call(&readonly, ()).unwrap();
        vm.runtime().collect_garbage().unwrap();
        let mut cx = vm.context(&owner).unwrap();
        assert_eq!(number.decode::<i32>(&mut cx).unwrap(), 42);
        assert!(number.decode::<String>(&mut cx).is_err());
        assert_eq!(text.decode::<String>(&mut cx).unwrap(), "retained");
        assert_eq!(
            vm.call(&string_argument, (text.clone(),)).unwrap(),
            "retained"
        );
        assert!(vm.call(&string_argument, (number.clone(),)).is_err());
        assert!(vm.call(&write, (readonly.clone(),)).is_err());
        assert_eq!(vm.call(&read, (readonly.clone(),)).unwrap(), 42);
        let items = vector.decode::<ScriptVec<ScriptValue>>(&mut cx).unwrap();
        let item = items
            .get(&mut cx, 0)
            .unwrap()
            .unwrap()
            .decode::<Object>(&mut cx)
            .unwrap();
        let field = vm
            .runtime()
            .bind_field::<i32>(item.object_type(), "value")
            .unwrap();
        assert_eq!(item.get(&mut cx, &field).unwrap(), 42);
        let callback = closure
            .decode::<PinnedFunction<(), Object>>(&mut cx)
            .unwrap();
        drop((number, text, vector, closure, readonly, items, item));
        vm.runtime().collect_garbage().unwrap();
        let result = callback.call(&mut cx, ()).unwrap();
        assert_eq!(result.get(&mut cx, &field).unwrap(), 42);
        let (foreign, foreign_owner) = fixture("pub fn number() -> i32 { 7 }", None);
        let foreign_binding = foreign
            .runtime()
            .bind_function::<(), ScriptValue>(&foreign_owner, "number")
            .unwrap();
        let value = foreign.call(&foreign_binding, ()).unwrap();
        assert!(value.decode::<i32>(&mut cx).is_err());
        drop((callback, result, cx));
    }
    publish_unused_version(&vm, &owner);
    assert_eq!(vm.runtime().collect_garbage().unwrap().live_objects, 0);
    assert_eq!(vm.runtime().gc().active_roots(), 0);
}

#[test]
fn selected_associated_collection_results_keep_their_supplying_scope() {
    let mut module = ModuleBuilder::new(
        "example::associated_values",
        &StandardDeclarations::default().catalog().unwrap(),
    );
    let mut contract = module.define_trait("Producer");
    let items = contract.associated_type("Items", []).unwrap();
    let get = contract
        .define_method(MethodDecl::instance("items").returns(items.clone()))
        .unwrap();
    let again = contract
        .define_method(MethodDecl::instance("again").returns(items))
        .unwrap();
    let operation = contract.operation(&get).unwrap();
    let selected = contract
        .method(&again, |method| Ok(method.requires(operation)))
        .unwrap();
    contract
        .bind_default_method(
            again,
            move |cx: &mut NativeContext<'_>,
                  receiver: ScriptValue,
                  (): ()|
                  -> NativeResult<ScriptValue> {
                let method = cx.selected_method::<(ScriptValue,), ScriptValue>(&selected)?;
                let result = method.call(cx, (receiver,))?;
                cx.collect_garbage()?;
                let items = result.decode::<ScriptVec<ScriptValue>>(cx)?;
                let item = items.get(cx, 0)?.unwrap().decode::<Object>(cx)?;
                let field = cx
                    .runtime()
                    .bind_field::<i32>(item.object_type(), "value")?;
                item.set(cx, &field, 43)?;
                Ok(result)
            },
        )
        .unwrap();
    contract.finish().unwrap();
    let module = module.finish().unwrap();
    let (vm, owner) = fixture(
        r#"
        use example::associated_values::Producer;
        pub struct Item { pub var value: i32 }
        pub struct Holder<T> { val item: T }
        impl<T> Producer for Holder<T> {
            type Items = Vec<T>;
            fn items(self) -> Vec<T> { [self.item] }
        }
        pub trait Relay { fn relay<P: Producer>(self, source: P) -> P::Items { source.again() } }
        impl Relay for i32 {}
        pub fn target() -> Relay { 0 }
        pub fn holder() -> Holder<Item> { Holder { item: Item { value: 42 } } }
        pub fn make() -> Vec<Item> {
            val r: Relay = 0;
            r.relay(Holder { item: Item { value: 42 } })
        }
    "#,
        Some(&module),
    );
    let make = vm
        .runtime()
        .bind_function::<(), ScriptVec<Object>>(&owner, "make")
        .unwrap();
    let values = vm.call(&make, ()).unwrap();
    vm.runtime().collect_garbage().unwrap();
    let mut cx = vm.context(&owner).unwrap();
    let item = values.get(&mut cx, 0).unwrap().unwrap();
    let field = vm
        .runtime()
        .bind_field::<i32>(item.object_type(), "value")
        .unwrap();
    assert_eq!(item.get(&mut cx, &field).unwrap(), 43);
    let target: Interface = vm.execute_typed(&owner, "target", ()).unwrap();
    let holder: Object = vm.execute_typed(&owner, "holder", ()).unwrap();
    let relay = vm
        .runtime()
        .bind_interface_method_application::<(Object,), ScriptVec<Object>>(
            &target.member(vm.runtime(), "relay").unwrap(),
            slice::from_ref(holder.object_type().type_argument()),
        )
        .unwrap();
    vm.runtime().collect_garbage().unwrap();
    let returned = target.call(&mut cx, &relay, (holder,)).unwrap();
    let returned_item = returned.get(&mut cx, 0).unwrap().unwrap();
    assert_eq!(returned_item.get(&mut cx, &field).unwrap(), 43);
    drop((target, relay, returned, returned_item));
    drop((item, values, cx));
    assert_eq!(vm.runtime().collect_garbage().unwrap().live_objects, 0);
    assert_eq!(vm.runtime().gc().active_roots(), 0);
}
