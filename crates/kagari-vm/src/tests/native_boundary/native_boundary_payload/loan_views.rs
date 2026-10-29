//! Safe NativeData callbacks cannot retain directory views across loan restoration.
use crate::tests::native_boundary::native_boundary_functions::fixture;
use kagari_runtime::{
    native::{
        binding::NativeResult, builder::ModuleBuilder, conversion::context::ConversionContext,
        declarations::FunctionDecl, function_handle::Function, module::NativeModule,
        payload::Opaque, storage::NativeStorage, typed::NativeContext,
    },
    value::Value,
};
use kagari_stdlib::declarations::StandardDeclarations;
use std::panic::{AssertUnwindSafe, catch_unwind};

kagari_runtime::native_data! {
    struct State { value: i32 }
}

fn module() -> NativeModule {
    let mut module = ModuleBuilder::new(
        "example::loan_views",
        &StandardDeclarations::default().catalog().unwrap(),
    )
    .unwrap();
    let mut declaration = module.define_type("State").unwrap();
    declaration
        .native_storage(NativeStorage::data::<State>())
        .unwrap();
    let state = declaration.finish().unwrap();
    let make = module
        .define_function(FunctionDecl::new("make").returns(state.apply([]).unwrap()))
        .unwrap();
    module
        .bind(
            make,
            |cx: &mut NativeContext<'_>, (): ()| -> NativeResult<Opaque<State>> {
                cx.result_native_type()?
                    .create(cx, State { value: 42 }, &[])
            },
        )
        .unwrap();
    module.finish().unwrap()
}

const SOURCE: &str = r#"
    use example::loan_views::{State, make};
    pub fn state() -> State { make() }
    pub fn callback() -> fn() -> i32 { || 42 }
"#;

#[test]
fn native_data_returned_directory_views_are_rejected_before_restoration() {
    let (vm, program) = fixture(SOURCE, Some(&module()));
    let owner = vm.runtime().select_program(&program).unwrap();
    let runtime = vm.runtime();
    let heap = runtime.gc();
    let cx = vm.context(&owner).unwrap();
    let state: Opaque<State> = vm.execute_typed(&owner, "state", ()).unwrap();
    let mut conversion = ConversionContext::new(runtime, &owner).unwrap();
    let text = conversion.encode("unrelated".to_owned()).unwrap();
    let tuple = conversion.encode((42_i32,)).unwrap();
    let enumeration = conversion.encode(Some(42_i32)).unwrap();
    let Value::Str(text_id) = unsafe { text.value_transport(heap) }.unwrap() else {
        panic!("string")
    };
    let Value::Tuple(tuple_id) = unsafe { tuple.value_transport(heap) }.unwrap() else {
        panic!("tuple")
    };
    let Value::Enum(enum_id) = unsafe { enumeration.value_transport(heap) }.unwrap() else {
        panic!("enum")
    };
    let callback: Function<(), i32> = vm.execute_typed(&owner, "callback", ()).unwrap();
    let callback_value =
        unsafe { callback.closure_value().unwrap().value_transport(heap) }.unwrap();
    let before = heap.stats();
    let returned = state
        .read_data(&cx, |_| {
            Ok((
                unsafe { heap.string_transport(text_id) },
                unsafe { heap.tuple_transport(tuple_id) },
                unsafe { heap.enum_view_transport(enum_id) },
                unsafe { runtime.resolve_closure_transport(&callback_value) }.ok(),
            ))
        })
        .unwrap();
    assert!(
        returned.0.is_none()
            && returned.1.is_none()
            && returned.2.is_none()
            && returned.3.is_none()
    );
    assert_eq!(heap.stats(), before);
    assert_eq!(state.read_data(&cx, |state| Ok(state.value)).unwrap(), 42);
    assert_eq!(
        &*unsafe { heap.string_transport(text_id) }.unwrap(),
        "unrelated"
    );
    assert_eq!(
        &*unsafe { heap.tuple_transport(tuple_id) }.unwrap(),
        &[Value::I32(42)]
    );
    assert!(unsafe { heap.enum_view_transport(enum_id) }.is_some());
    assert!(unsafe { runtime.resolve_closure_transport(&callback_value) }.is_ok());
    let existing_view = unsafe { heap.string_transport(text_id) }.unwrap();
    let mut entered = false;
    assert!(
        state
            .read_data(&cx, |_| {
                entered = true;
                Ok(())
            })
            .is_err()
    );
    assert!(
        !entered,
        "an existing view must reject the loan before payload removal"
    );
    drop(existing_view);
    assert_eq!(state.read_data(&cx, |state| Ok(state.value)).unwrap(), 42);
    runtime.collect_garbage().unwrap();
}

#[test]
fn native_data_saved_directory_view_cannot_break_unwind_restoration() {
    let (vm, program) = fixture(SOURCE, Some(&module()));
    let owner = vm.runtime().select_program(&program).unwrap();
    let runtime = vm.runtime();
    let heap = runtime.gc();
    let cx = vm.context(&owner).unwrap();
    let state: Opaque<State> = vm.execute_typed(&owner, "state", ()).unwrap();
    let text = ConversionContext::new(runtime, &owner)
        .unwrap()
        .encode("unrelated".to_owned())
        .unwrap();
    let Value::Str(id) = unsafe { text.value_transport(heap) }.unwrap() else {
        panic!("string")
    };
    let before = heap.stats();
    for unwind in [true, false] {
        let mut saved = None;
        let result = catch_unwind(AssertUnwindSafe(|| {
            state.read_data(&cx, |_| -> NativeResult<()> {
                saved = unsafe { heap.string_transport(id) };
                if unwind {
                    panic!("native data callback unwind")
                }
                Ok(())
            })
        }));
        assert!(saved.is_none());
        if unwind {
            assert!(result.is_err());
        } else {
            result.unwrap().unwrap();
        }
        assert_eq!(heap.stats(), before);
        assert_eq!(state.read_data(&cx, |state| Ok(state.value)).unwrap(), 42);
        state
            .edit_data(&cx, |state| {
                state.value = 42;
                Ok(())
            })
            .unwrap();
    }
    runtime.collect_garbage().unwrap();
}
