use crate::{
    declarations::StandardDeclarations,
    tests::{
        execution::native_boundary::{
            compile_program,
            native_boundary_functions::{fixture, fixture_without_foundation},
        },
        support::bytecode::reload_bytecode,
    },
};

#[cfg(test)]
use crate::tests::support::bytecode::load_bytecode;

use kagari_runtime::{
    error::{RuntimeError, RuntimeErrorKind},
    frame::types::arguments::TypeArgument,
    native::{
        assembly::{ModuleDefinition, ModuleSetBuilder},
        binding::NativeResult,
        builder::ModuleBuilder,
        catalog::DeclarationCatalog,
        conversion::{IntoKagari, KagariType, context::ConversionContext},
        objects::Object,
        types::Type,
        value_handle::ScriptValue,
    },
    value::view::ValueView,
};

use kagari_common::identity::{ModuleIdentity, PackageId};

use kagari_types::{declaration::native::NativeTypeConstructor, language::binding};
use std::{slice, thread};

const SOURCE: &str = include_str!("../../../../tests/fixtures/core_enum_payloads.kgr");

#[test]
fn enum_factories_preserve_nominal_identity_payloads_and_empty_unit_distinction() {
    let mut providers = ModuleSetBuilder::new(&DeclarationCatalog::default()).unwrap();
    providers
        .add(ModuleDefinition::new(
            ModuleIdentity {
                package: PackageId("kagari-alloc".into()),
                path: vec!["string".into()],
            },
            |module| {
                module.package_alias("alloc");
                module.define_constructor("String", NativeTypeConstructor::String)?;
                Ok(())
            },
        ))
        .unwrap();
    let strings = providers
        .finish()
        .unwrap()
        .into_modules()
        .unwrap()
        .pop()
        .unwrap();
    let (mut vm, owner) = fixture_without_foundation(SOURCE, Some(&strings));
    let _owner_program = owner;
    let owner = vm.select_program(&_owner_program).unwrap();
    vm.collect_garbage().unwrap();
    let baseline = vm.gc_stats();
    let runtime = &mut vm;
    let item_type = runtime
        .bind_type(&_owner_program.root(), "Item", &[])
        .unwrap();
    let packet = runtime
        .bind_enum_type(
            &_owner_program.root(),
            "Packet",
            slice::from_ref(item_type.type_argument()),
        )
        .unwrap();
    let data = runtime
        .bind_enum_variant::<(Object, String)>(&packet, "Data")
        .unwrap();
    let empty = runtime.bind_enum_variant::<()>(&packet, "Empty").unwrap();
    let unit = runtime
        .bind_enum_variant_declaration::<((),)>(&packet.variant(runtime, "Unit").unwrap())
        .unwrap();
    assert!(
        runtime
            .bind_enum_type(&_owner_program.root(), "Hidden", &[])
            .is_err()
    );
    assert!(
        runtime
            .bind_enum_type(&_owner_program.root(), "Packet", &[])
            .is_err()
    );
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
    let item: Object = runtime
        .execute_typed(&_owner_program, "item", (42,), &Default::default())
        .unwrap();
    let read = runtime
        .bind_function::<(ScriptValue,), i32>(&_owner_program.root(), "read")
        .unwrap();
    let mut cx = runtime.context(&owner).unwrap();
    let value = data
        .create(&mut cx, (item.clone(), "retained".into()))
        .unwrap();
    let zero = empty.create(&mut cx, ()).unwrap();
    let one = unit.create(&mut cx, ((),)).unwrap();
    let field = cx
        .runtime_mut()
        .bind_field::<i32>(&item_type, "value")
        .unwrap();
    item.set(&mut cx, &field, 43).unwrap();
    drop(item);
    cx.runtime_mut().collect_garbage().unwrap();
    assert_eq!(cx.runtime_mut().call(&read, (value.clone(),)).unwrap(), 43);
    assert_eq!(cx.runtime_mut().call(&read, (zero,)).unwrap(), 0);
    assert_eq!(cx.runtime_mut().call(&read, (one,)).unwrap(), 1);
    let other = ConversionContext::new(cx.runtime_mut(), &owner)
        .unwrap()
        .type_for::<i32>()
        .unwrap();
    assert!(
        cx.runtime_mut()
            .bind_enum_type(&_owner_program.root(), "Packet", &[other])
            .is_err()
    );
    let (mut foreign, foreign_owner) = fixture("pub fn main() {}", None);
    let _foreign_owner_program = foreign_owner;
    let foreign_owner = foreign.select_program(&_foreign_owner_program).unwrap();
    assert!(
        empty
            .create(&mut foreign.context(&foreign_owner).unwrap(), ())
            .is_err()
    );
    drop(value);
    drop(cx);
    drop((field, read, data, empty, unit, packet, item_type));
    vm.collect_garbage().unwrap();
    assert_eq!(vm.gc_stats().objects, baseline.objects);
    assert_eq!(
        vm.gc_stats().root_registrations,
        baseline.root_registrations
    );
}

#[test]
fn enum_variant_handles_keep_old_layouts_alive_and_release_them_after_reload() {
    let (mut vm, old) = fixture(SOURCE, None);
    let _old_program = old;
    let old = vm.select_program(&_old_program).unwrap();
    vm.collect_garbage().unwrap();
    let baseline = vm.gc_stats();
    let item_type = vm.bind_type(&_old_program.root(), "Item", &[]).unwrap();
    let packet = vm
        .bind_enum_type(
            &_old_program.root(),
            "Packet",
            slice::from_ref(item_type.type_argument()),
        )
        .unwrap();
    let data = vm
        .bind_enum_variant::<(Object, String)>(&packet, "Data")
        .unwrap();
    let item: Object = vm
        .execute_typed(&_old_program, "item", (42,), &Default::default())
        .unwrap();
    reload_bytecode(
        &mut vm,
        &_old_program,
        Some("functions".into()),
        compile_program(SOURCE, None),
    )
    .unwrap();
    let _new_program = _old_program.clone();
    let new = vm.select_program(&_new_program).unwrap();
    let mut cx = vm.context(&new).unwrap();
    let value = data.create(&mut cx, (item, String::new())).unwrap();
    cx.runtime_mut().collect_garbage().unwrap();
    assert!(cx.runtime().validate_executable_module(&old).is_ok());
    assert_eq!(
        cx.runtime_mut()
            .execute_typed::<_, i32>(&_new_program, "read", (value.clone(),), &Default::default())
            .unwrap(),
        42
    );
    drop((value, data, packet, item_type));
    drop(cx);
    vm.collect_garbage().unwrap();
    assert!(vm.validate_executable_module(&old).is_err());
    assert_eq!(vm.gc_stats().objects, baseline.objects);
    assert_eq!(
        vm.gc_stats().root_registrations,
        baseline.root_registrations
    );
}

struct FailAfterGc {
    live_objects: usize,
}

impl KagariType for FailAfterGc {
    fn kagari_type(catalog: &DeclarationCatalog) -> NativeResult<Type> {
        String::kagari_type(catalog)
    }
}

impl IntoKagari for FailAfterGc {
    fn into_kagari<'call>(
        self,
        cx: &mut ConversionContext<'call>,
        _: &TypeArgument,
    ) -> NativeResult<ValueView<'call>> {
        cx.runtime_mut().collect_garbage()?;
        // The first Object argument has already moved into the conversion pack;
        // this witness owns no root that could keep that earlier field alive.
        assert_eq!(cx.runtime().gc_stats().objects, self.live_objects);
        Err(RuntimeError::module_validation(
            "payload conversion failure",
        ))
    }
}

#[test]
fn failed_enum_payload_conversion_roots_earlier_fields_and_publishes_nothing() {
    let (mut vm, owner) = fixture(SOURCE, None);
    let _owner_program = owner;
    let owner = vm.select_program(&_owner_program).unwrap();
    let item_type = vm.bind_type(&_owner_program.root(), "Item", &[]).unwrap();
    let packet = vm
        .bind_enum_type(
            &_owner_program.root(),
            "Packet",
            slice::from_ref(item_type.type_argument()),
        )
        .unwrap();
    let data = vm
        .bind_enum_variant::<(Object, FailAfterGc)>(&packet, "Data")
        .unwrap();
    vm.collect_garbage().unwrap();
    let baseline = vm.gc_stats();
    let item: Object = vm
        .execute_typed(&_owner_program, "item", (42,), &Default::default())
        .unwrap();
    vm.collect_garbage().unwrap();
    let live_objects = vm.gc_stats().objects;
    assert_eq!(live_objects, baseline.objects + 1);
    let error = data
        .create(
            &mut vm.context(&owner).unwrap(),
            (item, FailAfterGc { live_objects }),
        )
        .unwrap_err();
    assert_eq!(error.kind(), RuntimeErrorKind::ModuleValidation);
    assert!(error.to_string().contains("payload conversion failure"));
    vm.collect_garbage().unwrap();
    assert_eq!(
        vm.gc_stats().root_registrations,
        baseline.root_registrations
    );
    assert_eq!(vm.gc_stats().objects, baseline.objects);
}

#[test]
fn registered_enum_templates_construct_without_script_constructor_roots() {
    let (mut vm, owner) = fixture("pub struct Item { pub var value: i32 }", None);
    let _owner_program = owner;
    let owner = vm.select_program(&_owner_program).unwrap();
    vm.collect_garbage().unwrap();
    let baseline = vm.gc_stats();
    let runtime = &mut vm;
    let item_type = runtime
        .bind_type(&_owner_program.root(), "Item", &[])
        .unwrap();
    let optional = runtime
        .bind_enum_type_declaration(
            &_owner_program.root(),
            &binding::option_declaration(),
            slice::from_ref(item_type.type_argument()),
        )
        .unwrap();
    let some = runtime
        .bind_enum_variant::<(Object,)>(&optional, "Some")
        .unwrap();
    let none = runtime.bind_enum_variant::<()>(&optional, "None").unwrap();
    let mut cx = runtime.context(&owner).unwrap();
    let field = cx
        .runtime_mut()
        .bind_field::<i32>(&item_type, "value")
        .unwrap();
    let mut builder = item_type.builder().unwrap();
    builder.set(&mut cx, &field, 42).unwrap();
    let item = builder.build(&mut cx).unwrap();
    let value = some.create(&mut cx, (item,)).unwrap();
    let decoded: Option<Object> = value.decode(&mut cx).unwrap();
    drop(value);
    cx.runtime_mut().collect_garbage().unwrap();
    assert_eq!(decoded.as_ref().unwrap().get(&mut cx, &field).unwrap(), 42);
    let empty = none.create(&mut cx, ()).unwrap();
    assert!(empty.decode::<Option<Object>>(&mut cx).unwrap().is_none());
    assert!(empty.decode::<Result<Object, String>>(&mut cx).is_err());
    drop((empty, decoded));
    drop(cx);
    drop((field, some, none, optional, item_type));
    vm.collect_garbage().unwrap();
    assert_eq!(
        vm.gc_stats().root_registrations,
        baseline.root_registrations
    );
    assert_eq!(vm.gc_stats().objects, baseline.objects);
}

#[test]
fn enum_bindings_reject_unwitnessed_bounds_and_incompatible_payload_scopes() {
    let source = include_str!("../../../../tests/fixtures/native/enum-restricted-applications.kgr");
    let (mut vm, owner) = fixture(source, None);
    let _owner_program = owner;
    let owner = vm.select_program(&_owner_program).unwrap();
    assert!(
        owner
            .bytecode
            .enumerations
            .iter()
            .any(|layout| layout.arguments.iter().any(|ty| !ty.is_concrete()))
    );
    let conversion = ConversionContext::new(&mut vm, &owner).unwrap();
    let integer = conversion.type_for::<i32>().unwrap();
    let float = conversion.type_for::<f64>().unwrap();
    drop(conversion);
    let restricted = vm
        .bind_enum_type(&_owner_program.root(), "Restricted", &[integer])
        .unwrap();
    vm.bind_enum_variant::<()>(&restricted, "Empty").unwrap();
    assert!(
        vm.bind_enum_type(&_owner_program.root(), "Restricted", &[float])
            .is_err()
    );

    let (mut vm, old) = fixture(SOURCE, None);
    let _old_program = old;
    let old_item = vm.bind_type(&_old_program.root(), "Item", &[]).unwrap();
    let old_packet = vm
        .bind_enum_type(
            &_old_program.root(),
            "Packet",
            slice::from_ref(old_item.type_argument()),
        )
        .unwrap();
    let old_data = vm
        .bind_enum_variant::<(Object, String)>(&old_packet, "Data")
        .unwrap();
    let changed = SOURCE
        .replace("pub var value: i32", "pub var value: i64")
        .replace("item(value: i32)", "item(value: i64)")
        .replace("-> i32", "-> i64");
    let new = load_bytecode(&mut vm, "independent", compile_program(&changed, None)).unwrap();
    let _new_program = new;
    let new = vm.select_program(&_new_program).unwrap();
    let new_item = vm.bind_type(&_new_program.root(), "Item", &[]).unwrap();
    assert_eq!(new_item.type_argument().ty(), old_item.type_argument().ty());
    assert!(
        vm.bind_enum_type(
            &_old_program.root(),
            "Packet",
            slice::from_ref(new_item.type_argument())
        )
        .is_err()
    );
    vm.collect_garbage().unwrap();
    let baseline = vm.gc_stats();
    let item: Object = vm
        .execute_typed(&_new_program, "item", (42i64,), &Default::default())
        .unwrap();
    assert!(
        old_data
            .create(&mut vm.context(&new).unwrap(), (item, String::new()))
            .is_err()
    );
    vm.collect_garbage().unwrap();
    assert_eq!(
        vm.gc_stats().root_registrations,
        baseline.root_registrations
    );
    assert_eq!(vm.gc_stats().objects, baseline.objects);
}

#[test]
fn registered_variant_identities_bind_directly_and_move_with_the_runtime() {
    let mut module = ModuleBuilder::new(
        "example::events",
        &StandardDeclarations::default().catalog().unwrap(),
    )
    .unwrap();
    let mut builder = module.define_enum("Event").unwrap();
    let element = builder.type_parameter("T").unwrap();
    let data = builder.variant("Data", [element.ty()]).unwrap();
    let event = builder.finish().unwrap();
    let mut other = module.define_enum("Other").unwrap();
    let wrong = other.variant("Data", [Type::i32()]).unwrap();
    other.finish().unwrap();
    let module = module.finish().unwrap();
    let (mut vm, owner) = fixture(
        include_str!("../../../../tests/fixtures/native/enum-registered-events.kgr"),
        Some(&module),
    );
    let _owner_program = owner;
    let owner = vm.select_program(&_owner_program).unwrap();
    let element = ConversionContext::new(&mut vm, &owner)
        .unwrap()
        .type_for::<i32>()
        .unwrap();
    let applied = vm
        .bind_enum_type_declaration(
            &_owner_program.root(),
            &event.definitions().resolve(event.id()).unwrap().to_path(),
            &[element],
        )
        .unwrap();
    let member = applied
        .variant_declaration(
            &vm,
            &data.definitions().resolve(data.id()).unwrap().to_path(),
        )
        .unwrap();
    let variant = vm.bind_enum_variant_declaration::<(i32,)>(&member).unwrap();
    assert!(
        applied
            .variant_declaration(
                &vm,
                &wrong.definitions().resolve(wrong.id()).unwrap().to_path()
            )
            .is_err()
    );
    let duplicate = variant.clone();
    let answer = thread::spawn(move || {
        let mut cx = vm.context(&owner).unwrap();
        let value = duplicate.create(&mut cx, (42,)).unwrap();
        let another = variant.create(&mut cx, (43,)).unwrap();
        assert_eq!(
            cx.runtime_mut()
                .execute_typed::<_, i32>(&_owner_program, "read", (another,), &Default::default())
                .unwrap(),
            43
        );
        cx.runtime_mut()
            .execute_typed::<_, i32>(&_owner_program, "read", (value,), &Default::default())
            .unwrap()
    })
    .join()
    .unwrap();
    assert_eq!(answer, 42);
}
