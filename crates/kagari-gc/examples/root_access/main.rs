//! Compare existing RootSet transport with directly scanned frame storage.
//! Setup and whole-cycle collection are timed separately from the copy loop.

mod frames;

use frames::Frames;
use kagari_gc::{
    error::GcError,
    heap::{Heap, HeapConfig, Phase},
    root_source::RootDriver,
    scan::WorkBudget,
};
use std::{hint::black_box, time::Instant};

const COPIES: usize = 50_000;
const WARMUPS: usize = 3;
const SAMPLES: usize = 11;

struct Measurement {
    setup_ns: u128,
    copy_ns: u128,
    collect_ns: u128,
    work: usize,
}

fn pack(slots: usize, marking: bool) -> Result<Measurement, GcError> {
    let start = Instant::now();
    let mut heap = Heap::default();
    let pack = heap.access()?.create_roots(slots)?;
    heap.scope(|cx| {
        let a = cx.alloc_leaf(7u64)?;
        let b = cx.alloc_leaf(11u64)?;
        cx.set_root(&pack, 0, Some(a))?;
        cx.set_root(&pack, 2, Some(b))
    })?;
    let setup_ns = start.elapsed().as_nanos();
    if marking {
        heap.step(WorkBudget::new(8))?;
    }
    assert_eq!(
        heap.stats().phase,
        if marking { Phase::Roots } else { Phase::Idle }
    );
    let copy_ns = heap.scope(|cx| {
        let start = Instant::now();
        for i in 0..COPIES {
            let value = cx.get_root::<u64>(black_box(&pack), black_box((i & 1) * 2))?;
            cx.set_root(black_box(&pack), black_box(1), Some(black_box(value)))?;
        }
        let elapsed = start.elapsed().as_nanos();
        let value = cx.get_root::<u64>(&pack, 1)?;
        assert_eq!(cx.read(value, |value| *value), 11);
        Ok(elapsed)
    })?;
    let collection = heap.collect()?;
    assert_eq!(heap.stats().objects, 2);
    assert_eq!(heap.stats().root_registrations, 1);
    drop(pack);
    heap.collect()?;
    assert_eq!(heap.stats().objects, 0);
    assert_eq!(heap.stats().backing_bytes, 0);
    Ok(Measurement {
        setup_ns,
        copy_ns,
        collect_ns: collection.elapsed.as_nanos(),
        work: collection.work,
    })
}

fn direct(slots: usize, marking: bool) -> Result<Measurement, GcError> {
    let start = Instant::now();
    let mut driver = RootDriver::<Frames<u64>>::new(HeapConfig::default())?;
    let id = driver.scope(|cx, roots| {
        let a = cx.alloc_leaf(7u64)?;
        let b = cx.alloc_leaf(11u64)?;
        let id = Frames::enter(roots, cx, 3)?;
        Frames::with_frame(roots, cx, id, |frame| {
            frame.grow(slots);
            frame.set(0, Some(a))?;
            frame.set(2, Some(b))
        })?;
        Ok(id)
    })?;
    let setup_ns = start.elapsed().as_nanos();
    if marking {
        driver.step(WorkBudget::new(8))?;
    }
    assert_eq!(
        driver.stats().phase,
        if marking { Phase::Roots } else { Phase::Idle }
    );
    let copy_ns = driver.scope(|cx, roots| {
        let elapsed = Frames::with_frame(roots, cx, id, |frame| {
            let start = Instant::now();
            for i in 0..COPIES {
                black_box(&mut *frame).copy(black_box(1), black_box((i & 1) * 2))?;
            }
            Ok(start.elapsed().as_nanos())
        })?;
        let value = Frames::with_frame(roots, cx, id, |frame| frame.get(1))?.unwrap();
        assert_eq!(cx.read(value, |value| *value), 11);
        Ok(elapsed)
    })?;
    let collection = driver.collect()?;
    assert_eq!(driver.stats().objects, 2);
    assert_eq!(driver.stats().root_registrations, 0);
    driver.scope(|cx, roots| Frames::leave(roots, cx, id))?;
    driver.collect()?;
    assert_eq!(driver.stats().objects, 0);
    assert_eq!(driver.stats().backing_bytes, 0);
    Ok(Measurement {
        setup_ns,
        copy_ns,
        collect_ns: collection.elapsed.as_nanos(),
        work: collection.work,
    })
}

fn median(mut values: Vec<u128>) -> u128 {
    values.sort_unstable();
    values[values.len() / 2]
}

fn main() -> Result<(), GcError> {
    println!("mode,phase,slots,copy_ns_per_op,setup_ns,collect_ns,collect_work");
    for slots in [32, 1024, 16384] {
        for marking in [false, true] {
            let mut packs = Vec::new();
            let mut directs = Vec::new();
            for round in 0..WARMUPS + SAMPLES {
                // Alternate order to reduce systematic thermal/cache bias.
                let (p, d) = if round % 2 == 0 {
                    (pack(slots, marking)?, direct(slots, marking)?)
                } else {
                    let d = direct(slots, marking)?;
                    (pack(slots, marking)?, d)
                };
                if round >= WARMUPS {
                    packs.push(p);
                    directs.push(d);
                }
            }
            for (name, values) in [("root_set", packs), ("direct_frame", directs)] {
                println!(
                    "{name},{},{slots},{:.3},{},{},{}",
                    if marking { "marking" } else { "idle" },
                    median(values.iter().map(|m| m.copy_ns).collect()) as f64 / COPIES as f64,
                    median(values.iter().map(|m| m.setup_ns).collect()),
                    median(values.iter().map(|m| m.collect_ns).collect()),
                    median(values.iter().map(|m| m.work as u128).collect())
                );
            }
        }
    }
    Ok(())
}
