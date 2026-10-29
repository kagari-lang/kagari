//! Small actual-pointer fixtures isolate Miri safety from source compilation cost.
use crate::test_support::native::program::ProgramFixture;
mod fields;
use crate::{
    Runtime,
    error::{RuntimeError, RuntimeErrorKind},
    native::{builder::ModuleBuilder, catalog::DeclarationCatalog, storage::NativeStorage},
    value::Value,
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
    let owner_program = runtime
        .load_program(
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
    let mut external = State { count: 1 };
    let mut saved = None;
    runtime
        .with_host_scope(|scope| {
            let view = scope.borrow_mut(descriptor.rust_type(), &mut external)?;
            // SAFETY: Only inert host-frame identity escapes; it contains no Rust address.
            unsafe {
                scope.execute_transport(&[view.argument()], |runtime, values| {
                    saved = Some(runtime.scoped_view(&values[0])?);
                    Ok(Value::Unit)
                })
            }?;
            view.read(scope, |state| {
                view.read(scope, |other| {
                    assert_eq!(state.count, other.count);
                    Ok(())
                })
            })?;
            assert!(view.read(scope, |_| view.edit(scope, |_| Ok(()))).is_err());
            assert!(view.edit(scope, |_| view.read(scope, |_| Ok(()))).is_err());
            assert!(
                view.edit(scope, |state| {
                    state.count += 1;
                    Err::<(), _>(RuntimeError::host_call_failure("completed edit"))
                })
                .is_err()
            );
            assert!(
                catch_unwind(AssertUnwindSafe(|| view.edit(
                    scope,
                    |state| -> Result<(), RuntimeError> {
                        state.count += 1;
                        panic!("loan unwind");
                    }
                )))
                .is_err()
            );
            view.edit(scope, |state| {
                state.count += 1;
                Ok(())
            })?;
            scope.runtime().collect_garbage()?;
            Ok(())
        })
        .unwrap();
    assert_eq!(external.count, 4);
    let error = runtime
        .host_borrows
        .validate_scoped(saved.unwrap())
        .unwrap_err();
    assert_eq!(error.kind(), RuntimeErrorKind::ExpiredHostBorrow);
    assert!(
        catch_unwind(AssertUnwindSafe(|| runtime.with_host_scope(
            |scope| -> Result<(), RuntimeError> {
                let view = scope.borrow_mut(descriptor.rust_type(), &mut external)?;
                // SAFETY: Only inert host-frame identity escapes; it contains no Rust address.
                unsafe {
                    scope.execute_transport(&[view.argument()], |runtime, values| {
                        saved = Some(runtime.scoped_view(&values[0])?);
                        Ok(Value::Unit)
                    })
                }?;
                view.edit(scope, |state| {
                    state.count += 1;
                    Ok(())
                })?;
                panic!("scope unwind");
            }
        )))
        .is_err()
    );
    assert_eq!(external.count, 5);
    assert!(
        runtime
            .host_borrows
            .validate_scoped(saved.unwrap())
            .is_err()
    );
    runtime
        .with_host_scope(|scope| {
            let view = scope.borrow(descriptor.rust_type(), &external)?;
            view.read(scope, |state| {
                assert_eq!(state.count, 5);
                Ok(())
            })
        })
        .unwrap();
    runtime.collect_garbage().unwrap();
    assert_eq!(runtime.gc().active_roots(), 0);
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
    let owner_program = runtime
        .load_program(
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
            first.edit(scope, |_| second.edit(scope, |_| Ok(())))?;
            assert!(
                first
                    .edit(scope, |_| first.edit(scope, |_| Ok(())))
                    .is_err()
            );
            Ok(())
        })
        .unwrap();
}
