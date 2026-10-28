//! Initial enumeration across the existing root owners; no final rescan or root snapshot.
use crate::{
    error::RuntimeError,
    frame::values::scanning::FrameCursor,
    gc::{GcHeap, graph::objects::ScanEdge, roots::scanning::RootCursor},
    host::{HostRegistry, scanning::HostRootCursor},
    module::{ModuleStore, scanning::ProgramRootCursor},
    native::storage::scan::{ScanBudget, ScanStatus},
};

#[derive(Debug)]
pub(crate) struct InitialRoots {
    roots: RootCursor,
    frames: FrameCursor,
    host: HostRootCursor,
    programs: ProgramRootCursor,
    section: u8,
}

impl GcHeap {
    pub(crate) fn begin_initial_roots(
        &self,
        modules: &ModuleStore,
        host: &HostRegistry,
    ) -> Result<InitialRoots, RuntimeError> {
        self.ensure_execution_allowed()?;
        let roots = InitialRoots {
            roots: self.begin_root_scan()?,
            frames: self
                .resources()
                .frame_values
                .try_borrow()
                .map_err(|_| invalid())?
                .begin_root_scan(),
            host: host.begin_root_scan()?,
            programs: modules.begin_root_scan()?,
            section: 0,
        };
        self.graph.try_lock()?.begin_cycle()?;
        Ok(roots)
    }

    pub(crate) fn scan_initial_root_slice(
        &self,
        modules: &ModuleStore,
        host: &HostRegistry,
        roots: &mut InitialRoots,
        budget: &mut ScanBudget,
    ) -> Result<ScanStatus, RuntimeError> {
        let result = self
            .resources()
            .collect_operation(|| {
                let mut visit = |edge: ScanEdge<'_>, budget: &mut ScanBudget| {
                    self.shade_scanned_edge(modules, edge, budget)
                };
                while roots.section < 4 {
                    let status = match roots.section {
                        0 => self.scan_root_slice(&mut roots.roots, budget, &mut visit)?,
                        1 => self
                            .resources()
                            .frame_values
                            .try_borrow()
                            .map_err(|_| invalid())?
                            .scan_root_slice(&mut roots.frames, budget, &mut visit)?,
                        2 => host.scan_root_slice(&mut roots.host, budget, &mut visit)?,
                        3 => modules.scan_root_slice(&mut roots.programs, budget, &mut visit)?,
                        _ => unreachable!(),
                    };
                    if status == ScanStatus::Pending {
                        return Ok(status);
                    }
                    if !budget.take(1) {
                        return Ok(ScanStatus::Pending);
                    }
                    roots.section += 1;
                }
                if roots.section == 4 {
                    if !budget.take(1) {
                        return Ok(ScanStatus::Pending);
                    }
                    self.graph.try_lock()?.finish_roots()?;
                    roots.section = 5;
                }
                Ok(ScanStatus::Complete)
            })
            .and_then(|result| result);
        if result.is_err() {
            if let Ok(mut graph) = self.graph.try_lock() {
                graph.fault();
            }
            self.resources().quarantine("initial root scan failed");
        }
        result
    }
}

fn invalid() -> RuntimeError {
    RuntimeError::module_validation("borrowed initial root owner")
}
