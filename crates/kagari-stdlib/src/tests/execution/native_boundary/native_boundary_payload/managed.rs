use crate::{
    declarations::StandardDeclarations,
    tests::{
        execution::native_boundary::{
            compile_program, native_boundary_functions::fixture, publish_unused_version,
        },
        support::bytecode::reload_bytecode,
    },
};

#[cfg(test)]
use crate::tests::support::bytecode::load_bytecode;

use kagari_runtime::{
    error::RuntimeError,
    frame::types::arguments::TypeArgument,
    native::{
        binding::NativeResult,
        builder::ModuleBuilder,
        catalog::DeclarationCatalog,
        conversion::{FromKagari, IntoKagari, KagariType, context::ConversionContext},
        declarations::FunctionDecl,
        function_handle::Function,
        module::NativeModule,
        payload::{
            Opaque,
            managed::{Field, Managed, ManagedStorage},
        },
        typed::NativeContext,
        types::{Type, TypeRef},
        value_handle::ScriptValue,
    },
    value::view::ValueView,
};

use std::{
    panic::{AssertUnwindSafe, catch_unwind},
    slice,
};

kagari_runtime::native_data! { struct State { hp: i32 } }
type Callback = Function<(), i32>;
type Holder = Opaque<Managed<State>>;

struct Registration {
    module: NativeModule,
    ty: TypeRef,
    name: Field<String>,
    callback: Field<Callback>,
}

fn registration() -> Registration {
    let language = StandardDeclarations::default();
    let mut module = ModuleBuilder::new("example::managed", &language.catalog().unwrap()).unwrap();
    let mut storage = ManagedStorage::<State>::new();
    let name = storage
        .field::<String>(
            "name",
            String::kagari_type(&language.catalog().unwrap()).unwrap(),
        )
        .unwrap();
    assert!(storage.field::<String>("name", Type::i32()).is_err());
    let callback = storage
        .field::<Callback>("callback", Type::function([], Type::i32()).unwrap())
        .unwrap();
    let mut ty = module.define_type("Holder").unwrap();
    ty.native_storage(storage.finish()).unwrap();
    let ty = ty.finish().unwrap();
    let new = module
        .define_function(
            FunctionDecl::new("new")
                .parameter("hp", Type::i32())
                .parameter(
                    "name",
                    String::kagari_type(&language.catalog().unwrap()).unwrap(),
                )
                .parameter("callback", Type::function([], Type::i32()).unwrap())
                .returns(ty.apply([]).unwrap()),
        )
        .unwrap();
    let (name_token, callback_token) = (name.clone(), callback.clone());
    module
        .bind(
            new,
            move |cx: &mut NativeContext<'_>,
                  (hp, name, callback): (i32, String, Callback)|
                  -> NativeResult<Holder> {
                let ty = cx.result_native_type::<Managed<State>>()?;
                let name_field = ty.bind_field(cx.runtime_mut(), &name_token)?;
                let callback_field = ty.bind_field(cx.runtime_mut(), &callback_token)?;
                let mut builder = ty.build(State { hp })?;
                builder.set(cx, &name_field, name)?;
                builder.set(cx, &callback_field, callback)?;
                cx.collect_garbage()?;
                builder.finish(cx)
            },
        )
        .unwrap();
    let read = module
        .define_function(
            FunctionDecl::new("read")
                .parameter("holder", ty.apply([]).unwrap())
                .returns(Type::i32()),
        )
        .unwrap();
    module
        .bind(
            read,
            |cx: &mut NativeContext<'_>, (holder,): (Holder,)| -> NativeResult<i32> {
                cx.collect_garbage()?;
                holder.read(cx, |payload| Ok(payload.data().hp))
            },
        )
        .unwrap();
    let install = module
        .define_function(
            FunctionDecl::new("install")
                .parameter("holder", ty.apply([]).unwrap())
                .parameter("callback", Type::function([], Type::i32()).unwrap()),
        )
        .unwrap();
    let token = callback.clone();
    module
        .bind(
            install,
            move |cx: &mut NativeContext<'_>,
                  (holder, callback): (Holder, Callback)|
                  -> NativeResult<()> {
                let field = holder.native_type().bind_field(cx.runtime_mut(), &token)?;
                holder.set(cx, &field, callback)
            },
        )
        .unwrap();
    let invoke = module
        .define_function(
            FunctionDecl::new("invoke")
                .parameter("holder", ty.apply([]).unwrap())
                .returns(Type::i32()),
        )
        .unwrap();
    let token = callback.clone();
    module
        .bind(
            invoke,
            move |cx: &mut NativeContext<'_>, (holder,): (Holder,)| -> NativeResult<i32> {
                let field = holder.native_type().bind_field(cx.runtime_mut(), &token)?;
                holder.get(cx, &field)?.call(cx, ())
            },
        )
        .unwrap();
    Registration {
        module: module.finish().unwrap(),
        ty,
        name,
        callback,
    }
}

const SOURCE: &str = include_str!("../../../../../tests/fixtures/native/managed_payload.kgr");

#[test]
fn registered_payload_fields_support_aliases_reentry_and_rootless_callback_cycles() {
    let Registration {
        module,
        ty,
        name,
        callback,
    } = registration();
    let (mut vm, owner) = fixture(SOURCE, Some(&module));
    let _owner_program = owner;
    let owner = vm.select_program(&_owner_program).unwrap();
    vm.collect_garbage().unwrap();
    let baseline = vm.gc_stats();
    let object: Holder = vm
        .execute_typed(&_owner_program, "cycle", (41,), &Default::default())
        .unwrap();
    let alias = object.clone();
    let native_type = vm
        .bind_native_type::<Managed<State>>(&_owner_program.root(), &ty, &[])
        .unwrap();
    let name = native_type.bind_field(&mut vm, &name).unwrap();
    let callback = native_type.bind_field(&mut vm, &callback).unwrap();
    let mut cx = vm.context(&owner).unwrap();
    assert_eq!(object.get(&mut cx, &name).unwrap(), "initial");
    object.set(&mut cx, &name, "updated".into()).unwrap();
    assert_eq!(alias.get(&mut cx, &name.clone()).unwrap(), "updated");
    cx.collect_garbage().unwrap();
    let retained = cx.runtime().gc_stats();
    assert!(retained.objects > baseline.objects);
    let held = object.get(&mut cx, &callback).unwrap();
    object
        .edit_data(&mut cx, |state| {
            state.hp = 40;
            Ok(())
        })
        .unwrap();
    assert_eq!(
        cx.runtime_mut()
            .execute_typed::<_, i32>(
                &_owner_program,
                "run",
                (object.clone(),),
                &Default::default()
            )
            .unwrap(),
        41
    );
    assert!(
        object
            .edit_data::<()>(&mut cx, |state| {
                state.hp = 41;
                Err(RuntimeError::module_validation("after write"))
            })
            .is_err()
    );
    assert!(
        catch_unwind(AssertUnwindSafe(|| {
            let _ = object.edit_data::<()>(&mut cx, |state| {
                state.hp = 42;
                panic!("after data write")
            });
        }))
        .is_err()
    );
    assert_eq!(held.call(&mut cx, ()).unwrap(), 43);
    drop((object, alias));
    cx.collect_garbage().unwrap();
    assert_eq!(cx.runtime().gc_stats().objects, retained.objects);
    assert_eq!(
        cx.runtime().gc_stats().provider_units,
        retained.provider_units
    );
    drop((held, cx, native_type, name, callback));
    publish_unused_version(&mut vm, &_owner_program);
    vm.collect_garbage().unwrap();
    assert_eq!(vm.gc_stats().objects, baseline.objects);
    assert_eq!(
        vm.gc_stats().root_registrations,
        baseline.root_registrations
    );
}

#[test]
fn managed_construction_and_replacement_require_every_checked_field() {
    let Registration {
        module,
        ty,
        name,
        callback,
    } = registration();
    let (mut vm, owner) = fixture(SOURCE, Some(&module));
    let _owner_program = owner;
    let owner = vm.select_program(&_owner_program).unwrap();
    let native_type = vm
        .bind_native_type::<Managed<State>>(&_owner_program.root(), &ty, &[])
        .unwrap();
    let name_field = native_type.bind_field(&mut vm, &name).unwrap();
    let callback_field = native_type.bind_field(&mut vm, &callback).unwrap();
    vm.collect_garbage().unwrap();
    let baseline = vm.gc_stats();
    let mut cx = vm.context(&owner).unwrap();
    let callback: Callback = cx
        .runtime_mut()
        .execute_typed(&_owner_program, "plain", (), &Default::default())
        .unwrap();
    cx.collect_garbage().unwrap();
    let callback_live = cx.runtime().gc_stats();
    let mut incomplete = native_type.build(State { hp: 10 }).unwrap();
    incomplete
        .set(&mut cx, &callback_field, callback.clone())
        .unwrap();
    assert!(incomplete.finish(&mut cx).is_err());
    cx.collect_garbage().unwrap();
    assert_eq!(cx.runtime().gc_stats().objects, callback_live.objects);
    assert_eq!(
        cx.runtime().gc_stats().root_registrations,
        callback_live.root_registrations
    );
    let mut builder = native_type.build(State { hp: 10 }).unwrap();
    builder
        .set(&mut cx, &callback_field, callback.clone())
        .unwrap();
    builder.set(&mut cx, &name_field, "built".into()).unwrap();
    let object = builder.finish(&mut cx).unwrap();
    let alias = object.clone();
    // A completed owning constructor must retire its source temporary even while
    // this Host context remains alive. Warm metadata above, then compare liveness
    // independently of the number of physical Native representations.
    cx.collect_garbage().unwrap();
    cx.collect_garbage().unwrap();
    let retained = cx.runtime().gc_stats().objects;
    let mut discarded = native_type.build(State { hp: 0 }).unwrap();
    discarded
        .set(&mut cx, &name_field, "transient".into())
        .unwrap();
    discarded
        .set(&mut cx, &callback_field, callback.clone())
        .unwrap();
    drop(discarded.finish(&mut cx).unwrap());
    // Finish any suspended assist, then observe a fresh post-release cycle.
    cx.collect_garbage().unwrap();
    cx.collect_garbage().unwrap();
    assert_eq!(cx.runtime().gc_stats().objects, retained);
    assert!(
        native_type
            .build(State { hp: 0 })
            .unwrap()
            .replace(&mut cx, &object)
            .is_err()
    );
    assert_eq!(object.read(&mut cx, |p| Ok(p.data().hp)).unwrap(), 10);
    cx.collect_garbage().unwrap();
    let units = cx.runtime().gc_stats().provider_units;
    let mut replacement = native_type.build(State { hp: 20 }).unwrap();
    replacement
        .set(&mut cx, &name_field, "replaced".into())
        .unwrap();
    replacement.set(&mut cx, &callback_field, callback).unwrap();
    replacement.replace(&mut cx, &object).unwrap();
    cx.collect_garbage().unwrap();
    // The replacement has the same fixed schema and callback, and three more
    // UTF-8 bytes in its sole String. Old provider/backing is no longer charged.
    assert_eq!(cx.runtime().gc_stats().provider_units, units + 3);
    assert_eq!(alias.read(&mut cx, |p| Ok(p.data().hp)).unwrap(), 20);
    assert_eq!(alias.get(&mut cx, &name_field).unwrap(), "replaced");
    let other = registration();
    assert!(
        native_type
            .bind_field(cx.runtime_mut(), &other.name)
            .is_err()
    );
    let (mut foreign, foreign_owner) = fixture(SOURCE, Some(&module));
    let _foreign_owner_program = foreign_owner;
    let foreign_owner = foreign.select_program(&_foreign_owner_program).unwrap();
    let callback: Callback = foreign
        .execute_typed(&_foreign_owner_program, "plain", (), &Default::default())
        .unwrap();
    assert!(object.set(&mut cx, &callback_field, callback).is_err());
    assert_eq!(
        object
            .get(&mut cx, &callback_field)
            .unwrap()
            .call(&mut cx, ())
            .unwrap(),
        7
    );
    assert!(
        object
            .get(&mut foreign.context(&foreign_owner).unwrap(), &name_field)
            .is_err()
    );
    drop((object, alias));
    cx.collect_garbage().unwrap();
    assert_eq!(cx.runtime().gc_stats().objects, baseline.objects);
    assert_eq!(
        cx.runtime().gc_stats().root_registrations,
        baseline.root_registrations
    );
}

#[test]
fn stored_callback_keeps_old_code_and_releases_the_old_program_with_its_cycle() {
    let Registration {
        module, callback, ..
    } = registration();
    let (mut vm, old) = fixture(SOURCE, Some(&module));
    let _old_program = old;
    let old = vm.select_program(&_old_program).unwrap();
    vm.collect_garbage().unwrap();
    let baseline = vm.gc_stats();
    let object: Holder = vm
        .execute_typed(&_old_program, "cycle", (41,), &Default::default())
        .unwrap();
    let field = object.native_type().bind_field(&mut vm, &callback).unwrap();
    reload_bytecode(
        &mut vm,
        &_old_program,
        Some("functions".into()),
        compile_program(&SOURCE.replace("+ 1", "+ 2"), Some(&module)),
    )
    .unwrap();
    let _new_program = _old_program.clone();
    let new = vm.select_program(&_new_program).unwrap();
    let mut cx = vm.context(&new).unwrap();
    assert_eq!(
        object
            .get(&mut cx, &field)
            .unwrap()
            .call(&mut cx, ())
            .unwrap(),
        42
    );
    let replacement: Holder = cx
        .runtime_mut()
        .execute_typed(&_new_program, "cycle", (41,), &Default::default())
        .unwrap();
    assert_eq!(
        replacement
            .get(&mut cx, &field)
            .unwrap()
            .call(&mut cx, ())
            .unwrap(),
        43
    );
    cx.collect_garbage().unwrap();
    cx.runtime_mut().validate_executable_module(&old).unwrap();
    drop((object, field));
    cx.collect_garbage().unwrap();
    assert!(cx.runtime_mut().validate_executable_module(&old).is_err());
    let current_field = replacement
        .native_type()
        .bind_field(cx.runtime_mut(), &callback)
        .unwrap();
    assert_eq!(
        replacement
            .get(&mut cx, &current_field)
            .unwrap()
            .call(&mut cx, ())
            .unwrap(),
        43
    );
    drop(current_field);
    drop((replacement, cx));
    publish_unused_version(&mut vm, &_new_program);
    vm.collect_garbage().unwrap();
    assert_eq!(vm.gc_stats().objects, baseline.objects);
    assert_eq!(
        vm.gc_stats().root_registrations,
        baseline.root_registrations
    );
}

struct Converted {
    text: String,
    fail: bool,
}

impl FromKagari for Converted {
    fn from_kagari(
        cx: &mut ConversionContext<'_>,
        expected: &TypeArgument,
        value: &ValueView<'_>,
    ) -> NativeResult<Self> {
        String::from_kagari(cx, expected, value).map(|text| Self { text, fail: false })
    }
}

impl KagariType for Converted {
    fn kagari_type(catalog: &DeclarationCatalog) -> NativeResult<Type> {
        String::kagari_type(catalog)
    }
}

impl IntoKagari for Converted {
    fn into_kagari<'call>(
        self,
        cx: &mut ConversionContext<'call>,
        expected: &TypeArgument,
    ) -> NativeResult<ValueView<'call>> {
        cx.runtime_mut().collect_garbage()?;
        if self.fail {
            return Err(RuntimeError::module_validation(
                "conversion failure after GC",
            ));
        }
        self.text.into_kagari(cx, expected)
    }
}

#[test]
fn failed_converters_keep_prior_initializers_and_committed_fields_rooted() {
    let language = StandardDeclarations::default();
    let mut module = ModuleBuilder::new("example::managed", &language.catalog().unwrap()).unwrap();
    let mut storage = ManagedStorage::<()>::new();
    let callback_token = storage
        .field::<Callback>("callback", Type::function([], Type::i32()).unwrap())
        .unwrap();
    let text_token = storage
        .field::<Converted>(
            "text",
            String::kagari_type(&language.catalog().unwrap()).unwrap(),
        )
        .unwrap();
    let mut ty = module.define_type("Holder").unwrap();
    ty.native_storage(storage.finish()).unwrap();
    let ty = ty.finish().unwrap();
    let mut wrong_storage = ManagedStorage::<()>::new();
    let wrong_token = wrong_storage
        .field::<i32>("wrong", Type::function([], Type::i32()).unwrap())
        .unwrap();
    let mut wrong_ty = module.define_type("Wrong").unwrap();
    wrong_ty.native_storage(wrong_storage.finish()).unwrap();
    let wrong_ty = wrong_ty.finish().unwrap();
    let (mut vm, owner) = fixture(
        include_str!("../../../../../tests/fixtures/native/managed-converter-callback.kgr"),
        Some(&module.finish().unwrap()),
    );
    let _owner_program = owner;
    let owner = vm.select_program(&_owner_program).unwrap();
    let native_type = vm
        .bind_native_type::<Managed<()>>(&_owner_program.root(), &ty, &[])
        .unwrap();
    assert!(
        vm.bind_native_type::<Managed<()>>(&_owner_program.root(), &wrong_ty, &[])
            .unwrap()
            .bind_field(&mut vm, &wrong_token)
            .is_err()
    );
    let callback_field = native_type.bind_field(&mut vm, &callback_token).unwrap();
    let text_field = native_type.bind_field(&mut vm, &text_token).unwrap();
    vm.collect_garbage().unwrap();
    let baseline = vm.gc_stats();
    let mut cx = vm.context(&owner).unwrap();
    let callback: Callback = cx
        .runtime_mut()
        .execute_typed(&_owner_program, "callback", (), &Default::default())
        .unwrap();
    cx.collect_garbage().unwrap();
    let callback_live = cx.runtime().gc_stats();
    let mut builder = native_type.build(()).unwrap();
    builder.set(&mut cx, &callback_field, callback).unwrap();
    assert!(
        builder
            .set(
                &mut cx,
                &text_field,
                Converted {
                    text: String::new(),
                    fail: true
                }
            )
            .is_err()
    );
    cx.collect_garbage().unwrap();
    assert_eq!(cx.runtime().gc_stats().objects, callback_live.objects);
    builder
        .set(
            &mut cx,
            &text_field,
            Converted {
                text: "initial".into(),
                fail: false,
            },
        )
        .unwrap();
    assert!(
        builder
            .set(
                &mut cx,
                &text_field,
                Converted {
                    text: "failed".into(),
                    fail: true
                }
            )
            .is_err()
    );
    let object = builder.finish(&mut cx).unwrap();
    assert_eq!(object.get(&mut cx, &text_field).unwrap().text, "initial");
    assert!(
        object
            .set(
                &mut cx,
                &text_field,
                Converted {
                    text: "failed".into(),
                    fail: true
                }
            )
            .is_err()
    );
    assert_eq!(object.get(&mut cx, &text_field).unwrap().text, "initial");
    assert_eq!(
        object
            .get(&mut cx, &callback_field)
            .unwrap()
            .call(&mut cx, ())
            .unwrap(),
        42
    );
    let mut replacement = native_type.build(()).unwrap();
    assert!(
        replacement
            .set(
                &mut cx,
                &text_field,
                Converted {
                    text: "failed".into(),
                    fail: true
                }
            )
            .is_err()
    );
    assert!(replacement.replace(&mut cx, &object).is_err());
    assert_eq!(object.get(&mut cx, &text_field).unwrap().text, "initial");
    drop(object);
    cx.collect_garbage().unwrap();
    assert_eq!(cx.runtime().gc_stats().objects, baseline.objects);
    assert_eq!(
        cx.runtime().gc_stats().root_registrations,
        baseline.root_registrations
    );
}

#[test]
fn generic_managed_fields_keep_their_argument_scope_across_native_calls_and_bindings() {
    type Boxed = Opaque<Managed<()>>;
    let mut module = ModuleBuilder::new(
        "example::managed",
        &StandardDeclarations::default().catalog().unwrap(),
    )
    .unwrap();
    let mut ty = module.define_type("Boxed").unwrap();
    let parameter = ty.type_parameter("T").unwrap();
    let mut storage = ManagedStorage::<()>::new();
    let value_token = storage
        .field::<ScriptValue>("value", parameter.ty())
        .unwrap();
    ty.native_storage(storage.finish()).unwrap();
    let ty = ty.finish().unwrap();
    let wrap = module.define_function(FunctionDecl::new("wrap")).unwrap();
    module
        .function(&wrap, |f| {
            let parameter = f.type_parameter("T")?.ty();
            f.parameter("value", parameter.clone())?;
            f.returns(ty.apply([parameter])?)?;
            Ok(())
        })
        .unwrap();
    let token = value_token.clone();
    module
        .bind(
            wrap,
            move |cx: &mut NativeContext<'_>, (value,): (ScriptValue,)| -> NativeResult<Boxed> {
                let ty = cx.result_native_type::<Managed<()>>()?;
                let field = ty.bind_field(cx.runtime_mut(), &token)?;
                let mut builder = ty.build(())?;
                builder.set(cx, &field, value)?;
                builder.finish(cx)
            },
        )
        .unwrap();
    let read = module.define_function(FunctionDecl::new("read")).unwrap();
    module
        .function(&read, |f| {
            let parameter = f.type_parameter("T")?.ty();
            f.parameter("value", ty.apply([parameter.clone()])?)?;
            f.returns(parameter)?;
            Ok(())
        })
        .unwrap();
    let token = value_token.clone();
    module
        .bind(
            read,
            move |cx: &mut NativeContext<'_>, (value,): (Boxed,)| -> NativeResult<ScriptValue> {
                let field = value.native_type().bind_field(cx.runtime_mut(), &token)?;
                cx.collect_garbage()?;
                value.get(cx, &field)
            },
        )
        .unwrap();
    let module = module.finish().unwrap();
    let source = include_str!("../../../../../tests/fixtures/native/generic-managed-field.kgr");
    let (mut vm, old) = fixture(source, Some(&module));
    let _old_program = old;
    let _old = vm.select_program(&_old_program).unwrap();
    vm.collect_garbage().unwrap();
    let baseline = vm.gc_stats();
    let old_value: Boxed = vm
        .execute_typed(&_old_program, "make", (), &Default::default())
        .unwrap();
    assert_eq!(
        vm.execute_typed::<_, i32>(
            &_old_program,
            "run",
            (old_value.clone(),),
            &Default::default()
        )
        .unwrap(),
        42
    );
    vm.collect_garbage().unwrap();
    let before_installation = vm.gc_stats().objects;
    let new = load_bytecode(
        &mut vm,
        "independent",
        compile_program(&source.replace("i32", "i64"), Some(&module)),
    )
    .unwrap();
    let _new_program = new;
    let new = vm.select_program(&_new_program).unwrap();
    vm.collect_garbage().unwrap();
    let publication_objects = vm.gc_stats().objects - before_installation;
    let new_value: Boxed = vm
        .execute_typed(&_new_program, "make", (), &Default::default())
        .unwrap();
    assert_eq!(
        vm.execute_typed::<_, i64>(
            &_new_program,
            "run",
            (new_value.clone(),),
            &Default::default()
        )
        .unwrap(),
        42
    );
    let old_field = old_value
        .native_type()
        .bind_field(&mut vm, &value_token)
        .unwrap();
    let new_field = new_value
        .native_type()
        .bind_field(&mut vm, &value_token)
        .unwrap();
    let mut cx = vm.context(&new).unwrap();
    assert!(new_value.get(&mut cx, &old_field).is_err());
    assert!(old_value.get(&mut cx, &new_field).is_err());
    let newer_item = new_value.get(&mut cx, &new_field).unwrap();
    assert!(
        old_value
            .set(&mut cx, &old_field, newer_item.clone())
            .is_err()
    );
    assert!(
        cx.runtime_mut()
            .execute_typed::<_, i64>(
                &_new_program,
                "run",
                (old_value.clone(),),
                &Default::default()
            )
            .is_err()
    );
    let new_item = cx
        .runtime_mut()
        .bind_type(&_new_program.root(), "Item", &[])
        .unwrap();
    assert!(
        cx.runtime_mut()
            .bind_native_type::<Managed<()>>(
                &_old_program.root(),
                &ty,
                slice::from_ref(new_item.type_argument())
            )
            .is_err()
    );
    let scoped_type = cx
        .runtime_mut()
        .bind_native_type::<Managed<()>>(
            &_new_program.root(),
            &ty,
            slice::from_ref(new_item.type_argument()),
        )
        .unwrap();
    let scoped_field = scoped_type
        .bind_field(cx.runtime_mut(), &value_token)
        .unwrap();
    let mut builder = scoped_type.build(()).unwrap();
    builder.set(&mut cx, &scoped_field, newer_item).unwrap();
    let scoped = builder.finish(&mut cx).unwrap();
    assert_eq!(
        cx.runtime_mut()
            .execute_typed::<_, i64>(&_new_program, "run", (scoped.clone(),), &Default::default())
            .unwrap(),
        42
    );
    let mut wrong_replacement = scoped_type.build(()).unwrap();
    let value = scoped.get(&mut cx, &scoped_field).unwrap();
    wrong_replacement
        .set(&mut cx, &scoped_field, value)
        .unwrap();
    assert!(wrong_replacement.replace(&mut cx, &old_value).is_err());
    assert_eq!(
        cx.runtime_mut()
            .execute_typed::<_, i32>(
                &_old_program,
                "run",
                (old_value.clone(),),
                &Default::default()
            )
            .unwrap(),
        42
    );
    drop((scoped, old_value, new_value));
    cx.collect_garbage().unwrap();
    assert_eq!(
        cx.runtime().gc_stats().objects,
        baseline.objects + publication_objects
    );
}
