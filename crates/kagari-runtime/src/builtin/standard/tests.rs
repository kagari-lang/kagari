use crate::{
    Runtime, builtin,
    error::{RuntimeError, trap::TrapReason},
    module::ExecutableModule,
    native::{binding::NativeResult, storage_type::StorageType},
    test_support::layouts::allocation_owner,
    value::{
        arrays::{ArrayObject, storage::ArrayValues},
        carriers::{RootedValue, ScopedValue},
        strings::StringObject,
    },
};
use kagari_contract::standard::RuntimePrimitive;
use kagari_gc::error::GcError;
use kagari_types::{scalar::BuiltinType, ty::Ty};
use std::sync::Arc;

fn text(runtime: &mut Runtime, value: &str) -> RootedValue {
    runtime
        .memory
        .scope(&runtime.resources, |cx, _| {
            let object = StringObject::alloc(cx, value.into())?;
            ScopedValue::managed(object).retain(cx)
        })
        .unwrap()
}

fn array(
    runtime: &mut Runtime,
    owner: &ExecutableModule,
    ty: Ty,
    values: &[RootedValue],
) -> RootedValue {
    let contract = Arc::new(StorageType::prepare(ty.clone(), owner).unwrap());
    runtime
        .memory
        .scope(&runtime.resources, |cx, _| {
            let mut data = ArrayValues::empty(&ty);
            for value in values {
                data.push(cx, value.get(cx)?)?;
            }
            let object = ArrayObject::alloc(cx, contract, data, Default::default())?;
            ScopedValue::managed(object).retain(cx)
        })
        .unwrap()
}

fn join(
    runtime: &mut Runtime,
    owner: &ExecutableModule,
    array: &RootedValue,
    separator: &RootedValue,
) -> NativeResult<RootedValue> {
    runtime.with_host_scope(|scope| {
        let arguments = [scope.import(array)?, scope.import(separator)?];
        let value = scope.execute(&arguments, |runtime, arguments, destination| {
            builtin::invoke_standard(
                runtime,
                owner,
                RuntimePrimitive::StringPartsJoin,
                arguments,
                destination,
            )
            .map_err(|error| error.into_runtime_error())
        })?;
        scope.retain(&value)
    })
}

fn strings(runtime: &mut Runtime, array: &RootedValue) -> Vec<String> {
    runtime
        .memory
        .scope(&runtime.resources, |cx, _| {
            let array = cx.downcast::<ArrayObject>(array.get(cx)?.object().unwrap())?;
            (0..cx.read(array, ArrayObject::len))
                .map(|index| {
                    let value = ArrayObject::get(cx, array, index)?;
                    let text = cx.downcast::<StringObject>(value.object().unwrap())?;
                    Ok(cx.read(text, |text| text.as_str().to_owned()))
                })
                .collect::<Result<Vec<_>, GcError>>()
        })
        .unwrap()
}

#[test]
fn join_validates_native_arguments_and_leaves_the_array_unchanged() {
    let mut runtime = Runtime::default();
    let owner = allocation_owner(&mut runtime);
    let first = text(&mut runtime, "é");
    let second = text(&mut runtime, "😀");
    let handle = array(
        &mut runtime,
        &owner,
        Ty::Builtin(BuiltinType::String),
        &[first, second],
    );
    let before = strings(&mut runtime, &handle);
    let separator = text(&mut runtime, "/");
    let joined = join(&mut runtime, &owner, &handle, &separator).unwrap();
    runtime
        .memory
        .scope(&runtime.resources, |cx, _| {
            let joined = cx.downcast::<StringObject>(joined.get(cx)?.object().unwrap())?;
            assert_eq!(cx.read(joined, |text| text.as_str().to_owned()), "é/😀");
            Ok::<_, GcError>(())
        })
        .unwrap();
    assert_eq!(strings(&mut runtime, &handle), before);
    let empty = text(&mut runtime, "");
    let error = join(&mut runtime, &owner, &RootedValue::from(1_i32), &empty).unwrap_err();
    assert_eq!(
        error.reason(),
        RuntimeError::from(TrapReason::ArrayJoinExpectsAStringArrayAndSeparator).reason()
    );
    let invalid = array(
        &mut runtime,
        &owner,
        Ty::Builtin(BuiltinType::I32),
        &[RootedValue::from(1_i32)],
    );
    let error = join(&mut runtime, &owner, &invalid, &empty).unwrap_err();
    assert_eq!(
        error.reason(),
        RuntimeError::from(TrapReason::StringInterpolationRequiresStringStorage).reason()
    );
    runtime
        .memory
        .scope(&runtime.resources, |cx, _| {
            let array = cx.downcast::<ArrayObject>(invalid.get(cx)?.object().unwrap())?;
            assert_eq!(cx.read(array, ArrayObject::len), 1);
            assert_eq!(
                ArrayObject::get(cx, array, 0)?.scalar_payload().unwrap().1,
                1
            );
            Ok::<_, GcError>(())
        })
        .unwrap();
}
