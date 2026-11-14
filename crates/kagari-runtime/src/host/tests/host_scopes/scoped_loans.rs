//! Small actual-pointer fixtures isolate Miri safety from source compilation cost.
#[cfg(test)]
use crate::test_support::bytecode::load_bytecode;
use crate::test_support::native::program::ProgramFixture;
mod fields;
use crate::{
    Runtime,
    error::{RuntimeError, RuntimeErrorKind},
    native::{
        builder::ModuleBuilder, catalog::DeclarationCatalog,
        conversion::context::ConversionContext, storage::NativeStorage,
    },
    value::{carriers::RootedValue, strings::StringObject, view::ValueView},
};
use kagari_bytecode::{module::BytecodeModule, program::ModuleRef};
use kagari_common::identity::map::DefinitionContext;
use std::{
    panic::{AssertUnwindSafe, catch_unwind},
    ptr,
};

crate::native_data! { struct State { count: i32 } }

#[test]
fn external_root_actual_pointer_loans_release_and_revoke_on_all_exits() {
    let mut builder = ModuleBuilder::new("test::scope", &DeclarationCatalog::default()).unwrap();
    let mut declaration = builder.define_type("State").unwrap();
    declaration
        .native_storage(NativeStorage::data::<State>())
        .unwrap();
    let declaration = declaration.finish().unwrap();
    let module = builder.finish().unwrap();
    let mut runtime = Runtime::default();
    module.install(&mut runtime).unwrap();
    // This scope uses the real checked descriptor and heap/header owner. No
    // frontend, standard-library declarations or substitute borrow model is needed.
    let context = DefinitionContext::new().unwrap();
    let owner_program = load_bytecode(
        &mut runtime,
        "scope-proof",
        ProgramFixture {
            exports: Default::default(),
            root: ModuleRef::new(0),
            modules: vec![BytecodeModule::default()],
        }
        .metadata(context.snapshot(), &Default::default())
        .unwrap(),
    )
    .unwrap();

    let descriptor = runtime
        .bind_native_type::<State>(&owner_program.root(), &declaration, &[])
        .unwrap();
    let owner = runtime.select_program(&owner_program).unwrap();
    runtime.collect_garbage().unwrap();
    let baseline = runtime.gc_stats().root_registrations;
    let mut external = State { count: 1 };
    let mut saved = None;
    let mut retained: Option<RootedValue> = None;
    runtime
        .with_host_scope(|scope| {
            let view = scope.borrow_mut(descriptor.rust_type(), &mut external)?;
            view.read(scope, |scope, state| {
                view.read(scope, |_scope, other| {
                    assert_eq!(state.count, other.count);
                    Ok(())
                })
            })?;
            assert!(
                view.read(scope, |scope, _| view.edit(scope, |_scope, _| Ok(())))
                    .is_err()
            );
            assert!(
                view.edit(scope, |scope, _| view.read(scope, |_scope, _| Ok(())))
                    .is_err()
            );
            assert!(
                view.edit(scope, |_scope, state| {
                    state.count += 1;
                    Err::<(), _>(RuntimeError::host_call_failure("completed edit"))
                })
                .is_err()
            );
            assert!(
                catch_unwind(AssertUnwindSafe(|| view.edit(
                    scope,
                    |_scope, state| -> Result<(), RuntimeError> {
                        state.count += 1;
                        panic!("loan unwind");
                    }
                )))
                .is_err()
            );
            view.edit(scope, |_scope, state| {
                state.count += 1;
                Ok(())
            })?;
            scope.execute(&[view.value()], |runtime, values, result| {
                let view = {
                    // The argument belongs to the caller source, while this
                    // conversion owns only its independent scratch interval.
                    let mut cx = ConversionContext::new(runtime, &owner)?;
                    let value = values.get(&mut cx, 0)?;
                    cx.runtime_mut().scoped_view(&value)?
                };
                saved = Some(view);
                std::thread::scope(|threads| {
                    let runtime = &mut *runtime;
                    let error = threads
                        .spawn(move || {
                            runtime
                                .host_borrows
                                .read_scoped::<State, ()>(runtime, view, |_| {
                                    panic!("foreign admission thread must not project a Rust root")
                                })
                        })
                        .join()
                        .unwrap()
                        .unwrap_err();
                    assert_eq!(error.kind(), RuntimeErrorKind::ExecutionPhaseViolation);
                });
                result.publish(runtime, &ValueView::unit())
            })?;
            scope.execute(&[view.value()], |runtime, _, result| {
                runtime.collect_garbage()?;
                assert_eq!(runtime.gc_stats().root_registrations, baseline);
                result.publish(runtime, &ValueView::unit())
            })?;
            let value = scope.execute(&[], |runtime, _, result| {
                let mut cx = ConversionContext::new(runtime, &owner)?;
                let value = cx.allocate_string("surviving child".into())?;
                // Publish before cx retires its own scratch and execution.
                result.publish(cx.runtime_mut(), &value)
            })?;
            scope.execute(&[value], |runtime, values, result| {
                runtime.collect_garbage()?;
                assert_eq!(runtime.gc_stats().root_registrations, baseline);
                let mut cx = ConversionContext::new(runtime, &owner)?;
                let value = values.get(&mut cx, 0)?;
                cx.read_string(&value, |text| assert_eq!(text, "surviving child"))?;
                result.publish(cx.runtime_mut(), &ValueView::unit())
            })?;
            // Owning escape is explicit; temporary transport above adds no root.
            retained = Some(scope.retain(&value)?);
            Ok(())
        })
        .unwrap();
    assert_eq!(external.count, 4);
    let error = runtime
        .host_borrows
        .validate_scoped(saved.unwrap())
        .unwrap_err();
    assert_eq!(error.kind(), RuntimeErrorKind::ExpiredHostBorrow);
    runtime.collect_garbage().unwrap();
    assert_eq!(runtime.gc_stats().root_registrations, baseline + 1);
    runtime
        .memory
        .scope(&runtime.resources, |cx, _| {
            let value = retained.as_ref().unwrap().get(cx).unwrap();
            let value = cx
                .downcast::<StringObject>(value.object().unwrap())
                .unwrap();
            cx.read(value, |value| assert_eq!(value.as_str(), "surviving child"));
            Ok(())
        })
        .unwrap();
    drop(retained.take());
    assert!(
        catch_unwind(AssertUnwindSafe(|| runtime.with_host_scope(
            |scope| -> Result<(), RuntimeError> {
                let view = scope.borrow_mut(descriptor.rust_type(), &mut external)?;
                scope.execute(&[view.value()], |runtime, values, result| {
                    let mut cx = ConversionContext::new(runtime, &owner)?;
                    let value = values.get(&mut cx, 0)?;
                    saved = Some(cx.runtime_mut().scoped_view(&value)?);
                    result.publish(cx.runtime_mut(), &ValueView::unit())
                })?;
                view.edit(scope, |_scope, state| {
                    state.count += 1;
                    Ok(())
                })?;
                panic!("scope unwind");
            }
        )))
        .is_err()
    );
    assert_eq!(external.count, 5);
    let error = runtime
        .host_borrows
        .validate_scoped(saved.unwrap())
        .unwrap_err();
    assert_eq!(error.kind(), RuntimeErrorKind::ExpiredHostBorrow);
    runtime
        .with_host_scope(|scope| {
            let view = scope.borrow(descriptor.rust_type(), &external)?;
            view.read(scope, |_scope, state| {
                assert_eq!(state.count, 5);
                Ok(())
            })
        })
        .unwrap();
    runtime.collect_garbage().unwrap();
    assert_eq!(runtime.gc_stats().root_registrations, baseline);
}

crate::native_data! { struct Marker {} }

#[test]
fn distinct_zero_sized_host_roots_allow_independent_unique_loans() {
    let mut builder = ModuleBuilder::new("test::zero", &DeclarationCatalog::default()).unwrap();
    let mut declaration = builder.define_type("Marker").unwrap();
    declaration
        .native_storage(NativeStorage::data::<Marker>())
        .unwrap();
    let declaration = declaration.finish().unwrap();
    let module = builder.finish().unwrap();
    let mut runtime = Runtime::default();
    module.install(&mut runtime).unwrap();
    let context = DefinitionContext::new().unwrap();
    let owner_program = load_bytecode(
        &mut runtime,
        "scope-proof",
        ProgramFixture {
            exports: Default::default(),
            root: ModuleRef::new(0),
            modules: vec![BytecodeModule::default()],
        }
        .metadata(context.snapshot(), &Default::default())
        .unwrap(),
    )
    .unwrap();
    let descriptor = runtime
        .bind_native_type::<Marker>(&owner_program.root(), &declaration, &[])
        .unwrap();
    let mut values = [Marker {}, Marker {}];
    let (first, second) = values.split_at_mut(1);
    assert!(ptr::eq(&first[0], &second[0]));
    runtime
        .with_host_scope(|scope| {
            let first = scope.borrow_mut(descriptor.rust_type(), &mut first[0])?;
            let second = scope.borrow_mut(descriptor.rust_type(), &mut second[0])?;
            first.edit(scope, |scope, _| second.edit(scope, |_scope, _| Ok(())))?;
            assert!(
                first
                    .edit(scope, |scope, _| first.edit(scope, |_scope, _| Ok(())))
                    .is_err()
            );
            Ok(())
        })
        .unwrap();
}
