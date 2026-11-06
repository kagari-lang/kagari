//! A safe business API over the small unsafe representation/provider kernel.

mod measure;

use kagari_gc::{
    access::{Access, EdgeField, ScalarField, Scope},
    error::GcError,
    heap::Heap,
    object::{Edge, Gc, GcObject},
    root::{Root, RootSet},
    scan::{EdgeVisitor, ScanBudget, ScanKind, ScanStatus},
};
use std::env;

struct Entry {
    value: i64,
    next: Option<Edge<Entry>>,
}

impl Entry {
    // SAFETY: plain scalar, independent of tracing/charge; both projections name it.
    const VALUE: ScalarField<Self, i64> =
        unsafe { ScalarField::new(|entry| &entry.value, |entry| &mut entry.value) };

    // SAFETY: one fixed traced position, no coupled invariants or owned capacity.
    const NEXT: EdgeField<Self, Self> =
        unsafe { EdgeField::new(|entry| &entry.next, |entry| &mut entry.next) };

    fn alloc<'gc>(
        cx: &mut Scope<'gc>,
        value: i64,
        next: Option<Gc<'gc, Self>>,
    ) -> Result<Gc<'gc, Self>, GcError> {
        // SAFETY: all edges come from scope-protected sources and stay in the
        // fully initialized traced result; no owned carrier escapes this builder.
        unsafe {
            cx.alloc_with(|mut init| {
                Ok(Self {
                    value,
                    next: next.map(|next| init.edge_scoped(next)).transpose()?,
                })
            })
        }
    }
}

// SAFETY: all references are visited and writes use the checked field providers.
// Fixed scan positions, pointer-free state and trivial Drop need no hooks.
unsafe impl GcObject for Entry {
    type ScanState = bool;

    const SCAN_KIND: ScanKind = ScanKind::Bounded { max_work: 1 };

    fn begin_scan(&self) -> bool {
        false
    }

    fn scan(
        &self,
        state: &mut bool,
        budget: &mut ScanBudget,
        visitor: &mut EdgeVisitor<'_>,
    ) -> Result<ScanStatus, GcError> {
        if !*state {
            if !budget.take(1) {
                return Ok(ScanStatus::Pending);
            }
            if let Some(edge) = &self.next {
                // SAFETY: the owning entry retains this initialized source edge.
                if unsafe { visitor.try_visit_stored(edge, budget)? } == ScanStatus::Pending {
                    return Ok(ScanStatus::Pending);
                }
            }
            *state = true;
        }
        Ok(ScanStatus::Complete)
    }
}

struct List {
    // One registration for the list's lifetime, including while it is empty.
    head: RootSet,
}

struct Values<'loan, 'gc> {
    cx: &'loan Scope<'gc>,
    current: Option<Gc<'gc, Entry>>,
}

impl Iterator for Values<'_, '_> {
    type Item = i64;

    fn next(&mut self) -> Option<Self::Item> {
        let current = self.current.take()?;
        let value = Entry::VALUE.get(self.cx, current);
        self.current = Entry::NEXT.get(self.cx, current);
        Some(value)
    }
}

struct ValuesMut<'loan, 'gc> {
    cx: &'loan mut Scope<'gc>,
    current: Option<Gc<'gc, Entry>>,
}

impl ValuesMut<'_, '_> {
    fn next(&mut self) -> Option<&mut i64> {
        let current = self.current.take()?;
        self.current = Entry::NEXT.get(self.cx, current);
        Some(Entry::VALUE.borrow_mut(self.cx, current))
    }
}

impl List {
    fn new(access: &mut Access<'_>) -> Result<Self, GcError> {
        Ok(Self {
            head: access.create_roots(1)?,
        })
    }

    fn head<'gc>(&self, cx: &Scope<'gc>) -> Result<Option<Gc<'gc, Entry>>, GcError> {
        match cx.get_root(&self.head, 0) {
            Ok(head) => Ok(Some(head)),
            // This private pack always has exactly one slot, so Index means
            // an empty head. Heap/state/type errors must still propagate.
            Err(GcError::Index) => Ok(None),
            Err(error) => Err(error),
        }
    }

    /// Read one scalar per next call; no eager collection or per-hop Root.
    fn values<'loan, 'gc>(&self, cx: &'loan Scope<'gc>) -> Result<Values<'loan, 'gc>, GcError> {
        Ok(Values {
            current: self.head(cx)?,
            cx,
        })
    }

    /// Lend one mutable scalar per next call. Each field loan must end before
    /// advancing; returning Iterator<Item=&mut i64> would not enforce that rule.
    fn values_mut<'loan, 'gc>(
        &self,
        cx: &'loan mut Scope<'gc>,
    ) -> Result<ValuesMut<'loan, 'gc>, GcError> {
        Ok(ValuesMut {
            current: self.head(cx)?,
            cx,
        })
    }

    fn push_front(&mut self, cx: &mut Scope<'_>, value: i64) -> Result<(), GcError> {
        let next = self.head(cx)?;
        let head = Entry::alloc(cx, value, next)?;
        cx.set_root(&self.head, 0, Some(head))
    }

    fn pop_front(&mut self, cx: &mut Scope<'_>) -> Result<Option<Root<Entry>>, GcError> {
        let Some(head) = self.head(cx)? else {
            return Ok(None);
        };
        let next = Entry::NEXT.get(cx, head);
        // Reserve the escaping result before changing the list. The successor
        // reuses the head slot; it needs no independent registration.
        let result = cx.retain(head)?;
        cx.set_root(&self.head, 0, next)?;
        Entry::NEXT.set(cx, head, None);
        Ok(Some(result))
    }
}

fn main() -> Result<(), GcError> {
    if env::args().any(|argument| argument == "--measure") {
        return measure::run();
    }
    let mut heap = Heap::default();
    let mut list = List::new(&mut heap.access()?)?;
    heap.scope(|cx| {
        for value in 0..10 {
            list.push_front(cx, value)?;
        }
        Ok(())
    })?;
    heap.collect()?;
    heap.scope(|cx| {
        let mut visited = 0;
        for (index, value) in list.values(cx)?.enumerate() {
            assert_eq!(value, 9 - index as i64);
            visited += 1;
        }
        assert_eq!(visited, 10);
        let mut values = list.values_mut(cx)?;
        while let Some(value) = values.next() {
            *value += 100;
        }
        Ok(())
    })?;
    for expected in (0..10).rev() {
        heap.scope(|cx| {
            let entry = list.pop_front(cx)?.expect("nonempty list");
            let entry = cx.get(&entry)?;
            assert_eq!(Entry::VALUE.get(cx, entry), expected + 100);
            Ok(())
        })?;
        heap.collect()?;
    }
    heap.scope(|cx| {
        assert!(list.values(cx)?.next().is_none());
        assert!(list.pop_front(cx)?.is_none());
        Ok(())
    })?;
    heap.collect()?;
    assert_eq!(heap.stats().objects, 0);
    assert_eq!(heap.stats().backing_bytes, 0);
    println!("Linked-list lazy reads, lending edits, rooted pop and reclamation passed.");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{Entry, List};
    use kagari_gc::{
        error::GcError,
        heap::{Heap, HeapConfig},
    };
    use std::panic::{AssertUnwindSafe, catch_unwind};

    #[test]
    fn iteration_needs_no_spare_root_and_edits_survive_early_drop_and_unwind() -> Result<(), GcError>
    {
        let mut heap = Heap::new(HeapConfig {
            max_roots: 2,
            ..HeapConfig::default()
        })?;
        let mut list = List::new(&mut heap.access()?)?;
        for value in 0..4 {
            heap.scope(|cx| list.push_front(cx, value))?;
            heap.collect()?;
        }
        let full = heap.access()?.create_roots(0)?;
        heap.scope(|cx| {
            let mut values = list.values(cx)?;
            assert_eq!(values.next(), Some(3));
            // Independent read iterators can share the same scope.
            let mut another = list.values(cx)?;
            assert_eq!(another.next(), Some(3));
            assert_eq!(values.next(), Some(2));
            Ok(())
        })?;
        heap.scope(|cx| {
            let mut values = list.values_mut(cx)?;
            let first = values.next().expect("first");
            *first = 30;
            // Scalar edits commit immediately, including on early traversal exit.
            Ok(())
        })?;
        assert_eq!(heap.stats().root_registrations, 2);
        let unwound = catch_unwind(AssertUnwindSafe(|| -> Result<(), GcError> {
            heap.scope(|cx| {
                let mut values = list.values_mut(cx)?;
                let first = values.next().expect("first");
                *first = 31;
                panic!("application loop unwound");
            })
        }));
        assert!(unwound.is_err());
        heap.collect()?;
        let observed = heap.scope(|cx| Ok(list.values(cx)?.collect::<Vec<_>>()))?;
        assert_eq!(observed, vec![31, 2, 1, 0]);
        drop(full);
        drop(list);
        heap.collect()?;
        assert_eq!(heap.stats().objects, 0);
        assert_eq!(heap.stats().backing_bytes, 0);
        Ok(())
    }

    #[test]
    fn push_reuses_the_head_slot_and_pop_retains_only_its_result() -> Result<(), GcError> {
        let mut heap = Heap::new(HeapConfig {
            max_roots: 2,
            ..HeapConfig::default()
        })?;
        let mut list = List::new(&mut heap.access()?)?;
        let full = heap.access()?.create_roots(0)?;
        let mut foreign = Heap::default();
        assert!(matches!(
            foreign.scope(|cx| list.pop_front(cx)),
            Err(GcError::ForeignHeap)
        ));
        heap.scope(|cx| {
            assert!(list.values(cx)?.next().is_none());
            assert!(list.pop_front(cx)?.is_none());
            // No spare root registration and no collection between insertions.
            for value in 0..512 {
                list.push_front(cx, value)?;
            }
            assert!(matches!(
                list.pop_front(cx),
                Err(GcError::Limit("root registrations"))
            ));
            assert_eq!(
                list.values(cx)?.collect::<Vec<_>>(),
                (0..512).rev().collect::<Vec<_>>()
            );
            Ok(())
        })?;
        assert_eq!(heap.stats().root_registrations, 2);
        drop(full);
        heap.collect()?;
        assert_eq!(heap.stats().objects, 512);
        assert_eq!(heap.stats().root_registrations, 1);
        let popped = heap.scope(|cx| list.pop_front(cx))?.unwrap();
        heap.collect()?;
        heap.scope(|cx| {
            assert_eq!(list.values(cx)?.next(), Some(510));
            assert_eq!(Entry::VALUE.get(cx, cx.get(&popped)?), 511);
            Ok(())
        })?;
        // The popped node survives independently and does not retain the suffix.
        drop(list);
        heap.collect()?;
        assert_eq!(heap.stats().objects, 1);
        assert_eq!(heap.stats().root_registrations, 1);
        heap.scope(|cx| {
            let object = cx.get(&popped)?;
            assert_eq!(Entry::VALUE.get(cx, object), 511);
            assert!(Entry::NEXT.get(cx, object).is_none());
            Ok(())
        })?;
        drop(popped);
        heap.collect()?;
        assert_eq!(heap.stats().objects, 0);
        assert_eq!(heap.stats().root_registrations, 0);
        Ok(())
    }
}
