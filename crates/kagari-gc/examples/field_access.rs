//! Warm scalar access characterization; registration/allocation/collection are untimed.

use kagari_gc::{
    access::ScalarField,
    dynamic::DynamicLayoutBuilder,
    error::GcError,
    heap::{Heap, Phase},
};
use std::{env, hint::black_box, time::Instant};

// SAFETY: the entire u64 is a scalar with no traced edges or indirect capacity.
const VALUE: ScalarField<u64, u64> = unsafe { ScalarField::new(|value| value, |value| value) };

const ITERATIONS: u64 = 50_000;
const WARMUPS: usize = 3;
const SAMPLES: usize = 11;

#[derive(Clone, Copy)]
enum Operation {
    Read,
    Write,
    Increment,
}

impl Operation {
    fn initial(self) -> u64 {
        match self {
            Self::Read => 7,
            Self::Write | Self::Increment => 0,
        }
    }

    fn expected(self) -> u64 {
        match self {
            Self::Read => 7 * ITERATIONS,
            Self::Write => ITERATIONS - 1,
            Self::Increment => ITERATIONS,
        }
    }
}

#[derive(Clone, Copy)]
enum Case {
    Plain(Operation),
    Typed(Operation),
    Dynamic(Operation),
    BorrowEach,
    BorrowBatch,
    DynamicBorrowEach,
    DynamicBorrowBatch,
    TypedAlternating,
    DynamicAlternating,
}

const CASES: [(Case, &str); 11] = [
    (Case::Plain(Operation::Read), "plain_read"),
    (Case::Plain(Operation::Write), "plain_write"),
    (Case::Plain(Operation::Increment), "plain_increment"),
    (Case::Typed(Operation::Read), "typed_read"),
    (Case::Typed(Operation::Write), "typed_write"),
    (Case::Typed(Operation::Increment), "typed_increment"),
    (Case::Dynamic(Operation::Read), "dynamic_read"),
    (Case::Dynamic(Operation::Write), "dynamic_write"),
    (Case::Dynamic(Operation::Increment), "dynamic_increment"),
    (Case::BorrowEach, "typed_borrow_each_increment"),
    (Case::BorrowBatch, "typed_borrow_batch_increment"),
];

const DYNAMIC_BORROW_CASES: [(Case, &str); 2] = [
    (Case::DynamicBorrowEach, "dynamic_borrow_each_increment"),
    (Case::DynamicBorrowBatch, "dynamic_borrow_batch_increment"),
];

struct Measurement {
    nanoseconds: u128,
    checksum: u64,
}

const ALTERNATING_CASES: [(Case, &str); 2] = [
    (Case::TypedAlternating, "typed_alternating_increment"),
    (Case::DynamicAlternating, "dynamic_alternating_increment"),
];

fn alternating(dynamic: bool) -> Result<Measurement, GcError> {
    let mut heap = Heap::default();
    let mut builder = DynamicLayoutBuilder::new();
    let field = builder.scalar::<u64>();
    let layout = heap.access()?.register_dynamic(builder)?;
    let field = layout.scalar(field)?;
    let nanoseconds = heap.scope(|cx| {
        let mut typed = Vec::new();
        let mut records = Vec::new();
        for _ in 0..32 {
            if dynamic {
                records.push(cx.alloc_dynamic(&layout)?);
            } else {
                typed.push(cx.alloc_leaf(0u64)?);
            }
        }
        let start = Instant::now();
        if dynamic {
            for i in 0..ITERATIONS {
                let object = black_box(records[black_box(i as usize & 31)]);
                let value = field.get(cx, object)?;
                field.set(cx, object, value + 1)?;
            }
        } else {
            for i in 0..ITERATIONS {
                let object = black_box(typed[black_box(i as usize & 31)]);
                let value = VALUE.get(cx, object);
                VALUE.set(cx, object, value + 1);
            }
        }
        let nanoseconds = start.elapsed().as_nanos();
        for i in 0..32 {
            let actual = if dynamic {
                field.get(cx, records[i])?
            } else {
                VALUE.get(cx, typed[i])
            };
            assert_eq!(
                actual,
                ITERATIONS / 32 + u64::from((i as u64) < ITERATIONS % 32)
            );
        }
        Ok(nanoseconds)
    })?;
    assert_eq!(heap.stats().objects, 32);
    assert_eq!(heap.stats().root_registrations, 0);
    heap.collect()?;
    assert_eq!(heap.stats().objects, 0);
    assert_eq!(heap.stats().backing_bytes, 0);
    Ok(Measurement {
        nanoseconds,
        checksum: ITERATIONS,
    })
}

fn plain(operation: Operation) -> Measurement {
    let mut value = Box::new(operation.initial());
    let mut sum = 0;
    let start = Instant::now();
    match operation {
        Operation::Read => {
            for _ in 0..ITERATIONS {
                sum += *black_box(&*value);
            }
        }
        Operation::Write => {
            for index in 0..ITERATIONS {
                *black_box(&mut *value) = black_box(index);
            }
        }
        Operation::Increment => {
            for _ in 0..ITERATIONS {
                *black_box(&mut *value) += 1;
            }
        }
    }
    let nanoseconds = start.elapsed().as_nanos();
    let checksum = match operation {
        Operation::Read => sum,
        Operation::Write | Operation::Increment => *value,
    };
    assert_eq!(checksum, operation.expected());
    Measurement {
        nanoseconds,
        checksum,
    }
}

fn typed(case: Case) -> Result<Measurement, GcError> {
    let operation = match case {
        Case::Typed(operation) => operation,
        Case::BorrowEach | Case::BorrowBatch => Operation::Increment,
        _ => unreachable!("typed measurement"),
    };
    let mut heap = Heap::default();
    let result = heap.scope(|cx| {
        let object = cx.alloc_leaf(operation.initial())?;
        let mut sum = 0;
        let start = Instant::now();
        match case {
            Case::Typed(Operation::Read) => {
                for _ in 0..ITERATIONS {
                    sum += VALUE.get(cx, black_box(object));
                }
            }
            Case::Typed(Operation::Write) => {
                for index in 0..ITERATIONS {
                    VALUE.set(cx, black_box(object), black_box(index));
                }
            }
            Case::Typed(Operation::Increment) => {
                for _ in 0..ITERATIONS {
                    let object = black_box(object);
                    let value = VALUE.get(cx, object);
                    VALUE.set(cx, object, value + 1);
                }
            }
            Case::BorrowEach => {
                for _ in 0..ITERATIONS {
                    let value = VALUE.borrow_mut(cx, black_box(object));
                    *black_box(&mut *value) += 1;
                }
            }
            Case::BorrowBatch => {
                let value = VALUE.borrow_mut(cx, black_box(object));
                for _ in 0..ITERATIONS {
                    *black_box(&mut *value) += 1;
                }
            }
            _ => unreachable!("typed measurement"),
        }
        let nanoseconds = start.elapsed().as_nanos();
        let checksum = match operation {
            Operation::Read => sum,
            Operation::Write | Operation::Increment => VALUE.get(cx, object),
        };
        assert_eq!(checksum, operation.expected());
        Ok(Measurement {
            nanoseconds,
            checksum,
        })
    })?;
    verify_cleanup(&mut heap)?;
    Ok(result)
}

fn dynamic(operation: Operation) -> Result<Measurement, GcError> {
    let mut heap = Heap::default();
    let mut builder = DynamicLayoutBuilder::new();
    let field = builder.scalar::<u64>();
    let layout = heap.access()?.register_dynamic(builder)?;
    let field = layout.scalar(field)?;
    let result = heap.scope(|cx| {
        let object = cx.alloc_dynamic(&layout)?;
        field.set(cx, object, operation.initial())?;
        let mut sum = 0;
        let start = Instant::now();
        match operation {
            Operation::Read => {
                for _ in 0..ITERATIONS {
                    sum += field.get(cx, black_box(object))?;
                }
            }
            Operation::Write => {
                for index in 0..ITERATIONS {
                    field.set(cx, black_box(object), black_box(index))?;
                }
            }
            Operation::Increment => {
                for _ in 0..ITERATIONS {
                    let object = black_box(object);
                    let value = field.get(cx, object)?;
                    field.set(cx, object, value + 1)?;
                }
            }
        }
        let nanoseconds = start.elapsed().as_nanos();
        let checksum = match operation {
            Operation::Read => sum,
            Operation::Write | Operation::Increment => field.get(cx, object)?,
        };
        assert_eq!(checksum, operation.expected());
        Ok(Measurement {
            nanoseconds,
            checksum,
        })
    })?;
    verify_cleanup(&mut heap)?;
    Ok(result)
}

fn verify_cleanup(heap: &mut Heap) -> Result<(), GcError> {
    let before = heap.stats();
    assert_eq!(before.phase, Phase::Idle);
    assert_eq!(before.completed_cycles, 0);
    assert_eq!(before.root_registrations, 0);
    assert_eq!(before.objects, 1);
    heap.collect()?;
    assert_eq!(heap.stats().objects, 0);
    assert_eq!(heap.stats().backing_bytes, 0);
    Ok(())
}

fn dynamic_borrow(batch: bool) -> Result<Measurement, GcError> {
    let mut heap = Heap::default();
    let mut builder = DynamicLayoutBuilder::new();
    let field = builder.scalar::<u64>();
    let layout = heap.access()?.register_dynamic(builder)?;
    let field = layout.scalar(field)?;
    let result = heap.scope(|cx| {
        let object = cx.alloc_dynamic(&layout)?;
        let start = Instant::now();
        if batch {
            let value = field.borrow_mut(cx, black_box(object))?;
            for _ in 0..ITERATIONS {
                *black_box(&mut *value) += 1;
            }
        } else {
            for _ in 0..ITERATIONS {
                let value = field.borrow_mut(cx, black_box(object))?;
                *black_box(&mut *value) += 1;
            }
        }
        let nanoseconds = start.elapsed().as_nanos();
        let checksum = field.get(cx, object)?;
        assert_eq!(checksum, ITERATIONS);
        Ok(Measurement {
            nanoseconds,
            checksum,
        })
    })?;
    verify_cleanup(&mut heap)?;
    Ok(result)
}

fn main() -> Result<(), GcError> {
    let process: usize = env::args()
        .nth(1)
        .map(|value| value.parse().expect("numeric process index"))
        .unwrap_or(0);
    // Keep the original eleven-row ordering for paired baseline/candidate runs.
    let cases = if env::args().nth(2).as_deref() == Some("--alternating") {
        ALTERNATING_CASES.as_slice()
    } else if env::args().nth(2).as_deref() == Some("--dynamic-borrow") {
        DYNAMIC_BORROW_CASES.as_slice()
    } else {
        CASES.as_slice()
    };
    println!("process,phase,sample,case,iterations,elapsed_ns,checksum");
    for sample in 0..WARMUPS + SAMPLES {
        let phase = if sample < WARMUPS {
            "warmup"
        } else {
            "measure"
        };
        for position in 0..cases.len() {
            let index = (position + process + sample) % cases.len();
            let (case, name) = cases[index];
            let measurement = match case {
                Case::Plain(operation) => plain(operation),
                Case::Dynamic(operation) => dynamic(operation)?,
                Case::DynamicBorrowEach => dynamic_borrow(false)?,
                Case::DynamicBorrowBatch => dynamic_borrow(true)?,
                Case::TypedAlternating => alternating(false)?,
                Case::DynamicAlternating => alternating(true)?,
                _ => typed(case)?,
            };
            println!(
                "{process},{phase},{sample},{name},{ITERATIONS},{},{}",
                measurement.nanoseconds, measurement.checksum
            );
        }
    }
    Ok(())
}
