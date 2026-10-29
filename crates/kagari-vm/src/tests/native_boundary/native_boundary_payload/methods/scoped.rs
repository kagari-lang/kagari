use super::{SOURCE, registration};
use crate::tests::native_boundary::{
    native_boundary_functions::fixture, native_boundary_payload::PlayerState,
};
use crate::vm::Vm;
use kagari_common::cancellation::CancellationToken;
use kagari_runtime::{
    error::{RuntimeError, RuntimeErrorKind},
    gc::roots::RootSet,
    host_scope::{scoped::HostScope, value::ScopedValue},
    native::binding::NativeResult,
    value::Value,
};
use std::cell::Cell;
use std::panic::{AssertUnwindSafe, catch_unwind};

#[test]
fn external_scopes_use_registered_payload_methods_and_revoke_old_views() {
    let (module, player, _) = registration();
    let (mut vm, owner) = fixture(SOURCE, Some(&module));
    let _owner_program = owner;
    let owner = vm.runtime().select_program(&_owner_program).unwrap();
    let descriptor = vm
        .runtime()
        .bind_native_type::<PlayerState>(&_owner_program.root(), &player, &[])
        .unwrap();
    let mut external = PlayerState {
        hp: 10,
        position: [1.0; 3],
    };
    let mut stale = None;
    vm.runtime_mut()
        .with_host_scope(|scope| {
            let view = scope.borrow_mut(descriptor.rust_type(), &mut external)?;
            view.read(scope, |state| {
                assert_eq!(state.hp, 10);
                assert!(view.edit(scope, |_| Ok(())).is_err());
                view.read(scope, |nested| {
                    assert_eq!(nested.hp, 10);
                    Ok(())
                })
            })?;
            view.read(scope, |_| {
                Vm::execute_scoped(
                    scope,
                    &_owner_program,
                    "scoped_read_mut",
                    &[view.argument()],
                    Default::default(),
                )?;
                assert!(
                    Vm::execute_scoped(
                        scope,
                        &_owner_program,
                        "scoped_damage",
                        &[view.argument()],
                        Default::default()
                    )
                    .is_err()
                );
                Ok(())
            })?;
            view.edit(scope, |_| {
                assert!(
                    Vm::execute_scoped(
                        scope,
                        &_owner_program,
                        "scoped_read_mut",
                        &[view.argument()],
                        Default::default()
                    )
                    .is_err()
                );
                Ok(())
            })?;
            assert!(
                view.edit::<()>(scope, |state| {
                    state.position[0] = 2.0;
                    assert!(view.read(scope, |_| Ok(())).is_err());
                    assert!(view.edit(scope, |_| Ok(())).is_err());
                    Err(RuntimeError::host_call_failure("completed write"))
                })
                .is_err()
            );
            assert!(
                catch_unwind(AssertUnwindSafe(|| {
                    let _: NativeResult<()> = view.edit(scope, |state| {
                        state.position[1] = 3.0;
                        panic!("loan cleanup")
                    });
                }))
                .is_err()
            );
            view.read(scope, |state| {
                assert_eq!(state.position[..2], [2.0, 3.0]);
                Ok(())
            })?;
            assert_eq!(
                Vm::execute_scoped(
                    scope,
                    &_owner_program,
                    "scoped_damage",
                    &[view.argument()],
                    Default::default()
                )?
                .inspect(|view| view.as_i32().map(Value::I32))?,
                Value::I32(9)
            );
            let inspected = Vm::execute_scoped(
                scope,
                &_owner_program,
                "scoped_read_mut",
                &[view.argument()],
                Default::default(),
            )?;
            assert_eq!(
                inspected.inspect(|view| Ok(view.as_str()?.to_owned()))?,
                "hp=9"
            );
            stale = Some(revocation_probe(scope, view.argument())?);
            Ok(())
        })
        .unwrap();
    assert_eq!(external.hp, 9);
    let (foreign, foreign_owner) = fixture(SOURCE, Some(&module));
    let _foreign_owner_program = foreign_owner;
    let foreign_descriptor = foreign
        .runtime()
        .bind_native_type::<PlayerState>(&_foreign_owner_program.root(), &player, &[])
        .unwrap();
    vm.runtime_mut()
        .with_host_scope(|scope| {
            assert!(
                scope
                    .borrow(foreign_descriptor.rust_type(), &external)
                    .is_err()
            );
            let view = scope.borrow(descriptor.rust_type(), &external)?;
            assert!(
                Vm::execute_scoped(
                    scope,
                    &_owner_program,
                    "scoped_damage",
                    &[view.argument()],
                    Default::default()
                )
                .is_err()
            );
            assert!(scope.retain_slot(stale.as_ref().unwrap(), 0).is_err());
            // SAFETY: The argument stays live; returning its capability must fail escape validation.
            assert!(
                unsafe {
                    scope.execute_transport(&[view.argument()], |_, arguments| Ok(arguments[0]))
                }
                .is_err()
            );
            Vm::execute_scoped(
                scope,
                &_owner_program,
                "scoped_inspect",
                &[view.argument()],
                Default::default(),
            )?;
            Ok(())
        })
        .unwrap();
    assert_eq!(external.hp, 9);
    let revoked = Cell::new(None);
    assert!(
        catch_unwind(AssertUnwindSafe(|| {
            let _: NativeResult<()> = vm.runtime_mut().with_host_scope(|scope| {
                let view = scope.borrow_mut(descriptor.rust_type(), &mut external)?;
                revoked.set(Some(revocation_probe(scope, view.argument())?));
                view.edit(scope, |state| {
                    state.hp = 8;
                    Ok(())
                })?;
                panic!("scope cleanup")
            });
        }))
        .is_err()
    );
    assert_eq!(external.hp, 8);
    vm.runtime_mut()
        .with_host_scope(|scope| {
            assert!(scope.retain_slot(&revoked.take().unwrap(), 0).is_err());
            let view = scope.borrow_mut(descriptor.rust_type(), &mut external)?;
            view.edit(scope, |state| {
                state.hp = 9;
                Ok(())
            })
        })
        .unwrap();

    vm.runtime_mut()
        .with_host_scope(|scope| {
            let view = scope.borrow_mut(descriptor.rust_type(), &mut external)?;
            let error = Vm::execute_scoped(
                scope,
                &_owner_program,
                "scoped_recurse",
                &[view.argument()],
                Default::default(),
            )
            .unwrap_err();
            assert_eq!(error.kind(), RuntimeErrorKind::ResourceLimitExceeded);
            view.read(scope, |state| {
                assert!(state.hp < 9);
                Ok(())
            })?;
            view.edit(scope, |state| {
                state.hp = 9;
                Ok(())
            })?;
            let token = CancellationToken::default();
            let error = (|| {
                let runtime = scope.runtime();
                let mut options = runtime.execution_options();
                options.cancellation = token.clone();
                let _session = runtime.begin_execution(&owner, options)?;
                token.cancel();
                Vm::execute_scoped(
                    scope,
                    &_owner_program,
                    "scoped_damage",
                    &[view.argument()],
                    Default::default(),
                )
            })()
            .unwrap_err();
            assert_eq!(error.kind(), RuntimeErrorKind::Cancelled);
            assert_eq!(
                Vm::execute_scoped(
                    scope,
                    &_owner_program,
                    "scoped_damage",
                    &[view.argument()],
                    Default::default()
                )?
                .inspect(|view| view.as_i32().map(Value::I32))?,
                Value::I32(8)
            );
            Ok(())
        })
        .unwrap();
    assert_eq!(external.hp, 8);
}

// Keep the GC wrapper alive to test revoked-host-capability admission, without
// dereferencing an expired heap identity or letting a Rust address escape.
fn revocation_probe<'scope>(
    scope: &HostScope<'scope, '_>,
    argument: ScopedValue<'scope>,
) -> NativeResult<RootSet> {
    let mut root = None;
    // SAFETY: The argument is live through root publication. After scope exit
    // the wrapper is used only for rejection by original host-frame validation.
    unsafe {
        scope.execute_transport(&[argument], |runtime, values| {
            root = runtime
                .gc()
                .root_execution_values_transport(values.to_vec());
            Ok(Value::Unit)
        })
    }?;
    root.ok_or_else(|| RuntimeError::module_validation("revocation probe root"))
}
