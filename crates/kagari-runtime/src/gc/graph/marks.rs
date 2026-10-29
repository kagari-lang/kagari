//! Marks live with physical owners, not in a second identity registry.
use std::{cell::Cell, collections::TryReserveError};

#[cfg(test)]
thread_local! {
    static INITIALIZED_WORDS: Cell<usize> = const { Cell::new(0) };
}

#[cfg(test)]
pub(crate) fn initialized_words() -> usize {
    INITIALIZED_WORDS.with(Cell::get)
}

/// Stable owner-local slot blocks, not payload addresses or cross-runtime IDs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Domain {
    Object,
    Struct,
    Moved,
    Environment,
    Group,
    Application,
    Interface,
    NativeApplication,
    InterfaceCall,
    InterfaceDescription,
    Program,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct WorkGroup {
    pub(crate) domain: Domain,
    pub(crate) block: usize,
}

/// Blocks are owned by the original slot store and cannot be removed while work
/// is pending. Generations remain in that store; a block job resolves each gray
/// slot's current full identity instead of storing a second generation registry.
#[derive(Debug, Default)]
pub(crate) struct MarkBlocks(Vec<PageMarks>);

impl MarkBlocks {
    pub(crate) fn prepare(&mut self, slot: usize) -> Result<(), TryReserveError> {
        let count = slot / 128 + 1;
        if count > self.0.len() {
            self.0.try_reserve(count - self.0.len())?;
            self.0.resize_with(count, PageMarks::default);
        }
        Ok(())
    }

    pub(crate) fn slot(&self, domain: Domain, slot: usize) -> MarkRef<'_> {
        self.0[slot / 128].group_slot(
            WorkGroup {
                domain,
                block: slot / 128,
            },
            slot % 128,
        )
    }

    pub(crate) fn next_gray(&self, block: usize, epoch: u64) -> (Option<usize>, bool) {
        let (slot, more) = self.0[block].next_gray(epoch);
        (slot.map(|slot| block * 128 + slot), more)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum Color {
    #[default]
    White,
    Gray,
    Black,
}

/// One checked epoch and color for a singleton owner (non-reused static cell).
#[derive(Debug, Default)]
pub(crate) struct OwnerMark {
    epoch: Cell<u64>,
    color: Cell<Color>,
    registered: Cell<bool>,
}

/// One epoch and two color bitmaps per 128 original-owner slots. No per-object
/// identity or generation is duplicated; payload backing may be released separately.
#[derive(Debug, Default)]
pub(crate) struct PageMarks {
    epoch: Cell<u64>,
    discovered: [Cell<u64>; 2],
    scanned: [Cell<u64>; 2],
    registered: Cell<bool>,
    queued: Cell<bool>,
}

/// Borrowed only for a checked owner operation; never stored in graph work.
#[derive(Clone, Copy)]
pub(crate) enum MarkRef<'a> {
    Owner(&'a OwnerMark),
    Page(&'a PageMarks, usize, WorkGroup),
}

impl OwnerMark {
    pub(crate) fn as_ref(&self) -> MarkRef<'_> {
        MarkRef::Owner(self)
    }
}

impl PageMarks {
    #[cfg(test)]
    pub(crate) fn slot(&self, index: usize) -> MarkRef<'_> {
        self.group_slot(
            WorkGroup {
                domain: Domain::Object,
                block: 0,
            },
            index,
        )
    }

    pub(in crate::gc) fn group_slot(&self, group: WorkGroup, index: usize) -> MarkRef<'_> {
        assert!(index < 128, "bounded slot block marks");
        MarkRef::Page(self, index, group)
    }

    pub(crate) fn next_gray(&self, epoch: u64) -> (Option<usize>, bool) {
        assert!(self.queued.replace(false), "queued mark block");
        if self.epoch.get() != epoch {
            return (None, false);
        }
        let mut gray = u128::from(self.discovered[0].get() & !self.scanned[0].get())
            | (u128::from(self.discovered[1].get() & !self.scanned[1].get()) << 64);
        if gray == 0 {
            return (None, false);
        }
        let first = gray.trailing_zeros() as usize;
        gray &= gray - 1;
        let more = gray != 0;
        self.queued.set(more);
        (Some(first), more)
    }
}

impl MarkRef<'_> {
    /// Caller proves the physical block has no remaining occupants. Old epoch
    /// bits need not be cleared, but current gray or queued work forbids removal.
    pub(in crate::gc) fn unregister_empty_group(self, epoch: u64) -> Option<bool> {
        let Self::Page(page, _, _) = self else {
            return None;
        };
        if page.queued.get()
            || (page.epoch.get() == epoch
                && page
                    .discovered
                    .iter()
                    .zip(&page.scanned)
                    .any(|(seen, scanned)| seen.get() & !scanned.get() != 0))
        {
            return None;
        }
        Some(page.registered.replace(false))
    }

    pub(crate) fn register(self) -> bool {
        !match self {
            Self::Owner(mark) => &mark.registered,
            Self::Page(mark, _, _) => &mark.registered,
        }
        .replace(true)
    }

    pub(crate) fn unregister_singleton(self) -> bool {
        match self {
            Self::Owner(mark) => mark.registered.replace(false),
            Self::Page(..) => false,
        }
    }

    pub(crate) fn queue_group(self) -> Option<(WorkGroup, bool)> {
        match self {
            Self::Owner(_) => None,
            Self::Page(mark, _, group) => Some((group, !mark.queued.replace(true))),
        }
    }

    pub(crate) fn color(self, epoch: u64) -> Color {
        match self {
            Self::Owner(mark) => {
                if mark.epoch.get() == epoch {
                    mark.color.get()
                } else {
                    Color::White
                }
            }
            Self::Page(page, slot, _) => {
                if page.epoch.get() != epoch {
                    return Color::White;
                }
                let bit = 1 << (slot % 64);
                if page.scanned[slot / 64].get() & bit != 0 {
                    Color::Black
                } else if page.discovered[slot / 64].get() & bit != 0 {
                    Color::Gray
                } else {
                    Color::White
                }
            }
        }
    }

    /// Work required before changing this mark in the selected epoch.
    pub(crate) fn initialization_work(self, epoch: u64) -> usize {
        match self {
            Self::Page(page, _, _) if page.epoch.get() != epoch => 4,
            _ => 0,
        }
    }

    /// Reset is lazy and bounded by four words. Collector callers must reserve
    /// initialization_work before entering; mutator publication pays its own work.
    pub(crate) fn set(self, epoch: u64, color: Color) {
        match self {
            Self::Owner(mark) => {
                mark.epoch.set(epoch);
                mark.color.set(color);
            }
            Self::Page(page, slot, _) => {
                let reset = page.epoch.get() != epoch;
                if reset {
                    for word in page.discovered.iter().chain(&page.scanned) {
                        word.set(0);
                        #[cfg(test)]
                        INITIALIZED_WORDS.with(|count| count.set(count.get() + 1));
                    }
                    page.epoch.set(epoch);
                }
                let bit = 1 << (slot % 64);
                let discovered = &page.discovered[slot / 64];
                let scanned = &page.scanned[slot / 64];
                discovered
                    .set((discovered.get() & !bit) | if color != Color::White { bit } else { 0 });
                scanned.set((scanned.get() & !bit) | if color == Color::Black { bit } else { 0 });
            }
        }
    }
}
