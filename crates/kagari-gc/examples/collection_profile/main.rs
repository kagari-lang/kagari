//! Collection stage characterization; transition slices are reported separately.

// Share the reviewed root adapter rather than duplicating its tracing protocol.
#[allow(dead_code)] // Shared adapter operations are exercised by its owning example/tests.
#[path = "../root_access/frames.rs"]
mod frames;

use frames::Frames;
use kagari_gc::{
    dynamic::DynamicLayoutBuilder,
    error::GcError,
    heap::{HeapConfig, Phase},
    object::AnyObject,
    root_source::RootDriver,
    scan::WorkBudget,
};
use std::time::Instant;

struct Row {
    record: &'static str,
    sample: usize,
    case: &'static str,
    count: usize,
    stage: &'static str,
    elapsed_ns: u128,
    work: usize,
    steps: usize,
    maintenance_ns: u128,
}

fn measure(
    sample: usize,
    case: &'static str,
    count: usize,
    rows: &mut Vec<Row>,
) -> Result<(), GcError> {
    let mut driver = RootDriver::<Frames<AnyObject>>::new(HeapConfig::default())?;
    let mut builder = DynamicLayoutBuilder::new();
    let next = builder.reference();
    let layout = driver.access()?.register_dynamic(builder)?;
    let next = layout.reference(next)?;
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
    let stages = ["roots", "mark", "sweep", "transition"];
    loop {
        let before = driver.stats().phase;
        let report = driver.step(WorkBudget::NORMAL)?;
        assert!(report.work <= 1024 && report.retired_owners <= 32);
        assert_eq!(report.required_budget, None);
        assert!(report.work > 0 || report.phase != before || report.completed_cycles > 0);
        let category = match (before, report.phase) {
            (Phase::Idle | Phase::Roots, Phase::Roots) => 0,
            (Phase::Mark, Phase::Mark) => 1,
            (Phase::Sweep, Phase::Sweep | Phase::Idle) => 2,
            _ => 3,
        };
        ns[category] += report.elapsed.as_nanos();
        work[category] += report.work;
        steps[category] += 1;
        rows.push(Row {
            record: "step",
            sample,
            case,
            count,
            stage: stages[category],
            elapsed_ns: report.elapsed.as_nanos(),
            work: report.work,
            steps: 1,
            maintenance_ns: 0,
        });
        if report.completed_cycles != 0 {
            assert_eq!(report.completed_cycles, 1);
            break;
        }
    }
    assert_eq!(driver.stats().phase, Phase::Idle);
    assert_eq!(driver.stats().completed_cycles, 1);
    assert!(work.iter().sum::<usize>() > 0);
    assert_eq!(driver.stats().objects, if duplicate { 1 } else { count });
    for (i, stage) in stages.into_iter().enumerate() {
        rows.push(Row {
            record: "stage",
            sample,
            case,
            count,
            stage,
            elapsed_ns: ns[i],
            work: work[i],
            steps: steps[i],
            maintenance_ns: 0,
        });
    }
    let start = Instant::now();
    let report = driver.collect()?;
    let elapsed_ns = start.elapsed().as_nanos();
    assert_eq!(report.completed_cycles, 1);
    assert!(report.work > 0);
    assert_eq!(driver.stats().objects, if duplicate { 1 } else { count });
    rows.push(Row {
        record: "full",
        sample,
        case,
        count,
        stage: "full",
        elapsed_ns,
        work: report.work,
        steps: 0,
        maintenance_ns: report.maintenance_elapsed.as_nanos(),
    });
    driver.scope(|cx, roots| Frames::leave(roots, cx, id))?;
    driver.collect()?;
    assert_eq!(driver.stats().objects, 0);
    assert_eq!(driver.stats().backing_bytes, 0);
    assert_eq!(driver.stats().indirect_bytes, 0);
    assert_eq!(driver.stats().phase, Phase::Idle);
    Ok(())
}

fn main() -> Result<(), GcError> {
    let mut rows = Vec::new();
    for sample in 0..14 {
        for case in ["leaves", "duplicate", "chain"] {
            measure(sample, case, 8192, &mut rows)?;
        }
    }
    println!("record,sample,case,count,stage,elapsed_ns,work,steps,maintenance_ns");
    for row in rows {
        println!(
            "{},{},{},{},{},{},{},{},{}",
            row.record,
            row.sample,
            row.case,
            row.count,
            row.stage,
            row.elapsed_ns,
            row.work,
            row.steps,
            row.maintenance_ns
        );
    }
    Ok(())
}
