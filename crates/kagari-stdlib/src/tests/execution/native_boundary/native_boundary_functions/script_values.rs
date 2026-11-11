use super::fixture;
use crate::{
    declarations::StandardDeclarations, tests::execution::native_boundary::publish_unused_version,
};
use kagari_runtime::native::{
    binding::NativeResult,
    builder::ModuleBuilder,
    declarations::{CallableRequirement, FunctionDecl, MethodDecl},
    function_handle::Function,
    interfaces::Interface,
    objects::Object,
    typed::NativeContext,
    value_handle::ScriptValue,
};

use std::slice;

#[test]
fn generic_value_handles_forward_selected_results_without_erasing_type_access_or_roots() {
    let mut module = ModuleBuilder::new(
        "example::script_values",
        &StandardDeclarations::default().catalog().unwrap(),
    )
    .unwrap();
    let mut contract = module.define_trait("Identity").unwrap();
    let member = contract
        .define_method(MethodDecl::instance("identity"))
        .unwrap();
    contract
        .method(&member, |method| {
            let value = method.type_parameter("T")?.ty();
            method.parameter("value", value.clone())?;
            method.returns(value)?;
            Ok(())
        })
        .unwrap();
    let contract = contract.finish().unwrap();
    let invoke = module.define_function(FunctionDecl::new("invoke")).unwrap();
    let selected = module
        .function(&invoke, |function| {
            let source = function.type_parameter("S")?.ty();
            let value = function.type_parameter("T")?.ty();
            function.parameter("source", source.clone())?;
            function.parameter("value", value.clone())?;
            function.returns(value.clone())?;
            function.bound(source.clone(), contract.apply([])?)?;
            function.requires(
                CallableRequirement::method(source, contract.method("identity")?)?
                    .arguments([value])?,
            )
        })
        .unwrap();
    module
        .bind(
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
    let (mut vm, owner) = fixture(
        include_str!("../../../../../tests/fixtures/native/generic-script-value-handles.kgr"),
        Some(&module),
    );
    let _owner_program = owner;
    let owner = vm.select_program(&_owner_program).unwrap();
    {
        let number = vm
            .bind_function::<(), ScriptValue>(&_owner_program.root(), "number")
            .unwrap();
        let text = vm
            .bind_function::<(), ScriptValue>(&_owner_program.root(), "text")
            .unwrap();
        let vector = vm
            .bind_function::<(), ScriptValue>(&_owner_program.root(), "vector")
            .unwrap();
        let closure = vm
            .bind_function::<(), ScriptValue>(&_owner_program.root(), "closure")
            .unwrap();
        let readonly = vm
            .bind_function::<(), ScriptValue>(&_owner_program.root(), "readonly")
            .unwrap();
        let string_argument = vm
            .bind_function::<(ScriptValue,), String>(&_owner_program.root(), "string_argument")
            .unwrap();
        let read = vm
            .bind_function::<(ScriptValue,), i32>(&_owner_program.root(), "read")
            .unwrap();
        let write = vm
            .bind_function::<(ScriptValue,), ()>(&_owner_program.root(), "write")
            .unwrap();
        let number = vm.call(&number, ()).unwrap();
        let text = vm.call(&text, ()).unwrap();
        let vector = vm.call(&vector, ()).unwrap();
        let closure = vm.call(&closure, ()).unwrap();
        let readonly = vm.call(&readonly, ()).unwrap();
        vm.collect_garbage().unwrap();
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
        let items = vector.decode::<(ScriptValue,)>(&mut cx).unwrap();
        let item = items.0.decode::<Object>(&mut cx).unwrap();
        let field = vm.bind_field::<i32>(item.object_type(), "value").unwrap();
        assert_eq!(item.get(&mut cx, &field).unwrap(), 42);
        let callback = closure.decode::<Function<(), Object>>(&mut cx).unwrap();
        drop((number, text, vector, closure, readonly, items, item));
        vm.collect_garbage().unwrap();
        let result = callback.call(&mut cx, ()).unwrap();
        assert_eq!(result.get(&mut cx, &field).unwrap(), 42);
        let (foreign, foreign_owner) = fixture("pub fn number() -> i32 { 7 }", None);
        let _foreign_owner_program = foreign_owner;
        let foreign_binding = foreign
            .bind_function::<(), ScriptValue>(&_foreign_owner_program.root(), "number")
            .unwrap();
        let value = foreign.call(&foreign_binding, ()).unwrap();
        assert!(value.decode::<i32>(&mut cx).is_err());
        drop((callback, result, cx));
    }
    publish_unused_version(&mut vm, &_owner_program);
    assert_eq!(vm.collect_garbage().unwrap().live_objects, 0);
    assert_eq!(vm.gc().active_roots(), 0);
}

#[test]
fn selected_associated_tuple_results_keep_their_supplying_scope() {
    let mut module = ModuleBuilder::new(
        "example::associated_values",
        &StandardDeclarations::default().catalog().unwrap(),
    )
    .unwrap();
    let mut contract = module.define_trait("Producer").unwrap();
    let items = contract.associated_type("Items", []).unwrap();
    let get = contract
        .define_method(MethodDecl::instance("items").returns(items.clone()))
        .unwrap();
    let again = contract
        .define_method(MethodDecl::instance("again").returns(items))
        .unwrap();
    let operation = contract.operation(&get).unwrap();
    let selected = contract
        .method(&again, |method| method.requires(operation))
        .unwrap();
    contract
        .bind(
            again,
            move |cx: &mut NativeContext<'_>,
                  receiver: ScriptValue,
                  (): ()|
                  -> NativeResult<ScriptValue> {
                let method = cx.selected_method::<(ScriptValue,), ScriptValue>(&selected)?;
                let result = method.call(cx, (receiver,))?;
                cx.collect_garbage()?;
                let items = result.decode::<(ScriptValue,)>(cx)?;
                let item = items.0.decode::<Object>(cx)?;
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
            type Items = (T,);
            fn items(self) -> (T,) { (self.item,) }
        }
        pub trait Relay { fn relay<P: Producer>(self, source: P) -> P::Items { source.again() } }
        impl Relay for i32 {}
        pub fn target() -> Relay { 0 }
        pub fn holder() -> Holder<Item> { Holder { item: Item { value: 42 } } }
        pub fn make() -> (Item,) {
            val r: Relay = 0;
            r.relay(Holder { item: Item { value: 42 } })
        }
    "#,
        Some(&module),
    );
    let _owner_program = owner;
    let owner = vm.select_program(&_owner_program).unwrap();
    let make = vm
        .bind_function::<(), (Object,)>(&_owner_program.root(), "make")
        .unwrap();
    let values = vm.call(&make, ()).unwrap();
    vm.collect_garbage().unwrap();
    let mut cx = vm.context(&owner).unwrap();
    let item = values.0.clone();
    let field = vm.bind_field::<i32>(item.object_type(), "value").unwrap();
    assert_eq!(item.get(&mut cx, &field).unwrap(), 43);
    let target: Interface = vm
        .execute_typed(&_owner_program, "target", (), &Default::default())
        .unwrap();
    let holder: Object = vm
        .execute_typed(&_owner_program, "holder", (), &Default::default())
        .unwrap();
    let relay = vm
        .bind_interface_method_application::<(Object,), (Object,)>(
            &target.member(&vm, "relay").unwrap(),
            slice::from_ref(holder.object_type().type_argument()),
        )
        .unwrap();
    vm.collect_garbage().unwrap();
    let returned = target.call(&mut cx, &relay, (holder,)).unwrap();
    let returned_item = returned.0.clone();
    assert_eq!(returned_item.get(&mut cx, &field).unwrap(), 43);
    drop((target, relay, returned, returned_item));
    drop((item, values, cx));
    assert_eq!(vm.collect_garbage().unwrap().live_objects, 0);
    assert_eq!(vm.gc().active_roots(), 0);
}
