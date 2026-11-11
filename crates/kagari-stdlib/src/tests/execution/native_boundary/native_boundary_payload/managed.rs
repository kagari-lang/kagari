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
    value::Value,
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
                let name_field = ty.bind_field(cx.runtime(), &name_token)?;
                let callback_field = ty.bind_field(cx.runtime(), &callback_token)?;
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
                let field = holder.native_type().bind_field(cx.runtime(), &token)?;
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
                let field = holder.native_type().bind_field(cx.runtime(), &token)?;
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

const SOURCE: &str = r#"
    use example::managed::{Holder, new, read, install, invoke};
    pub fn make(hp: i32) -> Holder { new(hp, "initial", || 42) }
    pub fn cycle(hp: i32) -> Holder {
        val holder = make(hp);
        install(holder, || read(holder) + 1);
        holder
    }
    pub fn plain() -> fn() -> i32 { || 7 }
    pub fn run(holder: Holder) -> i32 { invoke(holder) }
"#;

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
    let object: Holder = vm
        .execute_typed(&_owner_program, "cycle", (41,), &Default::default())
        .unwrap();
    let alias = object.clone();
    let native_type = vm
        .bind_native_type::<Managed<State>>(&_owner_program.root(), &ty, &[])
        .unwrap();
    let name = native_type.bind_field(&vm, &name).unwrap();
    let callback = native_type.bind_field(&vm, &callback).unwrap();
    let mut cx = vm.context(&owner).unwrap();
    assert_eq!(object.get(&mut cx, &name).unwrap(), "initial");
    object.set(&mut cx, &name, "updated".into()).unwrap();
    assert_eq!(alias.get(&mut cx, &name.clone()).unwrap(), "updated");
    // Holder/callback, the updated name, and the module string constant.
    assert_eq!(cx.collect_garbage().unwrap().live_objects, 4);
    let held = object.get(&mut cx, &callback).unwrap();
    object
        .edit_data(&cx, |state| {
            state.hp = 40;
            assert!(cx.collect_garbage().is_err());
            Ok(())
        })
        .unwrap();
    assert_eq!(
        vm.execute_typed::<_, i32>(
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
            .edit_data::<()>(&cx, |state| {
                state.hp = 41;
                Err(RuntimeError::module_validation("after write"))
            })
            .is_err()
    );
    assert!(
        catch_unwind(AssertUnwindSafe(|| {
            let _ = object.edit_data::<()>(&cx, |state| {
                state.hp = 42;
                panic!("after data write")
            });
        }))
        .is_err()
    );
    assert_eq!(held.call(&mut cx, ()).unwrap(), 43);
    drop((object, alias));
    // Holder/callback, the updated name, and the module string constant.
    assert_eq!(cx.collect_garbage().unwrap().live_objects, 4);
    drop((held, cx, native_type, name, callback));
    publish_unused_version(&mut vm, &_owner_program);
    assert_eq!(vm.collect_garbage().unwrap().live_objects, 0);
    assert_eq!(vm.gc().active_roots(), 0);
}

#[test]
fn managed_construction_and_replacement_require_every_checked_field() {
    let Registration {
        module,
        ty,
        name,
        callback,
    } = registration();
    let (vm, owner) = fixture(SOURCE, Some(&module));
    let _owner_program = owner;
    let owner = vm.select_program(&_owner_program).unwrap();
    let native_type = vm
        .bind_native_type::<Managed<State>>(&_owner_program.root(), &ty, &[])
        .unwrap();
    let name_field = native_type.bind_field(&vm, &name).unwrap();
    let callback_field = native_type.bind_field(&vm, &callback).unwrap();
    let mut cx = vm.context(&owner).unwrap();
    let callback: Callback = vm
        .execute_typed(&_owner_program, "plain", (), &Default::default())
        .unwrap();
    let mut incomplete = native_type.build(State { hp: 10 }).unwrap();
    incomplete
        .set(&mut cx, &callback_field, callback.clone())
        .unwrap();
    assert!(incomplete.finish(&mut cx).is_err());
    assert_eq!(cx.collect_garbage().unwrap().live_objects, 1);
    let mut builder = native_type.build(State { hp: 10 }).unwrap();
    builder
        .set(&mut cx, &callback_field, callback.clone())
        .unwrap();
    builder.set(&mut cx, &name_field, "built".into()).unwrap();
    let object = builder.finish(&mut cx).unwrap();
    let alias = object.clone();
    assert!(
        native_type
            .build(State { hp: 0 })
            .unwrap()
            .replace(&mut cx, &object)
            .is_err()
    );
    assert_eq!(object.read(&cx, |p| Ok(p.data().hp)).unwrap(), 10);
    let mut replacement = native_type.build(State { hp: 20 }).unwrap();
    replacement
        .set(&mut cx, &name_field, "replaced".into())
        .unwrap();
    replacement.set(&mut cx, &callback_field, callback).unwrap();
    let units = vm.gc().stats().current_heap_units;
    replacement.replace(&mut cx, &object).unwrap();
    assert_eq!(vm.gc().stats().current_heap_units, units);
    assert_eq!(alias.read(&cx, |p| Ok(p.data().hp)).unwrap(), 20);
    assert_eq!(alias.get(&mut cx, &name_field).unwrap(), "replaced");
    let other = registration();
    assert!(native_type.bind_field(&vm, &other.name).is_err());
    let (foreign, foreign_owner) = fixture(SOURCE, Some(&module));
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
    assert_eq!(cx.collect_garbage().unwrap().live_objects, 0);
}

#[test]
fn stored_callback_keeps_old_code_and_releases_the_old_program_with_its_cycle() {
    let Registration {
        module, callback, ..
    } = registration();
    let (mut vm, old) = fixture(SOURCE, Some(&module));
    let _old_program = old;
    let old = vm.select_program(&_old_program).unwrap();
    let object: Holder = vm
        .execute_typed(&_old_program, "cycle", (41,), &Default::default())
        .unwrap();
    let field = object.native_type().bind_field(&vm, &callback).unwrap();
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
    let replacement: Holder = vm
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
    assert!(
        !cx.collect_garbage()
            .unwrap()
            .reclaimed_modules
            .contains(&old.key())
    );
    drop((object, field));
    let collected = cx.collect_garbage().unwrap();
    assert!(collected.reclaimed_modules.contains(&old.key()));
    // Current holder/callback, its name, and its module string constant.
    assert_eq!(collected.live_objects, 4);
    drop((replacement, cx));
    publish_unused_version(&mut vm, &_new_program);
    assert_eq!(vm.collect_garbage().unwrap().live_objects, 0);
}

struct Converted {
    text: String,
    fail: bool,
}

impl FromKagari for Converted {
    fn from_kagari(
        cx: &mut ConversionContext<'_>,
        expected: &TypeArgument,
        value: &Value,
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
    fn into_kagari(
        self,
        cx: &mut ConversionContext<'_>,
        expected: &TypeArgument,
    ) -> NativeResult<Value> {
        cx.runtime().collect_garbage()?;
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
    let (vm, owner) = fixture(
        "pub fn callback() -> fn() -> i32 { || 42 }",
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
            .bind_field(&vm, &wrong_token)
            .is_err()
    );
    let callback_field = native_type.bind_field(&vm, &callback_token).unwrap();
    let text_field = native_type.bind_field(&vm, &text_token).unwrap();
    let mut cx = vm.context(&owner).unwrap();
    let callback: Callback = vm
        .execute_typed(&_owner_program, "callback", (), &Default::default())
        .unwrap();
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
    assert_eq!(cx.collect_garbage().unwrap().live_objects, 1);
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
    assert_eq!(cx.collect_garbage().unwrap().live_objects, 0);
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
                let field = ty.bind_field(cx.runtime(), &token)?;
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
                let field = value.native_type().bind_field(cx.runtime(), &token)?;
                cx.collect_garbage()?;
                value.get(cx, &field)
            },
        )
        .unwrap();
    let module = module.finish().unwrap();
    let source = r#"
        use example::managed::{Boxed, wrap, read};
        pub struct Item { pub var value: i32 }
        pub fn item() -> Item { Item { value: 42 } }
        pub fn make() -> Boxed<Item> { wrap(item()) }
        pub fn run(value: Boxed<Item>) -> i32 { read(value).value }
    "#;
    let (mut vm, old) = fixture(source, Some(&module));
    let _old_program = old;
    let _old = vm.select_program(&_old_program).unwrap();
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
    let new = load_bytecode(
        &mut vm,
        "independent",
        compile_program(&source.replace("i32", "i64"), Some(&module)),
    )
    .unwrap();
    let _new_program = new;
    let new = vm.select_program(&_new_program).unwrap();
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
        .bind_field(&vm, &value_token)
        .unwrap();
    let new_field = new_value
        .native_type()
        .bind_field(&vm, &value_token)
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
        vm.execute_typed::<_, i64>(
            &_new_program,
            "run",
            (old_value.clone(),),
            &Default::default()
        )
        .is_err()
    );
    let new_item = vm.bind_type(&_new_program.root(), "Item", &[]).unwrap();
    assert!(
        vm.bind_native_type::<Managed<()>>(
            &_old_program.root(),
            &ty,
            slice::from_ref(new_item.type_argument())
        )
        .is_err()
    );
    let scoped_type = vm
        .bind_native_type::<Managed<()>>(
            &_new_program.root(),
            &ty,
            slice::from_ref(new_item.type_argument()),
        )
        .unwrap();
    let scoped_field = scoped_type.bind_field(&vm, &value_token).unwrap();
    let mut builder = scoped_type.build(()).unwrap();
    builder.set(&mut cx, &scoped_field, newer_item).unwrap();
    let scoped = builder.finish(&mut cx).unwrap();
    assert_eq!(
        vm.execute_typed::<_, i64>(&_new_program, "run", (scoped.clone(),), &Default::default())
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
        vm.execute_typed::<_, i32>(
            &_old_program,
            "run",
            (old_value.clone(),),
            &Default::default()
        )
        .unwrap(),
        42
    );
    drop((scoped, old_value, new_value));
    assert_eq!(cx.collect_garbage().unwrap().live_objects, 0);
}
