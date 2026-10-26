//! Safe native facades must not bypass a receiver's central operation protection.
use crate::tests::native_boundary::native_boundary_functions::fixture;
use kagari_contract::operations::IterOp;
use kagari_runtime::{
    native::{
        binding::NativeResult, builder::ModuleBuilder, context::CallContext,
        declarations::FunctionDecl, types::Type, views::ValueHandle,
    },
    value::Value,
};
use kagari_stdlib::declarations::StandardDeclarations;

#[test]
fn safe_sequence_and_iterator_facades_respect_exclusive_operations() {
    let language = StandardDeclarations::default();
    let mut module =
        ModuleBuilder::new("example::guarded_facades", &language.catalog().unwrap()).unwrap();
    let sequence = module
        .define_function(
            FunctionDecl::new("sequence").parameter("values", language.vec(Type::i32()).unwrap()),
        )
        .unwrap();
    module
        .bind(
            sequence,
            |cx: &mut CallContext<'_>, values: ValueHandle<'_>| -> NativeResult<Value> {
                let Value::GcHandle(id) = values.value() else {
                    panic!("sequence")
                };
                let guard = values.try_enter_operation()?.unwrap();
                assert!(cx.heap().validate_value(&values.value()));
                assert_eq!(cx.heap().sequence_len(id), None);
                assert_eq!(cx.heap().sequence_get(id, 0), None);
                assert_eq!(cx.heap().sequence_snapshot(id), None);
                assert!(cx.heap().sequence_set(id, 0, Value::I32(99)).is_err());
                drop(guard);
                assert_eq!(
                    cx.heap().sequence_snapshot(id),
                    Some(vec![Value::I32(10), Value::I32(20)])
                );
                Ok(Value::Unit)
            },
        )
        .unwrap();
    let iterator = module
        .define_function(
            FunctionDecl::new("iterator")
                .parameter("cursor", language.collection_cursor(Type::i32()).unwrap())
                .returns(language.option(Type::i32()).unwrap()),
        )
        .unwrap();
    let option = StandardDeclarations::enumeration("Option").unwrap();
    let some = option.variant("Some").unwrap();
    let none = option.variant("None").unwrap();
    module
        .bind(
            iterator,
            move |cx: &mut CallContext<'_>, cursor: ValueHandle<'_>| -> NativeResult<Value> {
                let guard = cursor.try_enter_operation()?.unwrap();
                assert!(cx.iter_operation(0, IterOp::Next).is_err());
                assert!(cx.iterator_next_result(0, &some, &none).is_err());
                drop(guard);
                cx.iterator_next_result(0, &some, &none)
            },
        )
        .unwrap();
    let (vm, program) = fixture(
        r#"
        use example::guarded_facades::{sequence, iterator};
        pub fn run() -> (Option<i32>, Option<i32>) {
            val values = Vec::from([10, 20]);
            sequence(values);
            val cursor = values.iter();
            (iterator(cursor), cursor.next())
        }
    "#,
        Some(&module.finish().unwrap()),
    );
    let owner = vm.runtime().select_program(&program).unwrap();
    assert_eq!(
        vm.execute_typed::<_, (Option<i32>, Option<i32>)>(&owner, "run", ())
            .unwrap(),
        (Some(10), Some(20))
    );
}
