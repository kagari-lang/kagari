use super::fixture;
use crate::{
    declarations::StandardDeclarations, tests::execution::native_boundary::publish_unused_version,
};
use kagari_runtime::{
    error::RuntimeErrorKind,
    frame::types::arguments::TypeArgument,
    native::{
        binding::NativeResult,
        builder::ModuleBuilder,
        catalog::DeclarationCatalog,
        conversion::{FromKagari, KagariType, context::ConversionContext},
        declarations::{CallableRequirement, FunctionDecl, MethodDecl},
        function_handle::Function,
        interfaces::Interface,
        objects::Object,
        typed::NativeContext,
        types::Type,
        value_handle::ScriptValue,
    },
    value::view::ValueView,
};
use std::{
    slice,
    sync::atomic::{AtomicUsize, Ordering},
};

static STRING_DECODES: AtomicUsize = AtomicUsize::new(0);

#[derive(Debug)]
struct ObservedString(String);

impl KagariType for ObservedString {
    fn kagari_type(catalog: &DeclarationCatalog) -> NativeResult<Type> {
        String::kagari_type(catalog)
    }
}

impl FromKagari for ObservedString {
    fn from_kagari(
        cx: &mut ConversionContext<'_>,
        expected: &TypeArgument,
        value: &ValueView<'_>,
    ) -> NativeResult<Self> {
        STRING_DECODES.fetch_add(1, Ordering::Relaxed);
        String::from_kagari(cx, expected, value).map(Self)
    }
}

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
    vm.collect_garbage().unwrap();
    let baseline = vm.gc_stats();
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
        STRING_DECODES.store(0, Ordering::Relaxed);
        let error = number.decode::<ObservedString>(&mut cx).unwrap_err();
        assert_eq!(error.kind(), RuntimeErrorKind::ModuleValidation);
        assert_eq!(STRING_DECODES.load(Ordering::Relaxed), 0);
        assert_eq!(
            text.decode::<ObservedString>(&mut cx).unwrap().0,
            "retained"
        );
        assert_eq!(STRING_DECODES.load(Ordering::Relaxed), 1);
        assert_eq!(
            cx.runtime_mut()
                .call(&string_argument, (text.clone(),))
                .unwrap(),
            "retained"
        );
        assert!(
            cx.runtime_mut()
                .call(&string_argument, (number.clone(),))
                .is_err()
        );
        assert!(cx.runtime_mut().call(&write, (readonly.clone(),)).is_err());
        assert_eq!(
            cx.runtime_mut().call(&read, (readonly.clone(),)).unwrap(),
            42
        );
        let items = vector.decode::<(ScriptValue,)>(&mut cx).unwrap();
        let item = items.0.decode::<Object>(&mut cx).unwrap();
        let field = cx
            .runtime_mut()
            .bind_field::<i32>(item.object_type(), "value")
            .unwrap();
        assert_eq!(item.get(&mut cx, &field).unwrap(), 42);
        let callback = closure.decode::<Function<(), Object>>(&mut cx).unwrap();
        drop((number, text, vector, closure, readonly, items, item));
        cx.runtime_mut().collect_garbage().unwrap();
        let result = callback.call(&mut cx, ()).unwrap();
        assert_eq!(result.get(&mut cx, &field).unwrap(), 42);
        let (mut foreign, foreign_owner) = fixture("pub fn number() -> i32 { 7 }", None);
        let _foreign_owner_program = foreign_owner;
        let foreign_binding = foreign
            .bind_function::<(), ScriptValue>(&_foreign_owner_program.root(), "number")
            .unwrap();
        let value = foreign.call(&foreign_binding, ()).unwrap();
        assert!(value.decode::<i32>(&mut cx).is_err());
        drop((callback, result, cx));
    }
    publish_unused_version(&mut vm, &_owner_program);
    vm.collect_garbage().unwrap();
    assert_eq!(vm.gc_stats().objects, baseline.objects);
    assert_eq!(
        vm.gc_stats().root_registrations,
        baseline.root_registrations
    );
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
                    .runtime_mut()
                    .bind_field::<i32>(item.object_type(), "value")?;
                item.set(cx, &field, 43)?;
                Ok(result)
            },
        )
        .unwrap();
    contract.finish().unwrap();
    let module = module.finish().unwrap();
    let (mut vm, owner) = fixture(
        include_str!("../../../../../tests/fixtures/native/function-associated-tuple-scope.kgr"),
        Some(&module),
    );
    let _owner_program = owner;
    let owner = vm.select_program(&_owner_program).unwrap();
    vm.collect_garbage().unwrap();
    let baseline = vm.gc_stats();
    let make = vm
        .bind_function::<(), (Object,)>(&_owner_program.root(), "make")
        .unwrap();
    let values = vm.call(&make, ()).unwrap();
    vm.collect_garbage().unwrap();
    let mut cx = vm.context(&owner).unwrap();
    let item = values.0.clone();
    let field = cx
        .runtime_mut()
        .bind_field::<i32>(item.object_type(), "value")
        .unwrap();
    assert_eq!(item.get(&mut cx, &field).unwrap(), 43);
    let target: Interface = cx
        .runtime_mut()
        .execute_typed(&_owner_program, "target", (), &Default::default())
        .unwrap();
    let holder: Object = cx
        .runtime_mut()
        .execute_typed(&_owner_program, "holder", (), &Default::default())
        .unwrap();
    let member = target.member(cx.runtime_mut(), "relay").unwrap();
    let relay = cx
        .runtime_mut()
        .bind_interface_method_application::<(Object,), (Object,)>(
            &member,
            slice::from_ref(holder.object_type().type_argument()),
        )
        .unwrap();
    cx.runtime_mut().collect_garbage().unwrap();
    let returned = target.call(&mut cx, &relay, (holder,)).unwrap();
    let returned_item = returned.0.clone();
    assert_eq!(returned_item.get(&mut cx, &field).unwrap(), 43);
    drop((target, relay, returned, returned_item));
    drop((item, values, cx, make, field, member));
    publish_unused_version(&mut vm, &_owner_program);
    vm.collect_garbage().unwrap();
    assert_eq!(vm.gc_stats().objects, baseline.objects);
    assert_eq!(
        vm.gc_stats().root_registrations,
        baseline.root_registrations
    );
}
