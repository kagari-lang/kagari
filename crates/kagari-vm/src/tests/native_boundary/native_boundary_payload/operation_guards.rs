//! Safe native facades must not bypass a receiver's central operation protection.
use crate::tests::native_boundary::native_boundary_functions::fixture;
use kagari_common::identity::DefinitionKind;
use kagari_runtime::{
    native::{
        binding::NativeResult,
        builder::ModuleBuilder,
        context::CallContext,
        declarations::FunctionDecl,
        function_handle::Function,
        functions::NativeCallback,
        payload::Opaque,
        typed::NativeContext,
        types::Type,
        views::{NativeOperationKind, ValueHandle},
    },
    value::Value,
};
use kagari_stdlib::{
    collections::vector::{ScriptVec, cursor::VecCursor},
    declarations::StandardDeclarations,
    identity,
};

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
        .bind(sequence, unsafe {
            NativeCallback::new(
                |cx: &mut CallContext<'_>, values: ValueHandle<'_>| -> NativeResult<Value> {
                    let mut native = cx.native_context()?;
                    // SAFETY: The callback argument snapshot retains the ValueHandle referent before
                    // the exclusive operation starts.
                    let vector: ScriptVec<i32> = native.conversion().decode_transport(
                        &cx.argument_type_argument(0)?,
                        &values.value_transport(),
                    )?;
                    let guard = values.try_enter_operation()?.unwrap();
                    assert!(
                        cx.heap()
                            // SAFETY: The active call retains this input until its copy is published or consumed.
                            .validate_value_transport(&values.value_transport())
                    );
                    assert!(vector.len(&native).is_err());
                    assert!(vector.get(&mut native, 0).is_err());
                    assert!(vector.set(&mut native, 0, 99).is_err());
                    drop(guard);
                    assert_eq!(
                        vector.with_slice::<i32, _>(&native, |values| Ok(values.to_vec()))?,
                        [10, 20]
                    );
                    Ok(Value::Unit)
                },
            )
        })
        .unwrap();
    let iterator = module
        .define_function(
            FunctionDecl::new("iterator")
                .parameter(
                    "cursor",
                    language
                        .catalog()
                        .unwrap()
                        .type_reference(&identity::type_id(
                            "VecCursor",
                            DefinitionKind::AssociatedType,
                        ))
                        .unwrap()
                        .apply([Type::i32()])
                        .unwrap(),
                )
                .parameter(
                    "next",
                    Type::function([], language.option(Type::i32()).unwrap()).unwrap(),
                )
                .returns(language.option(Type::i32()).unwrap()),
        )
        .unwrap();
    module
        .bind(
            iterator,
            |cx: &mut NativeContext<'_>,
             (cursor, next): (Opaque<VecCursor>, Function<(), Option<i32>>)|
             -> NativeResult<Option<i32>> {
                // SAFETY: This test makes no payload edit and needs no recovery state.
                let guard = unsafe {
                    cursor.try_enter_operation(cx, NativeOperationKind::ExclusiveSlots, None)?
                }
                .unwrap();
                assert!(next.call(cx, ()).is_err());
                drop(guard);
                next.call(cx, ())
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
            (iterator(cursor, || cursor.next()), cursor.next())
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
