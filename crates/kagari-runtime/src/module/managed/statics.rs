//! Each version owns immutable bindings to shared persistent static cells.
use crate::module::managed::{InstallationState, cell::StaticCellObject};
use kagari_gc::{
    access::Scope,
    error::GcError,
    object::{Edge, Gc, GcObject, ObjectCharge},
    scan::{EdgeVisitor, ScanBudget, ScanKind, ScanStatus},
};

/// Staging creates another container, preserving existing cells and introducing
/// new cells without mutating the published graph. Bindings may alias a supplier.
#[derive(Debug)]
pub(crate) struct StaticState {
    installation: Edge<InstallationState>,
    bindings: Vec<Edge<StaticCellObject>>,
}

impl StaticState {
    pub(crate) fn alloc<'gc>(
        cx: &mut Scope<'gc>,
        installation: Gc<'gc, InstallationState>,
        cells: &[Gc<'gc, StaticCellObject>],
    ) -> Result<Gc<'gc, Self>, GcError> {
        for cell in cells {
            if !installation.ptr_eq(StaticCellObject::installation(cx, *cell)) {
                return Err(GcError::Representation);
            }
        }
        // SAFETY: all bindings are protected same-heap cells and imported only
        // through this initializer. Every initialized cell is scanned below.
        unsafe {
            cx.alloc_with(|mut init| {
                let mut bindings = Vec::new();
                bindings.try_reserve_exact(cells.len())?;
                for cell in cells {
                    bindings.push(init.edge_scoped(*cell)?);
                }
                Ok(Self {
                    installation: init.edge_scoped(installation)?,
                    bindings,
                })
            })
        }
    }

    pub(crate) fn installation<'gc>(
        cx: &Scope<'gc>,
        owner: Gc<'gc, Self>,
    ) -> Gc<'gc, InstallationState> {
        cx.follow(owner, |state| Some(&state.installation))
            .expect("static installation edge")
    }

    pub(crate) fn len(&self) -> usize {
        self.bindings.len()
    }

    pub(crate) fn get<'gc>(
        cx: &Scope<'gc>,
        owner: Gc<'gc, Self>,
        index: usize,
    ) -> Result<Gc<'gc, StaticCellObject>, GcError> {
        cx.follow(owner, |state| state.bindings.get(index))
            .ok_or(GcError::Index)
    }
}

// SAFETY: immutable initialized bindings are visited at stable numeric positions.
// A pending visit never advances its position; Drop only releases inert backing.
unsafe impl GcObject for StaticState {
    type ScanState = usize;

    const SCAN_KIND: ScanKind = ScanKind::Resumable;

    fn begin_scan(&self) -> usize {
        0
    }

    fn scan(
        &self,
        next: &mut usize,
        budget: &mut ScanBudget,
        visitor: &mut EdgeVisitor<'_>,
    ) -> Result<ScanStatus, GcError> {
        if *next == 0 {
            if !budget.take(1) {
                return Ok(ScanStatus::Pending);
            }
            // SAFETY: the admitted container retains its initialized installation.
            if unsafe { visitor.try_visit_stored(&self.installation, budget)? }
                == ScanStatus::Pending
            {
                return Ok(ScanStatus::Pending);
            }
            *next = 1;
        }
        while let Some(edge) = self.bindings.get(*next - 1) {
            if !budget.take(1) {
                return Ok(ScanStatus::Pending);
            }
            // SAFETY: this immutable admitted container owns the initialized edge.
            if unsafe { visitor.try_visit_stored(edge, budget)? } == ScanStatus::Pending {
                return Ok(ScanStatus::Pending);
            }
            *next += 1;
        }
        Ok(ScanStatus::Complete)
    }

    fn charge(&self) -> ObjectCharge {
        ObjectCharge {
            bytes: self.bindings.capacity() * size_of::<Edge<StaticCellObject>>(),
            units: self.bindings.len(),
        }
    }
}
