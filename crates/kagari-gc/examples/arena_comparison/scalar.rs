use gc_arena::{Arena, Gc, Rootable};
use kagari_gc::{access::ScalarField, error::GcError, heap::Heap};
use std::{
    cell::{Cell, RefCell},
    hint::black_box,
    time::Instant,
};

use crate::{Engine, ITERATIONS, elapsed};

// SAFETY: identity projections of a scalar cannot change tracing or capacity.
const VALUE: ScalarField<u64, u64> = unsafe { ScalarField::new(|v| v, |v| v) };

#[derive(Clone, Copy)]
pub enum Operation {
    Read,
    Write,
    Increment,
    BorrowEach,
    BorrowBatch,
}

impl Operation {
    pub const ALL: [Self; 5] = [
        Self::Read,
        Self::Write,
        Self::Increment,
        Self::BorrowEach,
        Self::BorrowBatch,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Self::Read => "scalar_read",
            Self::Write => "scalar_write",
            Self::Increment => "scalar_increment",
            Self::BorrowEach => "borrow_each_increment",
            Self::BorrowBatch => "borrow_batch_increment",
        }
    }

    fn initial(self) -> u64 {
        if matches!(self, Self::Read) { 7 } else { 0 }
    }

    fn expected(self) -> u64 {
        match self {
            Self::Read => 7 * ITERATIONS as u64,
            Self::Write => ITERATIONS as u64 - 1,
            _ => ITERATIONS as u64,
        }
    }
}

pub fn measure(engine: Engine, operation: Operation) -> Result<u128, GcError> {
    match engine {
        Engine::Kagari => kagari(operation),
        Engine::Arena => Ok(arena(operation)),
    }
}

fn kagari(operation: Operation) -> Result<u128, GcError> {
    let mut heap = Heap::default();
    let ns = heap.scope(|cx| {
        let object = cx.alloc_leaf(operation.initial())?;
        let mut sum = 0;
        let start = Instant::now();
        match operation {
            Operation::Read => {
                for _ in 0..ITERATIONS {
                    sum += VALUE.get(cx, black_box(object));
                }
            }
            Operation::Write => {
                for i in 0..ITERATIONS {
                    VALUE.set(cx, black_box(object), black_box(i as u64));
                }
            }
            Operation::Increment => {
                for _ in 0..ITERATIONS {
                    let object = black_box(object);
                    let value = VALUE.get(cx, object);
                    VALUE.set(cx, object, value + 1);
                }
            }
            Operation::BorrowEach => {
                for _ in 0..ITERATIONS {
                    let value = VALUE.borrow_mut(cx, black_box(object));
                    *black_box(&mut *value) += 1;
                }
            }
            Operation::BorrowBatch => {
                let value = VALUE.borrow_mut(cx, black_box(object));
                for _ in 0..ITERATIONS {
                    *black_box(&mut *value) += 1;
                }
            }
        }
        let ns = elapsed(start);
        let actual = if matches!(operation, Operation::Read) {
            sum
        } else {
            VALUE.get(cx, object)
        };
        assert_eq!(actual, operation.expected());
        Ok(ns)
    })?;
    assert_eq!(heap.stats().objects, 1);
    assert_eq!(heap.stats().root_registrations, 0);
    heap.collect()?;
    assert_eq!(heap.stats().objects, 0);
    assert_eq!(heap.stats().backing_bytes, 0);
    Ok(ns)
}

fn arena(operation: Operation) -> u128 {
    // Both are safe static scalar Collect implementations: no graph write barrier.
    let mut arena = Arena::<Rootable![()]>::new(|_| ());
    let ns = arena.mutate(|mc, _| {
        if matches!(operation, Operation::BorrowEach | Operation::BorrowBatch) {
            let object = Gc::new(mc, RefCell::new(0u64));
            let start = Instant::now();
            if matches!(operation, Operation::BorrowEach) {
                for _ in 0..ITERATIONS {
                    let object = black_box(object);
                    let mut value = object.borrow_mut();
                    *black_box(&mut *value) += 1;
                }
            } else {
                let object = black_box(object);
                let mut value = object.borrow_mut();
                for _ in 0..ITERATIONS {
                    *black_box(&mut *value) += 1;
                }
            }
            let ns = elapsed(start);
            assert_eq!(*object.borrow(), operation.expected());
            ns
        } else {
            let object = Gc::new(mc, Cell::new(operation.initial()));
            let mut sum = 0;
            let start = Instant::now();
            match operation {
                Operation::Read => {
                    for _ in 0..ITERATIONS {
                        sum += black_box(object).get();
                    }
                }
                Operation::Write => {
                    for i in 0..ITERATIONS {
                        black_box(object).set(black_box(i as u64));
                    }
                }
                Operation::Increment => {
                    for _ in 0..ITERATIONS {
                        let object = black_box(object);
                        object.set(object.get() + 1);
                    }
                }
                _ => unreachable!("Cell operations"),
            }
            let ns = elapsed(start);
            let actual = if matches!(operation, Operation::Read) {
                sum
            } else {
                object.get()
            };
            assert_eq!(actual, operation.expected());
            ns
        }
    });
    assert_eq!(arena.metrics().total_gc_count(), 1);
    arena.finish_cycle();
    assert_eq!(arena.metrics().total_gc_count(), 0);
    ns
}
