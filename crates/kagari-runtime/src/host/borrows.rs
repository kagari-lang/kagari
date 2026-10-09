use crate::{
    error::{RuntimeError, RuntimeErrorKind},
    gc::GcHeap,
    host::{
        ActiveBorrowFrame, BorrowEpoch, FrameBorrowRecord, FrameHostBorrowToken, HostBorrowKind,
        HostBorrowTable, HostCallGuard, HostFrameId, HostObjectId,
    },
    metadata::TypeId,
    resource::ResourceState,
    value::Value,
};

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
        if !frame
            .borrows
            .iter()
            .copied()
            .any(|record| record.matches(token))
        {
            return Err(RuntimeError::expired_host_borrow(format!(
                "token for host object {} is not active",
                token.object_id.0
            )));
        }
        Ok(())
    }

    pub fn validate_no_escape(heap: &GcHeap, value: &Value) -> Result<(), RuntimeError> {
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
        let object_state = state.object_borrows.entry(object_id).or_default();
        match borrow_kind {
            HostBorrowKind::Shared if object_state.unique_count > 0 => {
                return Err(RuntimeError::host_borrow_conflict(format!(
                    "host object {} already has an active unique borrow",
                    object_id.0
                )));
            }
            HostBorrowKind::Shared => {
                object_state.shared_count = object_state
                    .shared_count
                    .checked_add(1)
                    .ok_or_else(|| invariant(resources, "host shared borrow count exhausted"))?;
            }
            HostBorrowKind::Unique
                if object_state.shared_count > 0 || object_state.unique_count > 0 =>
            {
                return Err(RuntimeError::host_borrow_conflict(format!(
                    "host object {} already has an active borrow",
                    object_id.0
                )));
            }
            HostBorrowKind::Unique => {
                object_state.unique_count += 1;
            }
        }

        let token =
            FrameHostBorrowToken::new(self.owner, frame_id, object_id, borrow_kind, type_id, epoch);
        state
            .active_frames
            .get_mut(&frame_id)
            .expect("checked active host borrow frame before recording token")
            .borrows
            .push(FrameBorrowRecord::from_token(token));
        Ok(token)
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
            if let Some(object_state) = state.object_borrows.get_mut(&record.object_id) {
                match record.borrow_kind {
                    HostBorrowKind::Shared => {
                        object_state.shared_count =
                            object_state.shared_count.checked_sub(1).unwrap_or_else(|| {
                                invariant(resources, "host shared borrow count underflow");
                                0
                            });
                    }
                    HostBorrowKind::Unique => {
                        object_state.unique_count =
                            object_state.unique_count.checked_sub(1).unwrap_or_else(|| {
                                invariant(resources, "host unique borrow count underflow");
                                0
                            });
                    }
                }
                remove_object = object_state.is_empty();
            } else {
                invariant(resources, "host object borrow disappeared during cleanup");
            }
            if remove_object {
                state.object_borrows.remove(&record.object_id);
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
