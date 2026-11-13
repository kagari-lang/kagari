#[cfg(test)]
use crate::tests::support::bytecode::load_bytecode;
use crate::{
    declarations::StandardDeclarations,
    tests::{
        execution::native_boundary::{compile_program, native_boundary_functions::fixture},
        support::bytecode::reload_bytecode,
    },
};
mod managed;
mod methods;

use kagari_runtime::{
    error::RuntimeError,
    native::{
        binding::{Codec, NativeBinding, NativeResult},
        builder::ModuleBuilder,
        conversion::context::ConversionContext,
        declarations::{FunctionDecl, MethodDecl},
        module::NativeModule,
        payload::Opaque,
        storage::{NativePayload, NativeStorage, initializer::NativeInit},
        typed::{NativeContext, loans::LoanContext},
        types::{Type, TypeRef},
    },
    value::{carriers::StoredValue, view::ValueView},
};

use kagari_gc::{
    error::GcError,
    object::{Gc, GcObject, ObjectCharge},
    scan::{EdgeVisitor, ScanBudget, ScanKind, ScanStatus},
};
use kagari_types::{reference::ReferenceAccess, scalar::BuiltinType, ty::Ty};
use std::{
    panic::{AssertUnwindSafe, catch_unwind},
    slice,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    thread,
};

kagari_runtime::native_data! {
    struct PlayerState { hp: i32, position: [f32; 3] }
}

// SAFETY: the recursive NativeData fields are fixed and edge-free. This explicit
// provider tests read-only payload registration of the same Rust mapping.
unsafe impl GcObject for PlayerState {
    type ScanState = ();
    const SCAN_KIND: ScanKind = ScanKind::EdgeFree;

    fn begin_scan(&self) {}

    fn scan(
        &self,
        _: &mut (),
        _: &mut ScanBudget,
        _: &mut EdgeVisitor<'_>,
    ) -> Result<ScanStatus, GcError> {
        Ok(ScanStatus::Complete)
    }

    fn charge(&self) -> ObjectCharge {
        ObjectCharge {
            units: size_of::<Self>().div_ceil(size_of::<usize>()),
            bytes: 0,
        }
    }
}

#[test]
fn checked_data_edits_preserve_writes_and_release_exclusive_borrows_on_failure() {
    let mut module = ModuleBuilder::new(
        "example::data",
        &StandardDeclarations::default().catalog().unwrap(),
    )
    .unwrap();
    let mut declaration = module.define_type("Player").unwrap();
    declaration
        .native_storage(NativeStorage::data::<PlayerState>())
        .unwrap();
    let player_type = declaration.finish().unwrap();
    let mut declaration = module.define_type("ReadOnlyPayload").unwrap();
    declaration
        .native_storage(NativeStorage::payload::<PlayerState>())
        .unwrap();
    let no_edit = declaration.finish().unwrap();
    module
        .implement(no_edit.clone(), |implementation| {
            implementation.inherent_impl(|methods| {
                let edit = methods.define_method(
                    MethodDecl::instance("edit")
                        .receiver_access(ReferenceAccess::Mutable)
                        .returns(Type::i32()),
                )?;
                methods.bind(
                    edit,
                    |_: &mut LoanContext<'_>, (state,): (&mut PlayerState,)| -> NativeResult<i32> {
                        state.hp = 0;
                        panic!("payload registration must reject this loan before callback effects")
                    },
                )
            })
        })
        .unwrap();
    let new = module
        .define_function(FunctionDecl::new("new").returns(player_type.apply([]).unwrap()))
        .unwrap();
    module
        .bind(
            new,
            |cx: &mut NativeContext<'_>, (): ()| -> NativeResult<Opaque<PlayerState>> {
                cx.create_opaque(PlayerState {
                    hp: 100,
                    position: [0.0; 3],
                })
            },
        )
        .unwrap();
    let damage = module
        .define_function(
            FunctionDecl::new("damage")
                .parameter("player", player_type.apply([]).unwrap())
                .parameter("amount", Type::i32())
                .returns(Type::i32()),
        )
        .unwrap();
    module
        .bind(
            damage,
            |cx: &mut NativeContext<'_>,
             (player, damage): (Opaque<PlayerState>, i32)|
             -> NativeResult<i32> {
                player.edit(cx, |state| {
                    state.hp = state.hp.saturating_sub(damage);
                    Ok(())
                })?;
                cx.collect_garbage()?;
                player.read(cx, |state| Ok(state.hp))
            },
        )
        .unwrap();
    let (mut vm, owner) = fixture(
        include_str!("../../../../tests/fixtures/native/checked-data-edits.kgr"),
        Some(&module.finish().unwrap()),
    );
    let _owner_program = owner;
    let owner = vm.select_program(&_owner_program).unwrap();
    vm.collect_garbage().unwrap();
    let baseline = vm.gc_stats();
    let object: Opaque<PlayerState> = vm
        .execute_typed(&_owner_program, "make", (), &Default::default())
        .unwrap();
    let alias = object.clone();
    let mut cx = vm.context(&owner).unwrap();
    cx.collect_garbage().unwrap();
    let before = cx.runtime().gc_stats().provider_units;
    object
        .edit(&mut cx, |state| {
            state.position[0] = 2.0;
            Ok(())
        })
        .unwrap();
    assert!(
        object
            .edit::<()>(&mut cx, |state| {
                state.hp = 90;
                Err(RuntimeError::module_validation("after write"))
            })
            .is_err()
    );
    assert_eq!(alias.read(&mut cx, |state| Ok(state.hp)).unwrap(), 90);
    assert!(
        catch_unwind(AssertUnwindSafe(|| {
            let _ = object.edit::<()>(&mut cx, |state| {
                state.hp = 80;
                panic!("after write")
            });
        }))
        .is_err()
    );
    assert_eq!(
        alias
            .read(&mut cx, |state| Ok((state.hp, state.position[0])))
            .unwrap(),
        (80, 2.0)
    );
    assert_eq!(
        cx.runtime_mut()
            .execute_typed::<_, i32>(
                &_owner_program,
                "hit",
                (alias.clone(), 38),
                &Default::default()
            )
            .unwrap(),
        42
    );
    assert_eq!(cx.runtime().gc_stats().provider_units, before);
    let no_edit = cx
        .runtime_mut()
        .bind_native_type::<PlayerState>(&_owner_program.root(), &no_edit, &[])
        .unwrap()
        .create_with(&mut cx, |native| {
            native.scope().alloc_leaf(PlayerState {
                hp: 1,
                position: [0.0; 3],
            })
        })
        .unwrap();
    assert!(no_edit.edit(&mut cx, |_| Ok(())).is_err());
    assert!(
        cx.runtime_mut()
            .execute_typed::<_, i32>(
                &_owner_program,
                "blocked",
                (no_edit.clone(),),
                &Default::default()
            )
            .is_err()
    );
    assert_eq!(no_edit.read(&mut cx, |state| Ok(state.hp)).unwrap(), 1);
    drop((object, alias, no_edit, cx));
    vm.collect_garbage().unwrap();
    assert_eq!(vm.gc_stats().objects, baseline.objects);
    assert_eq!(
        vm.gc_stats().root_registrations,
        baseline.root_registrations
    );
}

#[derive(Debug)]
struct Counter {
    value: i32,
    dropped: Arc<AtomicUsize>,
}

impl Drop for Counter {
    fn drop(&mut self) {
        self.dropped.fetch_add(1, Ordering::Relaxed);
    }
}

impl Counter {
    fn allocate<'gc>(
        native: &mut NativeInit<'_, '_, 'gc>,
        value: i32,
        dropped: Arc<AtomicUsize>,
    ) -> Result<Gc<'gc, Self>, GcError> {
        native.scope().alloc_leaf(Self { value, dropped })
    }
}

impl NativePayload for Counter {
    fn registration_storage() -> NativeStorage {
        NativeStorage::payload::<Self>()
    }
}

// SAFETY: scalar state and an inert Rust drop counter; no managed peer or owned
// backing is inspected by scanning or destruction.
unsafe impl GcObject for Counter {
    type ScanState = ();
    const SCAN_KIND: ScanKind = ScanKind::EdgeFree;

    fn begin_scan(&self) {}

    fn scan(
        &self,
        _: &mut (),
        _: &mut ScanBudget,
        _: &mut EdgeVisitor<'_>,
    ) -> Result<ScanStatus, GcError> {
        Ok(ScanStatus::Complete)
    }

    fn charge(&self) -> ObjectCharge {
        ObjectCharge { units: 1, bytes: 0 }
    }
}

#[derive(Debug)]
struct Edge(StoredValue);

impl Edge {
    fn allocate<'gc>(
        native: &mut NativeInit<'_, '_, 'gc>,
        value: &ValueView<'_>,
    ) -> Result<Gc<'gc, Self>, GcError> {
        let value = native.value(value)?;
        // SAFETY: only this initializer installs the field; the provider scans
        // the complete same-heap cell and never extracts it in Drop.
        unsafe {
            native
                .scope()
                .alloc_with(|mut init| Ok(Self(StoredValue::initialize(&mut init, value)?)))
        }
    }
}

impl NativePayload for Edge {
    fn registration_storage() -> NativeStorage {
        NativeStorage::payload::<Self>()
    }
}

// SAFETY: one initialized immutable same-heap cell, complete retry-safe scanning
// and no peer-aware destructor or indirect backing.
unsafe impl GcObject for Edge {
    type ScanState = bool;
    const SCAN_KIND: ScanKind = ScanKind::Bounded { max_work: 1 };

    fn begin_scan(&self) -> bool {
        false
    }

    fn scan(
        &self,
        visited: &mut bool,
        budget: &mut ScanBudget,
        visitor: &mut EdgeVisitor<'_>,
    ) -> Result<ScanStatus, GcError> {
        if !*visited {
            if !budget.take(1) {
                return Ok(ScanStatus::Pending);
            }
            // SAFETY: this admitted provider retains its initialized cell.
            if unsafe { self.0.scan(budget, visitor)? } == ScanStatus::Pending {
                return Ok(ScanStatus::Pending);
            }
            *visited = true;
        }
        Ok(ScanStatus::Complete)
    }

    fn charge(&self) -> ObjectCharge {
        ObjectCharge { units: 1, bytes: 0 }
    }
}

fn module(dropped: Arc<AtomicUsize>) -> (NativeModule, TypeRef, TypeRef, TypeRef) {
    let mut module = ModuleBuilder::new(
        "example::payload",
        &StandardDeclarations::default().catalog().unwrap(),
    )
    .unwrap();
    let mut declaration = module.define_type("Counter").unwrap();
    declaration
        .native_storage(NativeStorage::payload::<Counter>())
        .unwrap();
    let counter = declaration.finish().unwrap();
    let mut declaration = module.define_type("Other").unwrap();
    declaration
        .native_storage(NativeStorage::payload::<Counter>())
        .unwrap();
    let other = declaration.finish().unwrap();
    let mut declaration = module.define_type("Edge").unwrap();
    declaration.type_parameter("T").unwrap();
    declaration
        .native_storage(NativeStorage::payload::<Edge>())
        .unwrap();
    let edge = declaration.finish().unwrap();
    let new = module
        .define_function(
            FunctionDecl::new("new")
                .parameter("value", Type::i32())
                .returns(counter.apply([]).unwrap()),
        )
        .unwrap();
    module
        .bind(
            new,
            move |cx: &mut NativeContext<'_>, (value,): (i32,)| -> NativeResult<Opaque<Counter>> {
                cx.result_native_type::<Counter>()?
                    .create_with(cx, |native| {
                        Counter::allocate(native, value, dropped.clone())
                    })
            },
        )
        .unwrap();
    let read = module
        .define_function(
            FunctionDecl::new("read")
                .parameter("counter", counter.apply([]).unwrap())
                .returns(Type::i32()),
        )
        .unwrap();
    module
        .bind(
            read,
            |cx: &mut NativeContext<'_>, (counter,): (Opaque<Counter>,)| -> NativeResult<i32> {
                cx.collect_garbage()?;
                counter.read(cx, |counter| Ok(counter.value))
            },
        )
        .unwrap();
    let wrap = module
        .define_function(FunctionDecl::new("wrap_payload"))
        .unwrap();
    module
        .function(&wrap, |function| {
            let item = function.type_parameter("T")?.ty();
            function.parameter("value", item.clone())?;
            function.returns(edge.apply([item])?)?;
            Ok(())
        })
        .unwrap();
    module
        .bind(
            wrap,
            NativeBinding::new(vec![Codec::Value], edge.codec(), |cx| {
                let value = cx.argument(0)?;
                cx.allocate_result_with(&[], |native| Edge::allocate(native, &value))
            }),
        )
        .unwrap();
    (module.finish().unwrap(), counter, other, edge)
}

const SOURCE: &str = include_str!("../../../../tests/fixtures/native/opaque-counter.kgr");

#[test]
fn retained_native_factories_share_aliases_and_check_nominal_and_runtime_identity() {
    let dropped = Arc::new(AtomicUsize::new(0));
    let (module, declaration, other, _) = module(dropped.clone());
    let (mut vm, owner) = fixture(SOURCE, Some(&module));
    let _owner_program = owner;
    let owner = vm.select_program(&_owner_program).unwrap();
    let runtime = &mut vm;
    let native = runtime
        .bind_native_type::<Counter>(&_owner_program.root(), &declaration, &[])
        .unwrap();
    assert!(
        runtime
            .bind_native_type::<Edge>(&_owner_program.root(), &declaration, &[])
            .is_err()
    );
    let read = runtime
        .bind_function::<(Opaque<Counter>,), i32>(&_owner_program.root(), "value")
        .unwrap();
    assert!(
        runtime
            .bind_function::<(Opaque<Edge>,), i32>(&_owner_program.root(), "value")
            .is_err()
    );
    runtime.collect_garbage().unwrap();
    let baseline = runtime.gc_stats();
    let mut cx = runtime.context(&owner).unwrap();
    let object = native
        .create_with(&mut cx, |native| {
            Counter::allocate(native, 42, dropped.clone())
        })
        .unwrap();
    let alias = object.clone();
    drop(object);
    cx.collect_garbage().unwrap();
    assert_eq!(alias.read(&mut cx, |state| Ok(state.value)).unwrap(), 42);
    assert_eq!(cx.runtime_mut().call(&read, (alias.clone(),)).unwrap(), 42);
    let returned: Opaque<Counter> = cx
        .runtime_mut()
        .execute_typed(&_owner_program, "make", (43,), &Default::default())
        .unwrap();
    assert_eq!(cx.runtime_mut().call(&read, (returned,)).unwrap(), 43);
    let wrong = cx
        .runtime_mut()
        .bind_native_type::<Counter>(&_owner_program.root(), &other, &[])
        .unwrap()
        .create_with(&mut cx, |native| {
            Counter::allocate(native, 0, dropped.clone())
        })
        .unwrap();
    assert!(cx.runtime_mut().call(&read, (wrong,)).is_err());
    let (mut foreign, foreign_owner) = fixture(SOURCE, Some(&module));
    let _foreign_owner_program = foreign_owner;
    let foreign_owner = foreign.select_program(&_foreign_owner_program).unwrap();
    let mut foreign_cx = foreign.context(&foreign_owner).unwrap();
    assert!(alias.read(&mut foreign_cx, |_| Ok(())).is_err());
    assert!(
        foreign_cx
            .runtime_mut()
            .call(&read, (alias.clone(),))
            .is_err()
    );
    assert!(
        native
            .create_with(&mut foreign_cx, |_| {
                panic!("foreign factory must reject before running the initializer")
            })
            .is_err()
    );
    drop((cx, alias));
    runtime.collect_garbage().unwrap();
    assert_eq!(runtime.gc_stats().objects, baseline.objects);
    assert_eq!(
        runtime.gc_stats().root_registrations,
        baseline.root_registrations
    );
    assert_eq!(dropped.load(Ordering::Relaxed), 3);
}

#[test]
fn native_borrows_release_on_error_and_unwind() {
    let (module, _, _, _) = module(Arc::new(AtomicUsize::new(0)));
    let (mut vm, owner) = fixture(SOURCE, Some(&module));
    let _owner_program = owner;
    let owner = vm.select_program(&_owner_program).unwrap();
    vm.collect_garbage().unwrap();
    let baseline = vm.gc_stats();
    let object: Opaque<Counter> = vm
        .execute_typed(&_owner_program, "make", (42,), &Default::default())
        .unwrap();
    let mut cx = vm.context(&owner).unwrap();
    assert!(
        object
            .read(&mut cx, |_| {
                Err::<(), _>(RuntimeError::module_validation("after inspection"))
            })
            .is_err()
    );
    assert!(
        catch_unwind(AssertUnwindSafe(|| {
            let _ = object.read::<()>(&mut cx, |_| panic!("borrow unwind"));
        }))
        .is_err()
    );
    cx.collect_garbage().unwrap();
    assert_eq!(object.read(&mut cx, |state| Ok(state.value)).unwrap(), 42);
    assert!(cx.result_native_type::<Counter>().is_err());
    drop((object, cx));
    vm.collect_garbage().unwrap();
    assert_eq!(vm.gc_stats().objects, baseline.objects);
    assert_eq!(
        vm.gc_stats().root_registrations,
        baseline.root_registrations
    );
}

#[test]
fn payload_constructors_trace_children_and_reject_foreign_inputs_before_publication() {
    let dropped = Arc::new(AtomicUsize::new(0));
    let (module, _, _, edge) = module(dropped.clone());
    let (mut vm, installed) = fixture(SOURCE, Some(&module));
    let owner = vm.select_program(&installed).unwrap();
    let element = ConversionContext::new(&mut vm, &owner)
        .unwrap()
        .type_for::<i32>()
        .unwrap();
    assert!(
        vm.bind_native_type::<Edge>(&installed.root(), &edge, &[])
            .is_err()
    );
    let native = vm
        .bind_native_type_declaration::<Edge>(
            &installed.root(),
            edge.id(),
            edge.definitions(),
            &[element],
        )
        .unwrap();
    assert!(
        matches!(native.type_argument().ty(), Ty::NativeObject(nominal)
        if nominal.arguments == [Ty::Builtin(BuiltinType::I32)])
    );
    drop(native);
    let child: Opaque<Counter> = vm
        .execute_typed(&installed, "make", (42,), &Default::default())
        .unwrap();
    let parent: Opaque<Edge> = vm
        .execute_typed(&installed, "wrap", (child.clone(),), &Default::default())
        .unwrap();
    drop(child);
    vm.collect_garbage().unwrap();
    let retained = vm.gc_stats();
    assert_eq!(dropped.load(Ordering::Relaxed), 0);
    let (mut foreign, foreign_installed) = fixture(SOURCE, Some(&module));
    let child: Opaque<Counter> = foreign
        .execute_typed(&foreign_installed, "make", (0,), &Default::default())
        .unwrap();
    assert!(
        vm.execute_typed::<_, Opaque<Edge>>(&installed, "wrap", (child,), &Default::default())
            .is_err()
    );
    vm.collect_garbage().unwrap();
    assert_eq!(vm.gc_stats().objects, retained.objects);
    assert_eq!(
        vm.gc_stats().root_registrations,
        retained.root_registrations
    );
    assert_eq!(dropped.load(Ordering::Relaxed), 0);
    drop(parent);
    vm.collect_garbage().unwrap();
    assert!(vm.gc_stats().objects < retained.objects);
    assert_eq!(dropped.load(Ordering::Relaxed), 1);
}

#[test]
fn native_handles_pin_old_scopes_and_transfer_with_their_runtime() {
    let dropped = Arc::new(AtomicUsize::new(0));
    let (module, declaration, _, _) = module(dropped.clone());
    let (mut vm, old) = fixture(SOURCE, Some(&module));
    let _old_program = old;
    let old = vm.select_program(&_old_program).unwrap();
    let native = vm
        .bind_native_type::<Counter>(&_old_program.root(), &declaration, &[])
        .unwrap();
    let object = native
        .create_with(&mut vm.context(&old).unwrap(), |native| {
            Counter::allocate(native, 42, dropped.clone())
        })
        .unwrap();
    reload_bytecode(
        &mut vm,
        &_old_program,
        Some("functions".into()),
        compile_program(SOURCE, Some(&module)),
    )
    .unwrap();
    let _new_program = _old_program.clone();
    let new = vm.select_program(&_new_program).unwrap();
    vm.collect_garbage().unwrap();
    vm.validate_executable_module(&old).unwrap();
    thread::spawn(move || {
        assert_eq!(
            vm.execute_typed::<_, i32>(
                &_new_program,
                "value",
                (object.clone(),),
                &Default::default()
            )
            .unwrap(),
            42
        );
        drop(object);
        let created = native
            .clone()
            .create_with(&mut vm.context(&new).unwrap(), |native| {
                Counter::allocate(native, 43, dropped.clone())
            })
            .unwrap();
        assert_eq!(
            created
                .read(&mut vm.context(&new).unwrap(), |state| Ok(state.value))
                .unwrap(),
            43
        );
        drop((created, native));
        vm.collect_garbage().unwrap();
        assert!(vm.validate_executable_module(&old).is_err());
        assert_eq!(dropped.load(Ordering::Relaxed), 2);
    })
    .join()
    .unwrap();
}

#[test]
fn applied_native_handles_preserve_the_supplied_nominal_layout_scope() {
    let (module, _, _, edge) = module(Arc::new(AtomicUsize::new(0)));
    let source = include_str!("../../../../tests/fixtures/native/opaque-applied-layout.kgr");
    let (mut vm, old) = fixture(source, Some(&module));
    let _old_program = old;
    let old = vm.select_program(&_old_program).unwrap();
    let old_item = vm.bind_type(&_old_program.root(), "Item", &[]).unwrap();
    let new = load_bytecode(
        &mut vm,
        "independent",
        compile_program(&source.replace("value: i32", "value: i64"), Some(&module)),
    )
    .unwrap();
    let _new_program = new;
    let runtime = &mut vm;
    let new_item = runtime
        .bind_type(&_new_program.root(), "Item", &[])
        .unwrap();
    assert_eq!(old_item.type_argument().ty(), new_item.type_argument().ty());
    let old_type = runtime
        .bind_native_type::<Edge>(
            &_old_program.root(),
            &edge,
            slice::from_ref(old_item.type_argument()),
        )
        .unwrap();
    // Nominal arguments cannot be rebound through another installation.
    assert!(
        runtime
            .bind_native_type::<Edge>(
                &_old_program.root(),
                &edge,
                slice::from_ref(new_item.type_argument())
            )
            .is_err()
    );
    let new_type = runtime
        .bind_native_type::<Edge>(
            &_new_program.root(),
            &edge,
            slice::from_ref(new_item.type_argument()),
        )
        .unwrap();
    runtime.collect_garbage().unwrap();
    let baseline = runtime.gc_stats();
    let mut cx = runtime.context(&old).unwrap();
    let old_value = old_type
        .create_with(&mut cx, |native| Edge::allocate(native, &ValueView::unit()))
        .unwrap();
    let new_owner = cx.runtime().select_program(&_new_program).unwrap();
    let new_value = new_type
        .create_with(
            &mut cx.runtime_mut().context(&new_owner).unwrap(),
            |native| Edge::allocate(native, &ValueView::unit()),
        )
        .unwrap();
    let old_call = cx
        .runtime_mut()
        .bind_function::<(Opaque<Edge>,), i32>(&_old_program.root(), "accept")
        .unwrap();
    let new_call = cx
        .runtime_mut()
        .bind_function::<(Opaque<Edge>,), i32>(&_new_program.root(), "accept")
        .unwrap();
    assert_eq!(
        cx.runtime_mut()
            .call(&old_call, (old_value.clone(),))
            .unwrap(),
        42
    );
    assert!(cx.runtime_mut().call(&new_call, (old_value,)).is_err());
    assert!(
        cx.runtime_mut()
            .call(&old_call, (new_value.clone(),))
            .is_err()
    );
    assert_eq!(cx.runtime_mut().call(&new_call, (new_value,)).unwrap(), 42);
    cx.collect_garbage().unwrap();
    assert_eq!(cx.runtime().gc_stats().objects, baseline.objects);
    assert_eq!(
        cx.runtime().gc_stats().root_registrations,
        baseline.root_registrations
    );
}
