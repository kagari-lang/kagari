use crate::{
    ResourceState, RuntimeErrorKind,
    error::RuntimeError,
    host::{
        ActiveBorrowFrame, BorrowEpoch, FrameBorrowRecord, FrameHostBorrowToken, HostBorrowKind,
        HostBorrowTable, HostCallGuard, HostFrameId, HostObjectId,
    },
    metadata::TypeId,
    value::Value,
};
use std::rc::{Rc, Weak};

impl HostBorrowTable {
    pub(crate) fn with_resources(resources: &Rc<ResourceState>) -> Self {
        Self {
            resources: Some(Rc::downgrade(resources)),
            ..Default::default()
        }
    }

    pub(super) fn ensure_allowed(&self) -> Result<(), RuntimeError> {
        if let Some(resources) = &self.resources {
            resources
                .upgrade()
                .ok_or_else(|| RuntimeError::expired_host_borrow("host runtime has been released"))?
                .ensure_execution_allowed()?;
        }
        Ok(())
    }

    pub(super) fn invariant(&self, message: &'static str) -> RuntimeError {
        self.resources.as_ref().and_then(Weak::upgrade).map_or_else(
            || RuntimeError::new(RuntimeErrorKind::EngineFault, message),
            |resources| resources.quarantine(message),
        )
    }

    pub(super) fn capacity_error(&self) -> RuntimeError {
        self.resources.as_ref().and_then(Weak::upgrade).map_or_else(
            || RuntimeError::resource_limit("host borrow capacity"),
            |resources| resources.limit("host borrow capacity"),
        )
    }

    pub fn enter_frame(&self) -> Result<HostCallGuard, RuntimeError> {
        self.ensure_allowed()?;
        let mut state = self.state.borrow_mut();
        let frame_id = HostFrameId(state.next_frame_id);
        let epoch = BorrowEpoch(state.next_epoch);
        let next_frame = state
            .next_frame_id
            .checked_add(1)
            .ok_or_else(|| self.invariant("host frame identity exhausted"))?;
        let next_epoch = state
            .next_epoch
            .checked_add(1)
            .ok_or_else(|| self.invariant("host borrow epoch exhausted"))?;
        state
            .active_frames
            .try_reserve(1)
            .map_err(|_| self.capacity_error())?;
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
            table: self.clone(),
            frame_id,
            epoch,
        })
    }

    pub fn validate(
        &self,
        token: FrameHostBorrowToken,
        required_kind: HostBorrowKind,
    ) -> Result<(), RuntimeError> {
        self.ensure_allowed()?;
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

    pub fn validate_no_escape(value: &Value) -> Result<(), RuntimeError> {
        if value.contains_host_borrow() {
            Err(RuntimeError::host_borrow_escape(
                "frame-scoped host borrow cannot outlive its call frame",
            ))
        } else {
            Ok(())
        }
    }

    pub(super) fn borrow(
        &self,
        frame_id: HostFrameId,
        epoch: BorrowEpoch,
        object_id: HostObjectId,
        borrow_kind: HostBorrowKind,
        type_id: TypeId,
    ) -> Result<FrameHostBorrowToken, RuntimeError> {
        self.ensure_allowed()?;
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
            .map_err(|_| self.capacity_error())?;
        state
            .object_borrows
            .try_reserve(1)
            .map_err(|_| self.capacity_error())?;
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
                    .ok_or_else(|| self.invariant("host shared borrow count exhausted"))?;
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

    pub(super) fn leave_frame(&self, frame_id: HostFrameId, epoch: BorrowEpoch) {
        let mut state = self.state.borrow_mut();
        let Some(frame) = state.active_frames.remove(&frame_id) else {
            self.invariant("host borrow frame disappeared during cleanup");
            return;
        };
        if frame.epoch != epoch {
            self.invariant("host borrow frame epoch changed during cleanup");
        }

        for record in frame.borrows {
            let mut remove_object = false;
            if let Some(object_state) = state.object_borrows.get_mut(&record.object_id) {
                match record.borrow_kind {
                    HostBorrowKind::Shared => {
                        object_state.shared_count =
                            object_state.shared_count.checked_sub(1).unwrap_or_else(|| {
                                self.invariant("host shared borrow count underflow");
                                0
                            });
                    }
                    HostBorrowKind::Unique => {
                        object_state.unique_count =
                            object_state.unique_count.checked_sub(1).unwrap_or_else(|| {
                                self.invariant("host unique borrow count underflow");
                                0
                            });
                    }
                }
                remove_object = object_state.is_empty();
            } else {
                self.invariant("host object borrow disappeared during cleanup");
            }
            if remove_object {
                state.object_borrows.remove(&record.object_id);
            }
        }
    }
}
