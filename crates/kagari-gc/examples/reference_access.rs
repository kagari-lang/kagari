//! Fixed reference writes across phases; excludes scope/root admission from timing.

use kagari_gc::{
    access::EdgeField,
    dynamic::DynamicLayoutBuilder,
    error::GcError,
    heap::{Heap, Phase},
    object::{Edge, GcObject},
    scan::{EdgeVisitor, ScanBudget, ScanKind, ScanStatus, WorkBudget},
};
use std::{hint::black_box, time::Instant};

struct Link(Option<Edge<u64>>);

impl Link {
    // SAFETY: the only field is an independent fixed-position traced edge.
    const NEXT: EdgeField<Self, u64> = unsafe { EdgeField::new(|v| &v.0, |v| &mut v.0) };
}

// SAFETY: all initialized edges are visited; fixed storage, charge and no callbacks.
unsafe impl GcObject for Link {
    type ScanState = ();

    const SCAN_KIND: ScanKind = ScanKind::Bounded { max_work: 1 };

    fn begin_scan(&self) {}

    fn scan(
        &self,
        _: &mut (),
        budget: &mut ScanBudget,
        visitor: &mut EdgeVisitor<'_>,
    ) -> Result<ScanStatus, GcError> {
        if !budget.take(1) {
            return Ok(ScanStatus::Pending);
        }
        if let Some(edge) = &self.0 {
            // SAFETY: the scanned owner retains this same-heap target.
            return unsafe { visitor.try_visit_stored(edge, budget) };
        }
        Ok(ScanStatus::Complete)
    }
}

const ITERATIONS: usize = 50_000;

fn measure(dynamic: bool, phase: Phase) -> Result<u128, GcError> {
    let mut heap = Heap::default();
    let descriptor = heap.access()?.register::<Link>()?;
    let mut builder = DynamicLayoutBuilder::new();
    let field = builder.reference();
    let layout = heap.access()?.register_dynamic(builder)?;
    let field = layout.reference(field)?;
    let roots = heap.access()?.create_roots(2)?;
    let (a, b, owners) = {
        let mut access = heap.access()?;
        let a = access.alloc_leaf(7u64)?;
        let b = access.alloc_leaf(11u64)?;
        let owners = (0..64)
            .map(|_| {
                // SAFETY: empty edge; checked fixed-field operations own later writes.
                unsafe { access.alloc_with(&descriptor, |_| Ok(Link(None))) }
            })
            .collect::<Result<Vec<_>, _>>()?;
        (a, b, owners)
    };
    heap.scope(|cx| {
        let object = cx.alloc_dynamic(&layout)?;
        cx.set_root(&roots, 0, Some(object))?;
        let a = cx.get(&a)?;
        field.set(cx, object, Some(a.erase()))
    })?;
    if phase != Phase::Idle {
        for _ in 0..10000 {
            heap.step(WorkBudget::new(8))?;
            if heap.stats().phase == phase {
                break;
            }
        }
    }
    assert_eq!(heap.stats().phase, phase);
    let ns = heap.scope(|cx| {
        let targets = [cx.get(&a)?, cx.get(&b)?];
        let ns = if dynamic {
            let object = cx.get_root(&roots, 0)?;
            let targets = [targets[0].erase(), targets[1].erase()];
            let start = Instant::now();
            for i in 0..ITERATIONS {
                field.set(
                    black_box(&mut *cx),
                    black_box(object),
                    Some(black_box(targets[i & 1])),
                )?;
            }
            let ns = start.elapsed().as_nanos();
            let target = field.get(cx, object)?.unwrap();
            let target = cx.downcast::<u64>(target)?;
            assert_eq!(cx.read(target, |v| *v), 11);
            ns
        } else {
            let object = cx.get(&owners[0])?;
            let start = Instant::now();
            for i in 0..ITERATIONS {
                Link::NEXT.set(
                    black_box(&mut *cx),
                    black_box(object),
                    Some(black_box(targets[i & 1])),
                );
            }
            let ns = start.elapsed().as_nanos();
            let target = Link::NEXT.get(cx, object).unwrap();
            assert_eq!(cx.read(target, |v| *v), 11);
            ns
        };
        Ok(ns)
    })?;
    heap.collect()?;
    assert_eq!(heap.stats().objects, 67);
    drop((a, b, owners, roots));
    heap.collect()?;
    assert_eq!(heap.stats().objects, 0);
    assert_eq!(heap.stats().backing_bytes, 0);
    Ok(ns)
}

fn main() -> Result<(), GcError> {
    println!("sample,kind,phase,count,elapsed_ns");
    for sample in 0..14 {
        for phase in [Phase::Idle, Phase::Mark, Phase::Sweep] {
            for dynamic in [false, true] {
                let ns = measure(dynamic, phase)?;
                println!(
                    "{sample},{},{phase:?},{ITERATIONS},{ns}",
                    if dynamic { "dynamic" } else { "typed" }
                );
            }
        }
    }
    Ok(())
}
