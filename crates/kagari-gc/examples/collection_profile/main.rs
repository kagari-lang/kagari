//! Collection stage characterization; transition slices are reported separately.

// Share the reviewed root adapter rather than duplicating its tracing protocol.
#[allow(dead_code)] // Shared adapter operations are exercised by its owning example/tests.
#[path = "../root_access/frames.rs"]
mod frames;

use frames::Frames;
use kagari_gc::{
    dynamic::FieldType,
    error::GcError,
    heap::{HeapConfig, Phase},
    object::AnyObject,
    root_source::RootDriver,
    scan::WorkBudget,
};
use std::time::Instant;

fn measure(sample: usize, case: &str, count: usize) -> Result<(), GcError> {
    let mut driver = RootDriver::<Frames<AnyObject>>::new(HeapConfig::default())?;
    let layout = driver.access()?.register_dynamic(&[FieldType::Reference])?;
    let next = layout.reference(0)?;
    let chain = case == "chain";
    let duplicate = case == "duplicate";
    let id = driver.scope(|cx, roots| {
        let id = Frames::enter(roots, cx, if chain { 1 } else { count })?;
        let mut previous = None;
        for i in 0..count {
            let object = if chain {
                let object = cx.alloc_dynamic(&layout)?;
                next.set(cx, object, previous)?;
                object
            } else if duplicate && previous.is_some() {
                previous.unwrap()
            } else {
                cx.alloc_leaf(i as u64)?.erase()
            };
            Frames::with_frame(roots, cx, id, |frame| {
                frame.set(if chain { 0 } else { i }, Some(object))
            })?;
            previous = Some(object);
        }
        Ok(id)
    })?;
    let mut ns = [0u128; 4];
    let mut work = [0usize; 4];
    let mut steps = [0usize; 4];
    loop {
        let before = driver.stats().phase;
        let report = driver.step(WorkBudget::NORMAL)?;
        let category = match (before, report.phase) {
            (Phase::Idle | Phase::Roots, Phase::Roots) => 0,
            (Phase::Mark, Phase::Mark) => 1,
            (Phase::Sweep, Phase::Sweep | Phase::Idle) => 2,
            _ => 3,
        };
        ns[category] += report.elapsed.as_nanos();
        work[category] += report.work;
        steps[category] += 1;
        if report.completed_cycles != 0 {
            break;
        }
    }
    assert_eq!(driver.stats().objects, if duplicate { 1 } else { count });
    for (i, name) in ["roots", "mark", "sweep", "transition"]
        .into_iter()
        .enumerate()
    {
        println!(
            "{sample},{case},{count},{name},{},{},{}",
            ns[i], work[i], steps[i]
        );
    }
    let start = Instant::now();
    let report = driver.collect()?;
    println!(
        "{sample},{case},{count},full,{},{},0",
        start.elapsed().as_nanos(),
        report.work
    );
    driver.scope(|cx, roots| Frames::leave(roots, cx, id))?;
    driver.collect()?;
    assert_eq!(driver.stats().objects, 0);
    assert_eq!(driver.stats().backing_bytes, 0);
    Ok(())
}

fn main() -> Result<(), GcError> {
    println!("sample,case,count,stage,elapsed_ns,work,steps");
    for sample in 0..14 {
        for case in ["leaves", "duplicate", "chain"] {
            measure(sample, case, 8192)?;
        }
    }
    Ok(())
}
