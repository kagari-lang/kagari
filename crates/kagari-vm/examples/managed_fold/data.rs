//! Prebuilt data shared by native and script callback measurements.

use kagari_gc::heap::HeapConfig;
use kagari_runtime::native::{
    binding::NativeResult,
    managed::{
        call::{CallRegion, Memory, NativeCall},
        payload::{Leaf, NativeObject},
        slots::{Local, Object, Plain},
        value::RootedValue,
    },
};
use kagari_stdlib::managed::{
    fold::fold,
    vector::{VectorMut, VectorStorage},
};
use std::{cell::RefCell, rc::Rc};

type ScalarVector = Object<NativeObject<VectorStorage<u64>>>;
type Item = Object<NativeObject<Leaf<u64>>>;
type ReferenceVector = Object<NativeObject<VectorStorage<Item>>>;

pub struct Data {
    pub memory: Memory,
    pub plain: Vec<u64>,
    scalar: RootedValue<ScalarVector>,
    references: RootedValue<ReferenceVector>,
}

thread_local! {
    // Benchmark-only host service. No TLS/payload loan crosses a script call.
    static ACTIVE: RefCell<Option<Rc<Data>>> = const { RefCell::new(None) };
}

pub struct ActiveData;

impl ActiveData {
    pub fn enter(data: Rc<Data>) -> Self {
        ACTIVE.with(|active| assert!(active.borrow_mut().replace(data).is_none()));
        Self
    }
}

impl Drop for ActiveData {
    fn drop(&mut self) {
        ACTIVE.with(|active| active.borrow_mut().take());
    }
}

pub fn active() -> Rc<Data> {
    ACTIVE.with(|active| active.borrow().as_ref().unwrap().clone())
}

fn admit<'call, T: Plain>(
    call: &mut NativeCall<'call>,
    root: &RootedValue<Object<NativeObject<VectorStorage<T>>>>,
) -> NativeResult<Local<'call, Object<NativeObject<VectorStorage<T>>>>> {
    let slot = call.local()?;
    call.region(|r| {
        let value = r.admit_value(root)?;
        r.write(slot, value)
    })?;
    Ok(slot)
}

impl Data {
    pub fn new(count: usize) -> NativeResult<Self> {
        let memory = Memory::new(HeapConfig::default())?;
        let (scalar, references) = memory.call(|call| {
            let scalars = call.local::<ScalarVector>()?;
            let references = call.local::<ReferenceVector>()?;
            call.region(|r| {
                let scalar = VectorMut::<u64>::alloc(r)?;
                let reference = VectorMut::<Item>::alloc(r)?;
                for value in 1..=count as u64 {
                    scalar.push(r, value)?;
                    let item = NativeObject::alloc(r, value)?;
                    reference.push(r, item)?;
                }
                r.write(scalars, scalar.object())?;
                r.write(references, reference.object())?;
                Ok((r.root(scalars)?, r.root(references)?))
            })
        })?;
        memory.collect()?;
        assert_eq!(memory.stats()?.objects, count + 2);
        assert_eq!(memory.stats()?.root_registrations, 2);
        Ok(Self {
            memory,
            plain: (1..=count as u64).collect(),
            scalar,
            references,
        })
    }

    pub fn scalar(
        &self,
        reducer: &mut impl FnMut(u64, u64) -> NativeResult<u64>,
    ) -> NativeResult<u64> {
        self.run(&self.scalar, |_, value| Ok(value), reducer)
    }

    pub fn references(
        &self,
        reducer: &mut impl FnMut(u64, u64) -> NativeResult<u64>,
    ) -> NativeResult<u64> {
        self.run(
            &self.references,
            |r, item| NativeObject::read(r, item, |v| v.0),
            reducer,
        )
    }

    fn run<T: Plain>(
        &self,
        root: &RootedValue<Object<NativeObject<VectorStorage<T>>>>,
        project: impl for<'gc> Fn(&CallRegion<'_, 'gc>, T::View<'gc>) -> NativeResult<u64>,
        reducer: &mut impl FnMut(u64, u64) -> NativeResult<u64>,
    ) -> NativeResult<u64> {
        self.memory.call(|call| {
            let source = admit(call, root)?;
            fold(call, source, 0, project, &mut |_, sum, item| {
                reducer(sum, item)
            })
        })
    }
}
