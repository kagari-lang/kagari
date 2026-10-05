use super::fixture;
use crate::{compile_program, native_boundary_interfaces::generic_identity_module};
use kagari_common::identity::DefinitionKind;
use kagari_runtime::{
    native::{
        binding::{Codec, NativeBinding},
        builder::ModuleBuilder,
        collections::vector::ScriptVec,
        conversion::context::ConversionContext,
        declarations::{CallableRequirement, FunctionDecl, MethodDecl},
        function_handle::PinnedFunction,
        objects::Object,
        types::Type,
    },
    value::Value,
};
use kagari_stdlib::declarations::StandardDeclarations;
use std::{
    slice,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
};

#[test]
fn generic_function_binding_uses_closed_witnesses_and_rejects_missing_applications() {
    let module = generic_identity_module();
    let declaration = module
        .to_declaration()
        .unwrap()
        .definition(DefinitionKind::Function, "identity");
    let (vm, owner) = fixture(
        r#"
        use example::generic_helper::identity;
        pub fn evidence() -> i32 { identity(42) }
        fn hidden<T>(value: T) -> T { value }
        pub fn hidden_evidence() -> i32 { hidden(1) }
    "#,
        Some(&module),
    );
    let conversion = ConversionContext::new(vm.runtime(), &owner).unwrap();
    let i32_type = conversion.type_for::<i32>().unwrap();
    let string_type = conversion.type_for::<String>().unwrap();
    let identity = vm
        .runtime()
        .bind_function_application_declaration::<(i32,), i32>(
            &owner,
            &declaration,
            slice::from_ref(&i32_type),
        )
        .unwrap();
    assert_eq!(vm.call(&identity, (42,)).unwrap(), 42);
    let native = owner
        .members()
        .find(|member| member.bytecode.identity == declaration.module)
        .unwrap();
    let by_name = vm
        .runtime()
        .bind_function_application::<(i32,), i32>(&native, "identity", slice::from_ref(&i32_type))
        .unwrap();
    assert_eq!(vm.call(&by_name, (43,)).unwrap(), 43);

    assert!(
        vm.runtime()
            .bind_function_application_declaration::<(String,), String>(
                &owner,
                &declaration,
                &[string_type]
            )
            .is_err()
    );
    assert!(
        vm.runtime()
            .bind_function_application::<(i32,), i32>(&owner, "hidden", slice::from_ref(&i32_type))
            .is_err()
    );
    assert!(
        vm.runtime()
            .bind_function_application_declaration::<(i32,), i32>(&owner, &declaration, &[])
            .is_err()
    );
    assert!(
        vm.runtime()
            .bind_function_application_declaration::<(i32,), bool>(
                &owner,
                &declaration,
                &[i32_type]
            )
            .is_err()
    );
}

#[test]
fn generic_native_calls_return_retained_objects_collections_and_closures() {
    let module = generic_identity_module();
    let identity_id = module
        .to_declaration()
        .unwrap()
        .definition(DefinitionKind::Function, "identity");
    let singleton_id = module
        .to_declaration()
        .unwrap()
        .definition(DefinitionKind::Function, "singleton");
    let (vm, owner) = fixture(
        r#"
        use example::generic_helper::{identity, singleton};
        pub struct Item { pub val value: i32 }
        pub fn make() -> Item { identity(Item { value: 42 }) }
        pub fn evidence() -> Vec<i32> { singleton(1) }
        pub fn closure() -> fn() -> i32 { identity(|| 42) }
    "#,
        Some(&module),
    );
    let conversion = ConversionContext::new(vm.runtime(), &owner).unwrap();
    let i32_type = conversion.type_for::<i32>().unwrap();
    let item = vm.runtime().bind_type(&owner, "Item", &[]).unwrap();
    let identity = vm
        .runtime()
        .bind_function_application_declaration::<(Object,), Object>(
            &owner,
            &identity_id,
            slice::from_ref(item.type_argument()),
        )
        .unwrap();
    let make = vm
        .runtime()
        .bind_function::<(), Object>(&owner, "make")
        .unwrap();
    let object = vm.call(&identity, (vm.call(&make, ()).unwrap(),)).unwrap();
    vm.runtime().collect_garbage().unwrap();
    let value = vm.runtime().bind_field::<i32>(&item, "value").unwrap();
    assert_eq!(
        object
            .get(&mut vm.context(&owner).unwrap(), &value)
            .unwrap(),
        42
    );
    let singleton = vm
        .runtime()
        .bind_function_application_declaration::<(i32,), ScriptVec<i32>>(
            &owner,
            &singleton_id,
            &[i32_type],
        )
        .unwrap();
    let values = vm.call(&singleton, (42,)).unwrap();
    assert_eq!(
        values.get(&mut vm.context(&owner).unwrap(), 0).unwrap(),
        Some(42)
    );
    type Callback = PinnedFunction<(), i32>;
    let callback_type = conversion.type_for::<Callback>().unwrap();
    let identity = vm
        .runtime()
        .bind_function_application_declaration::<(Callback,), Callback>(
            &owner,
            &identity_id,
            &[callback_type],
        )
        .unwrap();
    let closure = vm
        .runtime()
        .bind_function::<(), Callback>(&owner, "closure")
        .unwrap();
    let callback = vm
        .call(&identity, (vm.call(&closure, ()).unwrap(),))
        .unwrap();
    vm.runtime().collect_garbage().unwrap();
    assert_eq!(vm.call(&callback, ()).unwrap(), 42);
}

#[test]
fn generic_bindings_pin_their_program_and_weak_cache_releases_it() {
    let module = generic_identity_module();
    let declaration = module
        .to_declaration()
        .unwrap()
        .definition(DefinitionKind::Function, "identity");
    let source = r#"
        use example::generic_helper::identity;
        pub struct Item { pub val value: i32 }
        pub fn make() -> Item { identity(Item { value: 41 }) }
    "#;
    let (vm, old) = fixture(source, Some(&module));
    let item = vm.runtime().bind_type(&old, "Item", &[]).unwrap();
    let argument = item.type_argument().clone();
    let identity = vm
        .runtime()
        .bind_function_application_declaration::<(Object,), Object>(
            &old,
            &declaration,
            slice::from_ref(&argument),
        )
        .unwrap();
    let duplicate = vm
        .runtime()
        .bind_function_application_declaration::<(Object,), Object>(
            &old,
            &declaration,
            slice::from_ref(&argument),
        )
        .unwrap();
    let make = vm
        .runtime()
        .bind_function::<(), Object>(&old, "make")
        .unwrap();
    let object = vm.call(&make, ()).unwrap();
    let new_source = source
        .replace(
            "pub val value: i32",
            "pub val value: i32, pub val extra: bool",
        )
        .replace("value: 41", "value: 42, extra: true");
    assert!(
        vm.reload_program(
            &old,
            "functions",
            compile_program(&new_source, Some(&module))
        )
        .is_err()
    );
    // A rejected schema publication leaves the retained generic application usable.
    assert!(vm.call(&identity, (object.clone(),)).is_ok());
    let new_source = source.replace("value: 41", "value: 42");
    let new = vm
        .reload_program(
            &old,
            "functions",
            compile_program(&new_source, Some(&module)),
        )
        .unwrap();
    let current = vm
        .runtime()
        .bind_function::<(), Object>(&new, "make")
        .unwrap();
    let current_object = vm.call(&current, ()).unwrap();
    let current_type = current_object.object_type();
    let current_value = vm
        .runtime()
        .bind_field::<i32>(current_type, "value")
        .unwrap();
    assert_eq!(
        current_object
            .get(&mut vm.context(&new).unwrap(), &current_value)
            .unwrap(),
        42
    );
    let retained = identity
        .call(&mut vm.context(&new).unwrap(), (object,))
        .unwrap();
    let value = vm.runtime().bind_field::<i32>(&item, "value").unwrap();
    assert_eq!(
        retained
            .get(&mut vm.context(&new).unwrap(), &value)
            .unwrap(),
        41
    );
    drop((retained, value, item, make, identity));
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

#[test]
fn bound_native_application_uses_the_compilers_selected_trait_operation() {
    let mut builder = ModuleBuilder::new(
        "example::bound",
        &StandardDeclarations::default().catalog().unwrap(),
    );
    let mut step = builder.define_trait("Step");
    step.define_method(
        MethodDecl::instance("step")
            .parameter("value", Type::i32())
            .returns(Type::i32()),
    )
    .unwrap();
    let step = step.finish().unwrap();
    let repeat = builder
        .define_function(FunctionDecl::new("repeat").returns(Type::i32()))
        .unwrap();
    let declaration = repeat.id().clone();
    let selected = builder
        .function(&repeat, |function| {
            let item = function.type_parameter("T")?.ty();
            function.parameter("item", item.clone());
            function.bound(item.clone(), step.apply([]));
            Ok(function.requires(CallableRequirement::method(item, step.method("step")?)))
        })
        .unwrap();
    let effects = Arc::new(AtomicUsize::new(0));
    let observed = effects.clone();
    builder
        .bind_with(
            repeat,
            NativeBinding::new(vec![Codec::Value], Codec::Value, move |cx| {
                observed.fetch_add(1, Ordering::Relaxed);
                let item = cx.argument(0)?;
                let operation = cx.selected(&selected)?;
                cx.collect_garbage()?;
                cx.call_values(operation, &[item, Value::I32(0)])
            }),
        )
        .unwrap();
    let module = builder.finish().unwrap();
    let (vm, owner) = fixture(
        r#"
        use example::bound::{Step, repeat};
        pub struct Counter { pub val amount: i32 }
        impl Step for Counter { fn step(self, value: i32) -> i32 { value + self.amount } }
        pub fn make() -> Counter { Counter { amount: 42 } }
        pub fn evidence() -> i32 { repeat(make()) }
    "#,
        Some(&module),
    );
    let counter = vm.runtime().bind_type(&owner, "Counter", &[]).unwrap();
    let repeat = vm
        .runtime()
        .bind_function_application_declaration::<(Object,), i32>(
            &owner,
            &declaration,
            slice::from_ref(counter.type_argument()),
        )
        .unwrap();
    let integer = ConversionContext::new(vm.runtime(), &owner)
        .unwrap()
        .type_for::<i32>()
        .unwrap();
    assert!(
        vm.runtime()
            .bind_function_application_declaration::<(i32,), i32>(&owner, &declaration, &[integer])
            .is_err()
    );
    assert_eq!(effects.load(Ordering::Relaxed), 0);
    let make = vm
        .runtime()
        .bind_function::<(), Object>(&owner, "make")
        .unwrap();
    assert_eq!(
        vm.call(&repeat, (vm.call(&make, ()).unwrap(),)).unwrap(),
        42
    );
    assert_eq!(effects.load(Ordering::Relaxed), 1);
}
