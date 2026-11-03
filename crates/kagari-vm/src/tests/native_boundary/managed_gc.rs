//! Real VM callbacks reenter the same candidate memory driver through host code.

use super::compile;
use crate::error::VmError;
use kagari_gc::heap::HeapConfig;
use kagari_runtime::{
    error::RuntimeErrorKind,
    native::{
        binding::NativeResult,
        builder::ModuleBuilder,
        context::CallContext,
        declarations::FunctionDecl,
        function_handle::Function,
        managed::{
            call::Memory,
            payload::{Leaf, NativeObject},
            slots::Object,
        },
        typed::NativeContext,
        types::Type,
    },
};
use kagari_stdlib::{
    declarations::StandardDeclarations,
    managed::{
        fold::fold,
        vector::{VectorMut, VectorStorage},
    },
};
use std::{
    cell::RefCell,
    rc::Rc,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
};

type CounterObject = NativeObject<Leaf<u64>>;
type VectorObject = NativeObject<VectorStorage<Object<CounterObject>>>;

thread_local! {
    // A test-only scoped host service, never stored in a script object. Cloning
    // its Rc releases the TLS loan before callback/driver reentry.
    static ACTIVE: RefCell<Vec<Rc<Memory>>> = const { RefCell::new(Vec::new()) };
}

struct ActiveMemory;

impl ActiveMemory {
    fn enter(memory: Rc<Memory>) -> Self {
        ACTIVE.with(|active| active.borrow_mut().push(memory));
        Self
    }
}

impl Drop for ActiveMemory {
    fn drop(&mut self) {
        ACTIVE.with(|active| active.borrow_mut().pop());
    }
}

#[test]
fn managed_fold_calls_real_script_with_collection_and_synchronous_reentry() {
    let catalog = StandardDeclarations::default().catalog().unwrap();
    let mut builder = ModuleBuilder::new("example::gc_proof", &catalog).unwrap();
    let calls = Arc::new(AtomicUsize::new(0));
    let pulse = builder.define_function(FunctionDecl::new("pulse")).unwrap();
    builder
        .bind(pulse, |cx: &mut CallContext<'_>| -> NativeResult<()> {
            let memory = ACTIVE.with(|active| active.borrow().last().unwrap().clone());
            memory.collect()?;
            cx.collect_garbage()?;
            Ok(())
        })
        .unwrap();
    let nested = builder
        .define_function(
            FunctionDecl::new("nested")
                .parameter("value", Type::u64())
                .parameter(
                    "callback",
                    Type::function([Type::u64()], Type::u64()).unwrap(),
                )
                .returns(Type::u64()),
        )
        .unwrap();
    let observed = calls.clone();
    builder
        .bind(
            nested,
            move |cx: &mut NativeContext<'_>,
                  (value, callback): (u64, Function<(u64,), u64>)|
                  -> NativeResult<u64> {
                let memory = ACTIVE.with(|active| active.borrow().last().unwrap().clone());
                memory.call(|inner| {
                    let slot = inner.local::<Object<NativeObject<Leaf<u64>>>>()?;
                    inner.region(|r| {
                        let value = NativeObject::alloc(r, value)?;
                        r.write(slot, value)
                    })?;
                    inner.collect()?;
                    cx.collect_garbage()?;
                    let result = callback.call(cx, (value,))?;
                    inner.region(|r| {
                        let value = r.read(slot)?;
                        assert_eq!(NativeObject::read(r, value, |v| v.0)?, result - 1);
                        Ok(())
                    })?;
                    observed.fetch_add(1, Ordering::Relaxed);
                    Ok(result)
                })
            },
        )
        .unwrap();
    let entry = builder
        .define_function(
            FunctionDecl::new("fold")
                .parameter(
                    "callback",
                    Type::function([Type::u64(), Type::u64()], Type::u64()).unwrap(),
                )
                .returns(Type::u64()),
        )
        .unwrap();
    builder
        .bind(
            entry,
            |cx: &mut NativeContext<'_>,
             (callback,): (Function<(u64, u64), u64>,)|
             -> NativeResult<u64> {
                let memory = Rc::new(Memory::new(HeapConfig {
                    max_roots: 0,
                    ..Default::default()
                })?);
                let _active = ActiveMemory::enter(memory.clone());
                let result = memory.call(|call| {
                    let values = call.local::<Object<VectorObject>>()?;
                    call.region(|r| {
                        let vector = VectorMut::alloc(r)?;
                        for value in 1..=4 {
                            let value = NativeObject::alloc(r, value)?;
                            vector.push(r, value)?;
                        }
                        r.write(values, vector.object())
                    })?;
                    let before = call.working_slots()?;
                    let result = fold(
                        call,
                        values,
                        0,
                        |r, value| NativeObject::read(r, value, |v| v.0),
                        &mut |_, sum, item| callback.call(cx, (sum, item)),
                    )?;
                    // One source slot plus two reused scalar working slots. Nested calls
                    // are gone; no Root/RootSet is registered per callback or element.
                    assert_eq!(call.working_slots()?, before + 2);
                    call.collect()?;
                    assert_eq!(call.stats()?.objects, 5);
                    assert_eq!(call.stats()?.root_registrations, 0);
                    Ok(result)
                });
                memory.collect()?;
                assert_eq!(memory.stats()?.objects, 0);
                result
            },
        )
        .unwrap();
    let module = builder.finish().unwrap();
    let (vm, program) = compile(
        r#"
        use example::gc_proof;
        pub fn main() -> u64 {
            gc_proof::fold(|sum, item| sum + gc_proof::nested(item, |value| { val scratch = [value]; gc_proof::pulse(); scratch[0] + 1 }))
        }
        pub fn fail() -> u64 {
            gc_proof::fold(|sum, item| sum + item / (item - item))
        }
    "#,
        Some(&module),
    );
    let owner = vm.runtime().select_program(&program).unwrap();
    assert_eq!(vm.execute_typed::<_, u64>(&owner, "main", ()).unwrap(), 14);
    assert_eq!(calls.load(Ordering::Relaxed), 4);
    let failure = vm.execute_typed::<_, u64>(&owner, "fail", ()).unwrap_err();
    assert!(
        matches!(failure.cause(), VmError::RuntimeError(error) if error.kind() == RuntimeErrorKind::ScriptTrap)
    );
    assert!(ACTIVE.with(|active| active.borrow().is_empty()));
}
