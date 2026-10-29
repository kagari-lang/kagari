use crate::{
    error::{RuntimeError, RuntimeErrorKind},
    gc::GcHeap,
    host::{
        ActiveBorrowFrame, BorrowEpoch, FrameBorrowRecord, FrameHostBorrowToken, HostBorrowKind,
        HostBorrowOwner, HostBorrowTable, HostCallGuard, HostFrameId, HostObjectId,
        PathBorrowState,
    },
    metadata::TypeId,
    resource::ResourceState,
    value::Value,
};
use kagari_common::identity::table::DefinitionId;
use std::sync::Arc;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(super) enum BorrowObjectKey {
    Installed(HostObjectId),
    External(HostObjectId),
}

impl FrameHostBorrowToken {
    pub(super) fn shared(self) -> Self {
        Self {
            borrow_kind: HostBorrowKind::Shared,
            ..self
        }
    }

    fn new(
        owner: HostBorrowOwner,
        frame_id: HostFrameId,
        object_id: HostObjectId,
        borrow_kind: HostBorrowKind,
        type_id: TypeId,
        epoch: BorrowEpoch,
    ) -> Self {
        Self {
            owner,
            frame_id,
            object_id,
            borrow_kind,
            type_id,
            epoch,
        }
    }

    pub fn frame_id(self) -> HostFrameId {
        self.frame_id
    }

    pub fn object_id(self) -> HostObjectId {
        self.object_id
    }

    pub fn borrow_kind(self) -> HostBorrowKind {
        self.borrow_kind
    }

    pub fn type_id(self) -> TypeId {
        self.type_id
    }

    pub fn epoch(self) -> BorrowEpoch {
        self.epoch
    }
}

impl FrameBorrowRecord {
    fn object_key(&self) -> BorrowObjectKey {
        if self.type_id.is_some() {
            BorrowObjectKey::Installed(self.object_id)
        } else {
            BorrowObjectKey::External(self.object_id)
        }
    }

    fn matches(&self, token: FrameHostBorrowToken) -> bool {
        self.object_id == token.object_id
            && self.borrow_kind.satisfies(token.borrow_kind)
            && self.type_id == Some(token.type_id)
            && self.path.is_empty()
    }
}

impl HostBorrowTable {
    pub fn enter_frame(&self) -> Result<HostCallGuard<'_>, RuntimeError> {
        self.enter_frame_in(None)
    }

    pub(crate) fn enter_frame_in<'runtime>(
        &'runtime self,
        resources: Option<&'runtime ResourceState>,
    ) -> Result<HostCallGuard<'runtime>, RuntimeError> {
        if let Some(resources) = resources {
            resources.ensure_execution_allowed()?;
        }
        let mut state = self.state.borrow_mut();
        let frame_id = HostFrameId(state.next_frame_id);
        let epoch = BorrowEpoch(state.next_epoch);
        let next_frame = state
            .next_frame_id
            .checked_add(1)
            .ok_or_else(|| invariant(resources, "host frame identity exhausted"))?;
        let next_epoch = state
            .next_epoch
            .checked_add(1)
            .ok_or_else(|| invariant(resources, "host borrow epoch exhausted"))?;
        state
            .active_frames
            .try_reserve(1)
            .map_err(|_| capacity_error(resources))?;
        state.next_frame_id = next_frame;
        state.next_epoch = next_epoch;
        state.active_frames.insert(
            frame_id,
            ActiveBorrowFrame {
                epoch,
                borrows: Vec::new(),
                roots: Vec::new(),
            },
        );
        Ok(HostCallGuard {
            table: self,
            resources,
            frame_id,
            epoch,
        })
    }

    pub fn validate(
        &self,
        token: FrameHostBorrowToken,
        required_kind: HostBorrowKind,
    ) -> Result<(), RuntimeError> {
        if token.owner != self.owner {
            return Err(RuntimeError::expired_host_borrow(
                "host borrow belongs to another runtime or table",
            ));
        }
        let state = self.state.borrow();
        let frame = state.active_frames.get(&token.frame_id).ok_or_else(|| {
            RuntimeError::expired_host_borrow(format!(
                "frame {} is no longer active",
                token.frame_id.index()
            ))
        })?;
        if frame.epoch != token.epoch {
            return Err(RuntimeError::expired_host_borrow(format!(
                "frame {} epoch mismatch",
                token.frame_id.index()
            )));
        }
        if !token.borrow_kind.satisfies(required_kind) {
            return Err(RuntimeError::host_borrow_conflict(format!(
                "{:?} borrow cannot satisfy {:?} access",
                token.borrow_kind, required_kind
            )));
        }
        if !frame.borrows.iter().any(|record| record.matches(token)) {
            return Err(RuntimeError::expired_host_borrow(format!(
                "token for host object {} is not active",
                token.object_id.0
            )));
        }
        Ok(())
    }

    pub(crate) fn validate_no_escape(heap: &GcHeap, value: &Value) -> Result<(), RuntimeError> {
        if value.contains_host_borrow(heap) {
            Err(RuntimeError::host_borrow_escape(
                "frame-scoped host borrow cannot outlive its call frame",
            ))
        } else {
            Ok(())
        }
    }

    pub(super) fn borrow(
        &self,
        frame: &HostCallGuard<'_>,
        object_id: HostObjectId,
        borrow_kind: HostBorrowKind,
        type_id: TypeId,
    ) -> Result<FrameHostBorrowToken, RuntimeError> {
        self.borrow_record(
            frame,
            FrameBorrowRecord {
                object_id,
                borrow_kind,
                type_id: Some(type_id),
                path: Arc::default(),
            },
        )?;
        Ok(FrameHostBorrowToken::new(
            self.owner,
            frame.frame_id,
            object_id,
            borrow_kind,
            type_id,
            frame.epoch,
        ))
    }

    pub(super) fn borrow_record(
        &self,
        frame: &HostCallGuard<'_>,
        record: FrameBorrowRecord,
    ) -> Result<(), RuntimeError> {
        let FrameBorrowRecord {
            object_id,
            borrow_kind,
            ..
        } = record;
        let HostCallGuard {
            frame_id,
            epoch,
            resources,
            ..
        } = *frame;
        if let Some(resources) = resources {
            resources.ensure_execution_allowed()?;
        }
        let mut state = self.state.borrow_mut();
        let frame = state.active_frames.get(&frame_id).ok_or_else(|| {
            RuntimeError::expired_host_borrow(format!(
                "frame {} is no longer active",
                frame_id.index()
            ))
        })?;
        if frame.epoch != epoch {
            return Err(RuntimeError::expired_host_borrow(format!(
                "frame {} epoch mismatch",
                frame_id.index()
            )));
        }

        state
            .active_frames
            .get_mut(&frame_id)
            .expect("validated host frame")
            .borrows
            .try_reserve(1)
            .map_err(|_| capacity_error(resources))?;
        state
            .object_borrows
            .try_reserve(1)
            .map_err(|_| capacity_error(resources))?;
        let object_state = state.object_borrows.entry(record.object_key()).or_default();
        if object_state.paths.iter().any(|active| {
            paths_overlap(&active.path, &record.path)
                && (active.unique_count > 0
                    || (borrow_kind == HostBorrowKind::Unique && active.shared_count > 0))
        }) {
            return Err(RuntimeError::host_borrow_conflict(format!(
                "host object {} has an overlapping incompatible borrow",
                object_id.0
            )));
        }
        let slot = object_state
            .paths
            .iter()
            .position(|path| path.path == record.path);
        let slot = if let Some(slot) = slot {
            slot
        } else {
            object_state
                .paths
                .try_reserve(1)
                .map_err(|_| capacity_error(resources))?;
            object_state.paths.push(PathBorrowState {
                path: record.path.clone(),
                shared_count: 0,
                unique_count: 0,
            });
            object_state.paths.len() - 1
        };
        let path = &mut object_state.paths[slot];
        match borrow_kind {
            HostBorrowKind::Shared => {
                path.shared_count = path
                    .shared_count
                    .checked_add(1)
                    .ok_or_else(|| invariant(resources, "host shared borrow count exhausted"))?
            }
            HostBorrowKind::Unique => path.unique_count += 1,
        }

        state
            .active_frames
            .get_mut(&frame_id)
            .expect("checked active host borrow frame before recording token")
            .borrows
            .push(record);
        Ok(())
    }

    pub(super) fn leave_frame(
        &self,
        frame_id: HostFrameId,
        epoch: BorrowEpoch,
        resources: Option<&ResourceState>,
    ) {
        let mut state = self.state.borrow_mut();
        let Some(frame) = state.active_frames.remove(&frame_id) else {
            invariant(resources, "host borrow frame disappeared during cleanup");
            return;
        };
        if frame.epoch != epoch {
            invariant(resources, "host borrow frame epoch changed during cleanup");
        }

        for record in frame.borrows {
            let mut remove_object = false;
            if let Some(object_state) = state.object_borrows.get_mut(&record.object_key()) {
                if let Some(slot) = object_state
                    .paths
                    .iter()
                    .position(|path| path.path == record.path)
                {
                    let path = &mut object_state.paths[slot];
                    let count = match record.borrow_kind {
                        HostBorrowKind::Shared => &mut path.shared_count,
                        HostBorrowKind::Unique => &mut path.unique_count,
                    };
                    *count = count.checked_sub(1).unwrap_or_else(|| {
                        invariant(resources, "host path borrow count underflow");
                        0
                    });
                    if path.shared_count == 0 && path.unique_count == 0 {
                        object_state.paths.swap_remove(slot);
                    }
                } else {
                    invariant(resources, "host path borrow disappeared during cleanup");
                }

                remove_object = object_state.is_empty();
            } else {
                invariant(resources, "host object borrow disappeared during cleanup");
            }
            if remove_object {
                state.object_borrows.remove(&record.object_key());
            }
        }
    }
}

fn invariant(resources: Option<&ResourceState>, message: &'static str) -> RuntimeError {
    resources.map_or_else(
        || RuntimeError::new(RuntimeErrorKind::EngineFault, message),
        |resources| resources.quarantine(message),
    )
}

fn capacity_error(resources: Option<&ResourceState>) -> RuntimeError {
    resources.map_or_else(
        || RuntimeError::resource_limit("host borrow capacity"),
        |resources| resources.limit("host borrow capacity"),
    )
}

/// Paths are generated inline field identities. Equal/prefix paths overlap;
/// divergent structural fields are disjoint. Empty is the whole reserved root.
fn paths_overlap(left: &[DefinitionId], right: &[DefinitionId]) -> bool {
    left.starts_with(right) || right.starts_with(left)
}
