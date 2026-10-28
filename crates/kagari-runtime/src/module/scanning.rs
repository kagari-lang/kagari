//! Short-borrow module/static scanners over stable owner and descriptor slots.
use crate::{
    error::RuntimeError,
    execution_metadata::MetadataEdge,
    gc::graph::{
        GraphIdentity,
        marks::MarkRef,
        objects::{ScanEdge, ScanVisitor},
    },
    host::HostRegistryId,
    module::{
        ExecutableModule, ModuleKey, ModuleStore, ModuleStoreInner,
        descriptor_index::DescriptorSlot,
        records::storage::ResolvedRecord,
        state::{StaticState, cells::CellId},
    },
    native::storage::scan::{ScanBudget, ScanStatus},
};
use kagari_bytecode::program::ModuleRef;
use std::sync::Arc;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) struct ProgramScanId {
    pub(in crate::module) owner: HostRegistryId,
    pub(in crate::module) slot: usize,
    pub(in crate::module) generation: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) struct StaticScanId {
    pub(in crate::module) owner: HostRegistryId,
    pub(in crate::module) cell: CellId,
}

#[derive(Debug)]
pub(crate) struct ProgramRootCursor {
    next: usize,
    end: usize,
    retained: Option<RetainedProgramCursor>,
}

/// Only otherwise unrooted programs need a member-retention walk. The full root
/// identity pins lookup, not liveness; no lease or store borrow survives a slice.
#[derive(Debug)]
struct RetainedProgramCursor {
    id: ProgramScanId,
    next: usize,
    end: usize,
}

#[derive(Debug)]
pub(crate) struct ModuleCursor {
    id: ProgramScanId,
    section: u8,
    index: usize,
    field: usize,
    members: usize,
    constants: usize,
    cells: usize,
    functions: usize,
    descriptors: [usize; 5],
    kind: usize,
    pinned: Option<DescriptorSlot>,
    dependency: usize,
    available: bool,
}

impl ModuleStoreInner {
    /// Consume the same borrowed owner used for availability/executable admission.
    /// A member still checks its root record; only graph argument construction is lazy.
    pub(super) fn publish_resolved_program(
        &self,
        resolved: ResolvedRecord<'_>,
    ) -> Result<(), RuntimeError> {
        let module = &resolved.record().module;
        if module.slot == module.program.root {
            return self.records.publish(resolved, &self.graph);
        }
        let root = module.program_root();
        let root = self
            .records
            .resolve(&root.key())
            .filter(|resolved| resolved.record().matches(&root))
            .ok_or_else(invalid)?;
        self.records.publish(root, &self.graph)
    }
}

impl ModuleStore {
    pub(crate) fn next_group_owner(
        &self,
        block: usize,
        epoch: u64,
    ) -> Result<(Option<GraphIdentity>, bool), RuntimeError> {
        Ok(self
            .inner
            .try_borrow()
            .map_err(|_| invalid())?
            .records
            .next_gray(block, epoch))
    }

    /// Exact availability and phase admission precede exposure of a new program root.
    pub(crate) fn publish_program(&self, module: &ExecutableModule) -> Result<(), RuntimeError> {
        let store = self.inner.try_borrow().map_err(|_| invalid())?;
        let resolved = store.resolve_owner(module).ok_or_else(invalid)?;
        store.publish_resolved_program(resolved)
    }

    pub(crate) fn begin_root_scan(&self) -> Result<ProgramRootCursor, RuntimeError> {
        Ok(ProgramRootCursor {
            next: 0,
            end: self
                .inner
                .try_borrow()
                .map_err(|_| invalid())?
                .records
                .scan_extent(),
            retained: None,
        })
    }

    pub(crate) fn scan_root_slice(
        &self,
        cursor: &mut ProgramRootCursor,
        budget: &mut ScanBudget,
        visit: &mut ScanVisitor<'_>,
    ) -> Result<ScanStatus, RuntimeError> {
        let store = self.inner.try_borrow().map_err(|_| invalid())?;
        while cursor.retained.is_some() || cursor.next < cursor.end {
            if let Some(retained) = &mut cursor.retained {
                if scan_retained_program(&store, retained, budget, visit)? == ScanStatus::Pending {
                    return Ok(ScanStatus::Pending);
                }
                cursor.retained = None;
                continue;
            }
            if !budget.take(1) {
                return Ok(ScanStatus::Pending);
            }
            let resolved = store.records.resolve_slot(cursor.next);
            cursor.next += 1;
            let Some(resolved) = resolved else {
                continue;
            };
            let module = &resolved.record().module;
            // Program eligibility belongs to its root. Member records are still
            // charged slots, but never repeat staging/current checks or redirects.
            if module.slot != module.program.root
                || !store.records.accessible(resolved, &self.graph)?
            {
                continue;
            }
            let staged = match store.staged.get(&module.key()) {
                Some(lease) => match lease.upgrade() {
                    Some(lease) => Some(lease),
                    None => continue,
                },
                None => None,
            };
            let current =
                if let Some(installation) = self.installations.get(&module.installation_id()) {
                    installation
                        .current
                        .try_lock()
                        .map_err(|_| invalid())?
                        .as_ref()
                        .is_some_and(|current| Arc::ptr_eq(current, &module.program))
                } else {
                    false
                };
            if current || staged.is_some() {
                // Keep the upgraded candidate lease through publication, never
                // across a yield. Its concurrent last release can only overretain.
                if visit(
                    ScanEdge::Identity(GraphIdentity::Program(resolved.identity())),
                    budget,
                )? == ScanStatus::Pending
                {
                    cursor.next -= 1;
                    return Ok(ScanStatus::Pending);
                }
            } else {
                cursor.retained = Some(RetainedProgramCursor {
                    id: resolved.identity(),
                    next: 0,
                    end: module.program.modules.len(),
                });
            }
        }
        Ok(ScanStatus::Complete)
    }

    pub(crate) fn with_graph_mark<R>(
        &self,
        id: GraphIdentity,
        action: impl FnOnce(MarkRef<'_>) -> Result<R, RuntimeError>,
    ) -> Result<R, RuntimeError> {
        let store = self.inner.try_borrow().map_err(|_| invalid())?;
        match id {
            GraphIdentity::Program(id) => action(store.records.mark(id).ok_or_else(invalid)?),
            GraphIdentity::Static(id) => {
                let cell = store.cells.get(&id.cell).ok_or_else(invalid)?;
                if cell.owner.registry_owner != id.owner {
                    return Err(invalid());
                }
                action(cell.mark.as_ref())
            }
            _ => Err(invalid()),
        }
    }

    /// Collector lookup checks exact executable identity, not temporary candidate
    /// availability. Public admission checks availability before shading; an owner
    /// already queued must remain resolvable after its last candidate lease drops.
    pub(crate) fn with_program_edge_mark<R>(
        &self,
        owner: &ExecutableModule,
        action: impl FnOnce(GraphIdentity, MarkRef<'_>) -> Result<R, RuntimeError>,
    ) -> Result<R, RuntimeError> {
        let store = self.inner.try_borrow().map_err(|_| invalid())?;
        let root = owner.program_root();
        let resolved = store
            .records
            .resolve(&root.key())
            .filter(|resolved| resolved.record().matches(&root))
            .ok_or_else(invalid)?;
        let id = resolved.identity();
        action(
            GraphIdentity::Program(id),
            store.records.mark(id).ok_or_else(invalid)?,
        )
    }

    #[cfg(test)]
    pub(crate) fn program_scan_identity(
        &self,
        owner: &ExecutableModule,
    ) -> Result<GraphIdentity, RuntimeError> {
        let store = self.inner.try_borrow().map_err(|_| invalid())?;
        store
            .records
            .scan_id(&owner.program_root())
            .map(GraphIdentity::Program)
            .ok_or_else(invalid)
    }

    pub(crate) fn begin_module_scan(
        &self,
        id: ProgramScanId,
    ) -> Result<ModuleCursor, RuntimeError> {
        let store = self.inner.try_borrow().map_err(|_| invalid())?;
        let record = store.records.scan_record(id).ok_or_else(invalid)?;
        let members = if record.module.slot == record.module.program.root {
            record.module.program.modules.len()
        } else {
            0
        };
        if record.bindings.len() != record.module.bytecode.module_slots.len() {
            return Err(invalid());
        }
        Ok(ModuleCursor {
            id,
            section: 0,
            index: 0,
            field: 0,
            members,
            constants: record.constants.scan_len(),
            cells: record.bindings.len(),
            functions: record
                .execution
                .as_ref()
                .map_or(0, |execution| execution.scan_len()),
            descriptors: record.descriptors.scan_extents(),
            kind: 0,
            pinned: None,
            dependency: 0,
            available: true,
        })
    }

    /// Runtime-owned fields only: no provider invocation or descriptor release.
    /// Complete ends this prefix, not the module's descriptor continuation.
    pub(crate) fn scan_module_fields(
        &self,
        cursor: &mut ModuleCursor,
        budget: &mut ScanBudget,
        visit: &mut ScanVisitor<'_>,
    ) -> Result<ScanStatus, RuntimeError> {
        while cursor.section < 4 {
            if !budget.take(1) {
                return Ok(ScanStatus::Pending);
            }
            let store = self.inner.try_borrow().map_err(|_| invalid())?;
            let record = store.records.scan_record(cursor.id).ok_or_else(invalid)?;
            let extent = match cursor.section {
                0 => cursor.members,
                1 => cursor.constants,
                2 => cursor.cells,
                3 => cursor.functions,
                _ => unreachable!(),
            };
            if cursor.index == extent {
                cursor.section += 1;
                cursor.index = 0;
                cursor.field = 0;
                continue;
            }
            match cursor.section {
                0 => {
                    let member = record
                        .module
                        .member(ModuleRef::new(cursor.index))
                        .ok_or_else(invalid)?;
                    let id = store.records.scan_id(&member).ok_or_else(invalid)?;
                    if visit(ScanEdge::Identity(GraphIdentity::Program(id)), budget)?
                        == ScanStatus::Pending
                    {
                        return Ok(ScanStatus::Pending);
                    }
                    cursor.index += 1;
                }
                1 => {
                    if let Some(value) = record.constants.scan_field(cursor.index)
                        && visit(ScanEdge::Value(value), budget)? == ScanStatus::Pending
                    {
                        return Ok(ScanStatus::Pending);
                    }
                    cursor.index += 1;
                }
                2 => {
                    let cell = store.bound_cell(record, cursor.index).ok_or_else(invalid)?;
                    let id = StaticScanId {
                        owner: cell.owner.registry_owner,
                        cell: record.bindings[cursor.index],
                    };
                    if visit(ScanEdge::Identity(GraphIdentity::Static(id)), budget)?
                        == ScanStatus::Pending
                    {
                        return Ok(ScanStatus::Pending);
                    }
                    cursor.index += 1;
                }
                3 => {
                    let execution = record.execution.as_ref().ok_or_else(invalid)?;
                    if cursor.field == execution.scan_call_count(cursor.index) {
                        cursor.index += 1;
                        cursor.field = 0;
                    } else {
                        if let Some(edge) = execution.scan_call(cursor.index, cursor.field)
                            && visit(ScanEdge::Metadata(edge), budget)? == ScanStatus::Pending
                        {
                            return Ok(ScanStatus::Pending);
                        }
                        cursor.field += 1;
                    }
                }
                _ => unreachable!(),
            }
        }
        Ok(ScanStatus::Complete)
    }

    /// The driver must release graph control before this suffix: finishing a
    /// descriptor can destroy an evicted entry, including its owned dependencies.
    pub(crate) fn scan_module_descriptors(
        &self,
        cursor: &mut ModuleCursor,
        budget: &mut ScanBudget,
        visit: &mut ScanVisitor<'_>,
    ) -> Result<ScanStatus, RuntimeError> {
        if cursor.section < 4 {
            return Err(invalid());
        }
        while cursor.section < 5 {
            if !budget.take(1) {
                return Ok(ScanStatus::Pending);
            }
            if self.scan_descriptor_field(cursor, budget, visit)? == ScanStatus::Pending {
                return Ok(ScanStatus::Pending);
            }
        }
        Ok(ScanStatus::Complete)
    }

    fn scan_descriptor_field(
        &self,
        cursor: &mut ModuleCursor,
        budget: &mut ScanBudget,
        visit: &mut ScanVisitor<'_>,
    ) -> Result<ScanStatus, RuntimeError> {
        if cursor.kind == cursor.descriptors.len() {
            cursor.section = 5;
            return Ok(ScanStatus::Complete);
        }
        if cursor.index == cursor.descriptors[cursor.kind] {
            cursor.kind += 1;
            cursor.index = 0;
            return Ok(ScanStatus::Complete);
        }
        let Some(slot) = cursor.pinned else {
            let mut store = self.inner.try_borrow_mut().map_err(|_| invalid())?;
            let record = store
                .records
                .scan_record_mut(cursor.id)
                .ok_or_else(invalid)?;
            cursor.pinned = record.descriptors.scan_start(cursor.kind, cursor.index);
            if cursor.pinned.is_none() {
                cursor.index += 1;
            }
            cursor.dependency = 0;
            cursor.field = 0;
            cursor.available = true;
            return Ok(ScanStatus::Complete);
        };
        {
            let store = self.inner.try_borrow().map_err(|_| invalid())?;
            let record = store.records.scan_record(cursor.id).ok_or_else(invalid)?;
            let view = record
                .descriptors
                .scan_view(cursor.kind, slot)
                .ok_or_else(invalid)?;
            if let Some(owner) = view.dependencies.get(cursor.dependency) {
                cursor.available &= store.resolve(owner).is_some();
                cursor.dependency += 1;
                return Ok(ScanStatus::Complete);
            }
            if cursor.available && cursor.field < view.field_count() {
                if let Some(edge) = view.field(cursor.field)
                    && visit(ScanEdge::Metadata(edge), budget)? == ScanStatus::Pending
                {
                    return Ok(ScanStatus::Pending);
                }
                cursor.field += 1;
                return Ok(ScanStatus::Complete);
            }
        }
        let mut store = self.inner.try_borrow_mut().map_err(|_| invalid())?;
        store
            .records
            .scan_record_mut(cursor.id)
            .ok_or_else(invalid)?
            .descriptors
            .scan_finish(cursor.kind, slot);
        cursor.pinned = None;
        cursor.index += 1;
        Ok(ScanStatus::Complete)
    }

    #[cfg(test)]
    pub(crate) fn bound_static_scan_identity(
        &self,
        owner: &ExecutableModule,
        slot: usize,
    ) -> Result<GraphIdentity, RuntimeError> {
        let store = self.inner.try_borrow().map_err(|_| invalid())?;
        let record = store.resolve(owner).ok_or_else(invalid)?;
        let cell = store.bound_cell(record, slot).ok_or_else(invalid)?;
        Ok(GraphIdentity::Static(StaticScanId {
            owner: cell.owner.registry_owner,
            cell: record.bindings[slot],
        }))
    }

    pub(crate) fn scan_static_slice(
        &self,
        id: StaticScanId,
        budget: &mut ScanBudget,
        visit: &mut ScanVisitor<'_>,
    ) -> Result<ScanStatus, RuntimeError> {
        if !budget.take(1) {
            return Ok(ScanStatus::Pending);
        }
        let store = self.inner.try_borrow().map_err(|_| invalid())?;
        let cell = store.cells.get(&id.cell).ok_or_else(invalid)?;
        if cell.owner.registry_owner != id.owner {
            return Err(invalid());
        }
        if !cell.failed_attempt() {
            match &cell.state {
                StaticState::Ready(value) => {
                    if visit(ScanEdge::Value(*value), budget)? == ScanStatus::Pending {
                        return Ok(ScanStatus::Pending);
                    }
                }
                StaticState::Uninitialized(recipe) | StaticState::Initializing(recipe) => {
                    if visit(
                        ScanEdge::Metadata(MetadataEdge::Program(&recipe.module)),
                        budget,
                    )? == ScanStatus::Pending
                    {
                        return Ok(ScanStatus::Pending);
                    }
                }
                StaticState::Failed(_) => {}
            }
        }
        Ok(ScanStatus::Complete)
    }
}

/// Retention registrations can only be added under the store's exclusive borrow
/// and insertion barrier. Concurrent lease drops may make this walk conservative;
/// neither a release nor publication behind the cursor requires a restart.
fn scan_retained_program(
    store: &ModuleStoreInner,
    cursor: &mut RetainedProgramCursor,
    budget: &mut ScanBudget,
    visit: &mut ScanVisitor<'_>,
) -> Result<ScanStatus, RuntimeError> {
    let record = store.records.scan_record(cursor.id).ok_or_else(invalid)?;
    while cursor.next < cursor.end {
        if !budget.take(1) {
            return Ok(ScanStatus::Pending);
        }
        let member = record
            .module
            .program
            .modules
            .get(cursor.next)
            .ok_or_else(invalid)?;
        cursor.next += 1;
        let key = ModuleKey {
            id: member.id,
            epoch: member.epoch,
        };
        if store
            .retentions
            .get(&key)
            .is_some_and(|retentions| retentions.is_retained())
        {
            if visit(
                ScanEdge::Identity(GraphIdentity::Program(cursor.id)),
                budget,
            )? == ScanStatus::Pending
            {
                cursor.next -= 1;
                return Ok(ScanStatus::Pending);
            }
            return Ok(ScanStatus::Complete);
        }
    }
    Ok(ScanStatus::Complete)
}

fn invalid() -> RuntimeError {
    RuntimeError::module_validation("invalid or borrowed module scan owner")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        Runtime,
        module::{ModuleEpochRetention, state::tests::program},
        test_support::native::program::ProgramFixture,
    };
    use kagari_bytecode::{module::BytecodeModule, program::BytecodeProgram};
    use kagari_common::identity::{map::DefinitionContext, metadata::DefinitionMetadata};
    use kagari_types::{scalar::BuiltinType, ty::Ty};
    use std::thread;

    fn members(count: usize) -> DefinitionMetadata<BytecodeProgram> {
        let mut modules: Vec<_> = (0..count)
            .map(|index| {
                let mut module = BytecodeModule::default();
                module.identity.path.push(format!("member{index}"));
                module
            })
            .collect();
        modules[2].dependencies = (0..count)
            .filter(|index| *index != 2)
            .map(ModuleRef::new)
            .collect();
        ProgramFixture {
            exports: Default::default(),
            root: ModuleRef::new(2),
            modules,
        }
        .metadata(
            DefinitionContext::new().unwrap().snapshot(),
            &Default::default(),
        )
        .unwrap()
    }

    fn scan_program_roots(
        runtime: &Runtime,
        cursor: &mut ProgramRootCursor,
    ) -> (Vec<GraphIdentity>, usize) {
        let mut selected = Vec::new();
        let mut work = 0;
        loop {
            let mut budget = ScanBudget::new(5);
            let status = runtime
                .modules
                .scan_root_slice(cursor, &mut budget, &mut |edge, budget| {
                    let ScanEdge::Identity(id) = edge else {
                        panic!("program root identity")
                    };
                    let status = runtime.gc.shade_scanned_edge(
                        &runtime.modules,
                        ScanEdge::Identity(id),
                        budget,
                    )?;
                    if status == ScanStatus::Complete {
                        selected.push(id);
                    }
                    Ok(status)
                })
                .unwrap();
            assert!(budget.spent() <= 5);
            work += budget.spent();
            if status == ScanStatus::Complete {
                return (selected, work);
            }
            assert!(budget.spent() > 0);
        }
    }

    fn close_root_scan(runtime: &Runtime) {
        runtime.gc.graph.lock().finish_roots().unwrap();
        runtime.gc.drain_graph_work(&runtime.modules).unwrap();
        runtime.gc.graph.lock().close_mark().unwrap();
    }

    #[test]
    fn current_and_staged_programs_select_one_root_each_not_one_per_member() {
        let mut runtime = Runtime::default();
        let installed = runtime.load_program("root-owners", members(40)).unwrap();
        let baseline = runtime.select_program(&installed).unwrap();
        let staged = runtime
            .stage_reload_verified_program(&installed, None, baseline.verified_program().clone())
            .unwrap();
        let baseline_id = runtime.modules.program_scan_identity(&baseline).unwrap();
        let staged_id = runtime
            .modules
            .program_scan_identity(staged.module())
            .unwrap();
        let mut cursor = runtime.modules.begin_root_scan().unwrap();
        runtime.gc.graph.lock().begin_cycle().unwrap();
        let (selected, work) = scan_program_roots(&runtime, &mut cursor);
        assert_eq!(selected, [baseline_id, staged_id]);
        // Every stable slot is inspected, including nonroots; current/staged
        // programs do not additionally inspect each member's retention records.
        assert_eq!(
            work,
            80 + 4 + 1,
            "slots, one four-word block reset, and the root slot retried when reset did not fit"
        );
        thread::spawn(move || drop(staged)).join().unwrap();
        close_root_scan(&runtime);
        assert!(
            runtime
                .gc
                .graph_survivor(&runtime.modules, staged_id)
                .unwrap()
        );
        runtime.gc.graph.lock().finish_sweep().unwrap();
        assert_eq!(
            runtime.collect_garbage().unwrap().reclaimed_modules.len(),
            40
        );
    }

    #[test]
    fn otherwise_unrooted_program_checks_member_retention_with_bounded_progress() {
        let mut runtime = Runtime::default();
        let installed = runtime
            .load_program("member-retention", members(40))
            .unwrap();
        let baseline = runtime.select_program(&installed).unwrap();
        let member = baseline.member(ModuleRef::new(39)).unwrap();
        let lease = runtime
            .retain_module(&member, ModuleEpochRetention::RuntimeValue)
            .unwrap();
        let clone = lease.clone();
        let staged = runtime
            .stage_reload_verified_program(&installed, None, baseline.verified_program().clone())
            .unwrap();
        runtime.publish_staged_reload(staged).unwrap();
        let baseline_id = runtime.modules.program_scan_identity(&baseline).unwrap();
        let mut cursor = runtime.modules.begin_root_scan().unwrap();
        runtime.gc.graph.lock().begin_cycle().unwrap();
        // Reach the last member in separate slices. A cross-thread clone release
        // must not lose the still-live member registration or restart enumeration.
        while cursor
            .retained
            .as_ref()
            .is_none_or(|retained| retained.next != 39)
        {
            let mut budget = ScanBudget::new(1);
            assert_eq!(
                runtime
                    .modules
                    .scan_root_slice(&mut cursor, &mut budget, &mut |_, _| {
                        panic!("no member retained before the final member")
                    })
                    .unwrap(),
                ScanStatus::Pending
            );
            assert_eq!(budget.spent(), 1);
        }
        thread::spawn(move || drop(clone)).join().unwrap();
        let (selected, _) = scan_program_roots(&runtime, &mut cursor);
        assert_eq!(selected.iter().filter(|id| **id == baseline_id).count(), 1);
        thread::spawn(move || drop(lease)).join().unwrap();
        close_root_scan(&runtime);
        assert!(
            runtime
                .gc
                .graph_survivor(&runtime.modules, baseline_id)
                .unwrap()
        );
        runtime.gc.graph.lock().finish_sweep().unwrap();
        assert_eq!(
            runtime.collect_garbage().unwrap().reclaimed_modules.len(),
            40
        );
    }

    #[test]
    fn member_retention_and_staging_behind_completed_root_cursor_publish_work() {
        let mut runtime = Runtime::default();
        let installed = runtime.load_program("late-roots", members(4)).unwrap();
        let baseline = runtime.select_program(&installed).unwrap();
        let staged = runtime
            .stage_reload_verified_program(&installed, None, baseline.verified_program().clone())
            .unwrap();
        runtime.publish_staged_reload(staged).unwrap();
        let baseline_id = runtime.modules.program_scan_identity(&baseline).unwrap();
        let mut cursor = runtime.modules.begin_root_scan().unwrap();
        runtime.gc.graph.lock().begin_cycle().unwrap();
        let (selected, work) = scan_program_roots(&runtime, &mut cursor);
        assert!(!selected.contains(&baseline_id));
        assert_eq!(
            work,
            12 + 4,
            "slots and retention checks plus one block reset"
        );
        let member = baseline.member(ModuleRef::new(0)).unwrap();
        let lease = runtime
            .retain_module(&member, ModuleEpochRetention::ActiveCall)
            .unwrap();
        let staged = runtime
            .stage_reload_verified_program(&installed, None, baseline.verified_program().clone())
            .unwrap();
        let staged_id = runtime
            .modules
            .program_scan_identity(staged.module())
            .unwrap();
        let (selected, work) = scan_program_roots(&runtime, &mut cursor);
        assert!(selected.is_empty());
        assert_eq!(work, 0);
        close_root_scan(&runtime);
        for id in [baseline_id, staged_id] {
            assert!(runtime.gc.graph_survivor(&runtime.modules, id).unwrap());
        }
        runtime.gc.graph.lock().finish_sweep().unwrap();
        drop((lease, staged));
        assert_eq!(
            runtime.collect_garbage().unwrap().reclaimed_modules.len(),
            8
        );
    }

    #[test]
    fn reload_between_slices_keeps_queued_static_backing_and_exact_program_owners() {
        let mut runtime = Runtime::default();
        let installed = runtime
            .load_program("scan-reload", program(Ty::Builtin(BuiltinType::I32), true))
            .unwrap();
        let baseline = runtime.select_program(&installed).unwrap();
        let GraphIdentity::Program(baseline_id) =
            runtime.modules().program_scan_identity(&baseline).unwrap()
        else {
            panic!("program identity")
        };
        runtime.gc.graph.lock().begin_cycle().unwrap();
        runtime
            .gc
            .shade_graph_identity(runtime.modules(), GraphIdentity::Program(baseline_id))
            .unwrap();
        assert_eq!(
            runtime
                .gc
                .scan_graph_slice(runtime.modules(), &mut ScanBudget::new(1))
                .unwrap(),
            ScanStatus::Pending
        );
        // Staging admits newborn cells, then reload rebinds to the baseline cell.
        // The superseded newborn still has pending work and must stay resolvable.
        let candidate = runtime
            .stage_reload_verified_program(&installed, None, baseline.verified_program().clone())
            .unwrap();
        let candidate_id = runtime
            .modules()
            .program_scan_identity(candidate.module())
            .unwrap();
        assert_ne!(GraphIdentity::Program(baseline_id), candidate_id);
        let cells = {
            let store = runtime.modules().inner.borrow();
            assert_eq!(store.cells.len(), 2);
            store
                .cells
                .iter()
                .map(|(id, cell)| StaticScanId {
                    owner: cell.owner.registry_owner,
                    cell: *id,
                })
                .collect::<Vec<_>>()
        };
        runtime.publish_staged_reload(candidate).unwrap();
        runtime.gc.graph.lock().finish_roots().unwrap();
        for _ in 0..1000 {
            let mut budget = ScanBudget::new(1);
            if runtime
                .gc
                .scan_graph_slice(runtime.modules(), &mut budget)
                .unwrap()
                == ScanStatus::Complete
            {
                break;
            }
            assert_eq!(budget.spent(), 1);
        }
        runtime.gc.graph.lock().close_mark().unwrap();
        assert!(
            runtime
                .gc
                .graph_survivor(runtime.modules(), GraphIdentity::Program(baseline_id))
                .unwrap()
        );
        assert!(
            runtime
                .gc
                .graph_survivor(runtime.modules(), candidate_id)
                .unwrap()
        );
        for id in cells {
            assert!(
                runtime
                    .gc
                    .graph_survivor(runtime.modules(), GraphIdentity::Static(id))
                    .unwrap()
            );
        }
        runtime.gc.graph.lock().finish_sweep().unwrap();
        runtime.collect_garbage().unwrap();
        assert_eq!(runtime.modules().inner.borrow().cells.len(), 1);
    }

    #[test]
    fn module_scan_slots_reject_retired_generations_and_foreign_runtime_ids() {
        let mut runtime = Runtime::default();
        let installed = runtime
            .load_program("scan-slots", program(Ty::Builtin(BuiltinType::I32), true))
            .unwrap();
        let baseline = runtime.select_program(&installed).unwrap();
        let candidate = runtime
            .stage_reload_verified_program(&installed, None, baseline.verified_program().clone())
            .unwrap();
        let old_owner = candidate.module().clone();
        let GraphIdentity::Program(old) =
            runtime.modules().program_scan_identity(&old_owner).unwrap()
        else {
            panic!("program identity")
        };
        drop(candidate);
        runtime.collect_garbage().unwrap();
        assert!(runtime.modules().begin_module_scan(old).is_err());
        let candidate = runtime
            .stage_reload_verified_program(&installed, None, baseline.verified_program().clone())
            .unwrap();
        let GraphIdentity::Program(new) = runtime
            .modules()
            .program_scan_identity(candidate.module())
            .unwrap()
        else {
            panic!("program identity")
        };
        assert_eq!(old.slot, new.slot);
        assert_ne!(old.generation, new.generation);
        assert!(runtime.modules().program_scan_identity(&old_owner).is_err());
        assert!(runtime.modules().begin_module_scan(new).is_ok());
        let foreign = Runtime::default();
        assert!(foreign.modules().begin_module_scan(new).is_err());
        let _guard = runtime.gc.graph.lock();
        // Availability and publication share the checked owner resolution, and
        // Idle must not enter the queue mutex even for a still-staged program.
        assert!(
            runtime
                .modules()
                .contains_module(candidate.module())
                .unwrap()
        );
        runtime
            .modules()
            .publish_program(candidate.module())
            .unwrap();
        runtime
            .publish_executable_module(candidate.module())
            .unwrap();
        assert!(runtime.publish_executable_module(&old_owner).is_err());
        assert!(runtime.modules().publish_program(&old_owner).is_err());
        assert!(
            foreign
                .modules()
                .publish_program(candidate.module())
                .is_err()
        );
    }
}
