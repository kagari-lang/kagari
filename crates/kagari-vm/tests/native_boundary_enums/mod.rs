use crate::{compile_program, native_boundary_functions::fixture};
use kagari_runtime::{
    error::RuntimeError,
    frame::types::arguments::TypeArgument,
    native::{
        binding::NativeResult,
        builder::ModuleBuilder,
        catalog::DeclarationCatalog,
        conversion::{IntoKagari, KagariType, context::ConversionContext},
        objects::Object,
        types::Type,
        value_handle::ScriptValue,
    },
    value::Value,
};
use kagari_stdlib::declarations::StandardDeclarations;
use kagari_types::language::binding;
use std::{slice, thread};

const SOURCE: &str = r#"
    pub struct Item { pub var value: i32 }
    pub enum Packet<T> { Empty, Unit(()), Data(T, String) }
    enum Hidden { Value(i32) }
    pub fn item(value: i32) -> Item { Item { value: value } }
    pub fn read(packet: Packet<Item>) -> i32 {
        match packet { Packet::Empty => 0, Packet::Unit(value) => 1, Packet::Data(item, text) => item.value }
    }
    pub fn hidden() -> Hidden { Hidden::Value(42) }
"#;

#[test]
fn enum_factories_preserve_nominal_identity_payloads_and_empty_unit_distinction() {
    let (vm, owner) = fixture(SOURCE, None);
    let runtime = vm.runtime();
    let item_type = runtime.bind_type(&owner, "Item", &[]).unwrap();
    let packet = runtime
        .bind_enum_type(&owner, "Packet", slice::from_ref(item_type.type_argument()))
        .unwrap();
    let data = runtime
        .bind_enum_variant::<(Object, String)>(&packet, "Data")
        .unwrap();
    let empty = runtime.bind_enum_variant::<()>(&packet, "Empty").unwrap();
    let unit = runtime
        .bind_enum_variant_declaration::<((),)>(&packet.variant(runtime, "Unit").unwrap())
        .unwrap();
    assert!(runtime.bind_enum_type(&owner, "Hidden", &[]).is_err());
    assert!(runtime.bind_enum_type(&owner, "Packet", &[]).is_err());
    assert!(
        runtime
            .bind_enum_variant::<((),)>(&packet, "Empty")
            .is_err()
    );
    assert!(runtime.bind_enum_variant::<()>(&packet, "Unit").is_err());
    assert!(
        runtime
            .bind_enum_variant::<(Object, i32)>(&packet, "Data")
            .is_err()
    );
    assert!(runtime.bind_enum_variant::<()>(&packet, "Missing").is_err());
    let item: Object = vm.execute_typed(&owner, "item", (42,)).unwrap();
    let read = runtime
        .bind_function::<(ScriptValue,), i32>(&owner, "read")
        .unwrap();
    let mut cx = vm.context(&owner).unwrap();
    let value = data
        .create(&mut cx, (item.clone(), "retained".into()))
        .unwrap();
    let zero = empty.create(&mut cx, ()).unwrap();
    let one = unit.create(&mut cx, ((),)).unwrap();
    let field = runtime.bind_field::<i32>(&item_type, "value").unwrap();
    item.set(&mut cx, &field, 43).unwrap();
    drop(item);
    runtime.collect_garbage().unwrap();
    assert_eq!(vm.call(&read, (value.clone(),)).unwrap(), 43);
    assert_eq!(vm.call(&read, (zero,)).unwrap(), 0);
    assert_eq!(vm.call(&read, (one,)).unwrap(), 1);
    let other = ConversionContext::new(runtime, &owner)
        .unwrap()
        .type_for::<i32>()
        .unwrap();
    assert!(runtime.bind_enum_type(&owner, "Packet", &[other]).is_err());
    let (foreign, foreign_owner) = fixture("pub fn main() {}", None);
    assert!(
        empty
            .create(&mut foreign.context(&foreign_owner).unwrap(), ())
            .is_err()
    );
    drop(value);
    assert_eq!(runtime.collect_garbage().unwrap().live_objects, 0);
    assert_eq!(runtime.gc().active_roots(), 0);
}

#[test]
fn enum_variant_handles_keep_old_layouts_alive_and_release_them_after_reload() {
    let (vm, old) = fixture(SOURCE, None);
    let item_type = vm.runtime().bind_type(&old, "Item", &[]).unwrap();
    let packet = vm
        .runtime()
        .bind_enum_type(&old, "Packet", slice::from_ref(item_type.type_argument()))
        .unwrap();
    let data = vm
        .runtime()
        .bind_enum_variant::<(Object, String)>(&packet, "Data")
        .unwrap();
    let item: Object = vm.execute_typed(&old, "item", (42,)).unwrap();
    let new = vm
        .reload_program(&old, "functions", compile_program(SOURCE, None))
        .unwrap();
    let mut cx = vm.context(&new).unwrap();
    let value = data.create(&mut cx, (item, String::new())).unwrap();
    assert!(
        !vm.runtime()
            .collect_garbage()
            .unwrap()
            .reclaimed_modules
            .contains(&old.key())
    );
    assert_eq!(
        vm.execute_typed::<_, i32>(&new, "read", (value.clone(),))
            .unwrap(),
        42
    );
    drop((value, data, packet, item_type));
    let collected = vm.runtime().collect_garbage().unwrap();
    assert!(collected.reclaimed_modules.contains(&old.key()));
    assert_eq!(collected.live_objects, 0);
}

struct FailAfterGc;

impl KagariType for FailAfterGc {
    fn kagari_type(catalog: &DeclarationCatalog) -> NativeResult<Type> {
        String::kagari_type(catalog)
    }
}

impl IntoKagari for FailAfterGc {
    fn into_kagari(self, cx: &mut ConversionContext<'_>, _: &TypeArgument) -> NativeResult<Value> {
        assert_eq!(cx.runtime().collect_garbage()?.live_objects, 1);
        Err(RuntimeError::module_validation(
            "payload conversion failure",
        ))
    }
}

#[test]
fn failed_enum_payload_conversion_roots_earlier_fields_and_publishes_nothing() {
    let (vm, owner) = fixture(SOURCE, None);
    let item_type = vm.runtime().bind_type(&owner, "Item", &[]).unwrap();
    let packet = vm
        .runtime()
        .bind_enum_type(&owner, "Packet", slice::from_ref(item_type.type_argument()))
        .unwrap();
    let data = vm
        .runtime()
        .bind_enum_variant::<(Object, FailAfterGc)>(&packet, "Data")
        .unwrap();
    let item: Object = vm.execute_typed(&owner, "item", (42,)).unwrap();
    let roots = vm.runtime().gc().active_roots();
    assert!(roots > 0);
    assert!(
        data.create(&mut vm.context(&owner).unwrap(), (item, FailAfterGc))
            .is_err()
    );
    assert_eq!(vm.runtime().gc().active_roots(), 0);
    assert_eq!(vm.runtime().collect_garbage().unwrap().live_objects, 0);
}

#[test]
fn registered_enum_templates_construct_without_script_constructor_roots() {
    let (vm, owner) = fixture("pub struct Item { pub var value: i32 }", None);
    let runtime = vm.runtime();
    let item_type = runtime.bind_type(&owner, "Item", &[]).unwrap();
    let optional = runtime
        .bind_enum_type_declaration(
            &owner,
            &binding::option_declaration(),
            slice::from_ref(item_type.type_argument()),
        )
        .unwrap();
    let some = runtime
        .bind_enum_variant::<(Object,)>(&optional, "Some")
        .unwrap();
    let none = runtime.bind_enum_variant::<()>(&optional, "None").unwrap();
    let mut cx = vm.context(&owner).unwrap();
    let field = runtime.bind_field::<i32>(&item_type, "value").unwrap();
    let mut builder = item_type.builder().unwrap();
    builder.set(&mut cx, &field, 42).unwrap();
    let item = builder.build(&mut cx).unwrap();
    let value = some.create(&mut cx, (item,)).unwrap();
    let decoded: Option<Object> = value.decode(&mut cx).unwrap();
    drop(value);
    runtime.collect_garbage().unwrap();
    assert_eq!(decoded.as_ref().unwrap().get(&mut cx, &field).unwrap(), 42);
    let empty = none.create(&mut cx, ()).unwrap();
    assert!(empty.decode::<Option<Object>>(&mut cx).unwrap().is_none());
    assert!(empty.decode::<Result<Object, String>>(&mut cx).is_err());
    drop((empty, decoded));
    assert_eq!(runtime.collect_garbage().unwrap().live_objects, 0);
}

#[test]
fn enum_bindings_reject_unwitnessed_bounds_and_incompatible_payload_scopes() {
    let source = r#"
        pub enum Restricted<T: Eq> { Empty, Data(T) }
        pub fn evidence(value: Restricted<i32>) {}
        trait Factory { fn wrap<T: Eq>(self, value: T) -> Restricted<T>; }
        impl Factory for i32 { fn wrap<T: Eq>(self, value: T) -> Restricted<T> { Restricted::Data(value) } }
        pub fn shared() -> Restricted<i32> { val factory: Factory = 0; factory.wrap(1) }
    "#;
    let (vm, owner) = fixture(source, None);
    assert!(
        owner
            .bytecode
            .enumerations
            .iter()
            .any(|layout| layout.arguments.iter().any(|ty| !ty.is_concrete()))
    );
    let conversion = ConversionContext::new(vm.runtime(), &owner).unwrap();
    let integer = conversion.type_for::<i32>().unwrap();
    let float = conversion.type_for::<f64>().unwrap();
    let restricted = vm
        .runtime()
        .bind_enum_type(&owner, "Restricted", &[integer])
        .unwrap();
    vm.runtime()
        .bind_enum_variant::<()>(&restricted, "Empty")
        .unwrap();
    assert!(
        vm.runtime()
            .bind_enum_type(&owner, "Restricted", &[float])
            .is_err()
    );

    let (mut vm, old) = fixture(SOURCE, None);
    let old_item = vm.runtime().bind_type(&old, "Item", &[]).unwrap();
    let old_packet = vm
        .runtime()
        .bind_enum_type(&old, "Packet", slice::from_ref(old_item.type_argument()))
        .unwrap();
    let old_data = vm
        .runtime()
        .bind_enum_variant::<(Object, String)>(&old_packet, "Data")
        .unwrap();
    let changed = SOURCE
        .replace("pub var value: i32", "pub var value: i64")
        .replace("item(value: i32)", "item(value: i64)")
        .replace("-> i32", "-> i64");
    let new = vm
        .runtime_mut()
        .load_program("independent", compile_program(&changed, None))
        .unwrap();
    let new_item = vm.runtime().bind_type(&new, "Item", &[]).unwrap();
    assert_eq!(new_item.type_argument().ty(), old_item.type_argument().ty());
    assert!(
        vm.runtime()
            .bind_enum_type(&old, "Packet", slice::from_ref(new_item.type_argument()))
            .is_err()
    );
    let item: Object = vm.execute_typed(&new, "item", (42i64,)).unwrap();
    assert!(
        old_data
            .create(&mut vm.context(&new).unwrap(), (item, String::new()))
            .is_err()
    );
    assert_eq!(vm.runtime().gc().active_roots(), 0);
    assert_eq!(vm.runtime().collect_garbage().unwrap().live_objects, 0);
}

#[test]
fn registered_variant_identities_bind_directly_and_move_with_the_runtime() {
    let mut module = ModuleBuilder::new(
        "example::events",
        &StandardDeclarations::default().catalog().unwrap(),
    );
    let mut builder = module.define_enum("Event");
    let element = builder.type_parameter("T").unwrap();
    let data = builder.variant("Data", [element.ty()]).unwrap();
    let event = builder.finish().unwrap();
    let mut other = module.define_enum("Other");
    let wrong = other.variant("Data", [Type::i32()]).unwrap();
    other.finish().unwrap();
    let module = module.finish().unwrap();
    let (vm, owner) = fixture(
        r#"
        use example::events::Event;
        pub fn read(event: Event<i32>) -> i32 { match event { Event::Data(value) => value } }
    "#,
        Some(&module),
    );
    let element = ConversionContext::new(vm.runtime(), &owner)
        .unwrap()
        .type_for::<i32>()
        .unwrap();
    let applied = vm
        .runtime()
        .bind_enum_type_declaration(&owner, event.id(), &[element])
        .unwrap();
    let member = applied
        .variant_declaration(vm.runtime(), data.id())
        .unwrap();
    let variant = vm
        .runtime()
        .bind_enum_variant_declaration::<(i32,)>(&member)
        .unwrap();
    assert!(
        applied
            .variant_declaration(vm.runtime(), wrong.id())
            .is_err()
    );
    let duplicate = variant.clone();
    let answer = thread::spawn(move || {
        let mut cx = vm.context(&owner).unwrap();
        let value = duplicate.create(&mut cx, (42,)).unwrap();
        let another = variant.create(&mut cx, (43,)).unwrap();
        assert_eq!(
            vm.execute_typed::<_, i32>(&owner, "read", (another,))
                .unwrap(),
            43
        );
        vm.execute_typed::<_, i32>(&owner, "read", (value,))
            .unwrap()
    })
    .join()
    .unwrap();
    assert_eq!(answer, 42);
}
