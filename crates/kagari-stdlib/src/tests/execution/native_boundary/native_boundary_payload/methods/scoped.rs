use super::{SOURCE, registration};
use crate::tests::execution::native_boundary::{
    native_boundary_functions::fixture, native_boundary_payload::PlayerState,
};
use kagari_runtime::{
    Runtime,
    context::ExecutionContext,
    error::{RuntimeError, RuntimeErrorKind},
    native::{binding::NativeResult, conversion::context::ConversionContext},
    value::view::ValueView,
};

use kagari_common::cancellation::CancellationToken;
use std::panic::{AssertUnwindSafe, catch_unwind};

#[test]
fn external_scopes_use_registered_payload_methods_and_revoke_old_views() {
    let (module, player, _) = registration();
    let (mut vm, owner) = fixture(SOURCE, Some(&module));
    let _owner_program = owner;
    let owner = vm.select_program(&_owner_program).unwrap();
    let descriptor = vm
        .bind_native_type::<PlayerState>(&_owner_program.root(), &player, &[])
        .unwrap();
    let mut external = PlayerState {
        hp: 10,
        position: [1.0; 3],
    };
    vm.with_host_scope(|scope| {
        let view = scope.borrow_mut(descriptor.rust_type(), &mut external)?;
        view.read(scope, |scope, state| {
            assert_eq!(state.hp, 10);
            assert!(view.edit(scope, |_scope, _| Ok(())).is_err());
            view.read(scope, |_scope, nested| {
                assert_eq!(nested.hp, 10);
                Ok(())
            })
        })?;
        view.read(scope, |scope, _| {
            Runtime::execute_scoped(
                scope,
                &_owner_program,
                "scoped_read_mut",
                &[view.value()],
                &Default::default(),
            )?;
            assert!(
                Runtime::execute_scoped(
                    scope,
                    &_owner_program,
                    "scoped_damage",
                    &[view.value()],
                    &Default::default()
                )
                .is_err()
            );
            Ok(())
        })?;
        view.edit(scope, |scope, _| {
            assert!(
                Runtime::execute_scoped(
                    scope,
                    &_owner_program,
                    "scoped_read_mut",
                    &[view.value()],
                    &Default::default()
                )
                .is_err()
            );
            Ok(())
        })?;
        assert!(
            view.edit::<()>(scope, |scope, state| {
                state.position[0] = 2.0;
                assert!(view.read(scope, |_scope, _| Ok(())).is_err());
                assert!(view.edit(scope, |_scope, _| Ok(())).is_err());
                Err(RuntimeError::host_call_failure("completed write"))
            })
            .is_err()
        );
        assert!(
            catch_unwind(AssertUnwindSafe(|| {
                let _: NativeResult<()> = view.edit(scope, |_scope, state| {
                    state.position[1] = 3.0;
                    panic!("loan cleanup")
                });
            }))
            .is_err()
        );
        view.read(scope, |_scope, state| {
            assert_eq!(state.position[..2], [2.0, 3.0]);
            Ok(())
        })?;
        assert_eq!(
            Runtime::execute_scoped(
                scope,
                &_owner_program,
                "scoped_damage",
                &[view.value()],
                &Default::default()
            )?
            .as_i32(),
            Some(9)
        );
        let inspected = Runtime::execute_scoped(
            scope,
            &_owner_program,
            "scoped_read_mut",
            &[view.value()],
            &Default::default(),
        )?;
        scope.execute(&[inspected], |runtime, arguments, result| {
            let mut cx = ConversionContext::new(runtime, &owner)?;
            let text = arguments.get(&mut cx, 0)?;
            cx.read_string(&text, |text| assert_eq!(text, "hp=9"))?;
            result.publish(cx.runtime_mut(), &ValueView::unit())
        })?;
        Ok(())
    })
    .unwrap();
    assert_eq!(external.hp, 9);
    let (mut foreign, foreign_owner) = fixture(SOURCE, Some(&module));
    let _foreign_owner_program = foreign_owner;
    let foreign_descriptor = foreign
        .bind_native_type::<PlayerState>(&_foreign_owner_program.root(), &player, &[])
        .unwrap();
    vm.with_host_scope(|scope| {
        assert!(
            scope
                .borrow(foreign_descriptor.rust_type(), &external)
                .is_err()
        );
        let view = scope.borrow(descriptor.rust_type(), &external)?;
        assert!(
            Runtime::execute_scoped(
                scope,
                &_owner_program,
                "scoped_damage",
                &[view.value()],
                &Default::default()
            )
            .is_err()
        );
        assert!(
            scope
                .execute(&[view.value()], |runtime, arguments, result| {
                    let mut cx = ConversionContext::new(runtime, &owner)?;
                    let value = arguments.get(&mut cx, 0)?;
                    result.publish(cx.runtime_mut(), &value)
                })
                .is_err()
        );
        Runtime::execute_scoped(
            scope,
            &_owner_program,
            "scoped_inspect",
            &[view.value()],
            &Default::default(),
        )?;
        Ok(())
    })
    .unwrap();
    assert_eq!(external.hp, 9);
    assert!(
        catch_unwind(AssertUnwindSafe(|| {
            let _: NativeResult<()> = vm.with_host_scope(|scope| {
                let view = scope.borrow_mut(descriptor.rust_type(), &mut external)?;
                view.edit(scope, |_scope, state| {
                    state.hp = 8;
                    Ok(())
                })?;
                panic!("scope cleanup")
            });
        }))
        .is_err()
    );
    assert_eq!(external.hp, 8);
    vm.with_host_scope(|scope| {
        let view = scope.borrow_mut(descriptor.rust_type(), &mut external)?;
        view.edit(scope, |_scope, state| {
            state.hp = 9;
            Ok(())
        })
    })
    .unwrap();

    vm.with_host_scope(|scope| {
        let view = scope.borrow_mut(descriptor.rust_type(), &mut external)?;
        let error = Runtime::execute_scoped(
            scope,
            &_owner_program,
            "scoped_recurse",
            &[view.value()],
            &Default::default(),
        )
        .unwrap_err();
        assert_eq!(error.kind(), RuntimeErrorKind::ResourceLimitExceeded);
        view.read(scope, |_scope, state| {
            assert!(state.hp < 9);
            Ok(())
        })?;
        view.edit(scope, |_scope, state| {
            state.hp = 9;
            Ok(())
        })?;
        let token = CancellationToken::default();
        token.cancel();
        let context = ExecutionContext {
            cancellation: token,
            ..Default::default()
        };
        let error = Runtime::execute_scoped(
            scope,
            &_owner_program,
            "scoped_damage",
            &[view.value()],
            &context,
        )
        .unwrap_err();
        assert_eq!(error.kind(), RuntimeErrorKind::Cancelled);
        assert_eq!(
            Runtime::execute_scoped(
                scope,
                &_owner_program,
                "scoped_damage",
                &[view.value()],
                &Default::default()
            )?
            .as_i32(),
            Some(8)
        );
        Ok(())
    })
    .unwrap();
    assert_eq!(external.hp, 8);
}
