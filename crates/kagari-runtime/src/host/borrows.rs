use crate::{
    error::{RuntimeError, fault::EngineFault},
    host::{
        ActiveBorrowFrame, BorrowEpoch, FrameBorrowRecord, FrameHostBorrowToken, HostBorrowKind,
        HostBorrowOwner, HostBorrowTable, HostCallGuard, HostFrameAccess, HostFrameId,
        HostFrameMark, HostObjectId, HostRegistryId, HostRootHandle,
    },
    metadata::TypeId,
    resource::ResourceState,
};

impl FrameHostBorrowToken {
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
    fn matches(&self, token: FrameHostBorrowToken) -> bool {
        self.object_id == token.object_id
            && self.borrow_kind.satisfies(token.borrow_kind)
            && self.type_id == token.type_id
    }
}

impl HostBorrowTable {
    pub(crate) fn for_registry(registry: HostRegistryId) -> Self {
        Self {
            registry: Some(registry),
            ..Self::default()
        }
    }

    // Retention does not grant access. Even after parking, a consumer must
    // authenticate the complete handle against the registry's live roots.
    pub(crate) fn permits_root_retention(&self, root: HostRootHandle) -> bool {
        self.registry == Some(root.owner) && !self.is_borrowed(root.object_id)
    }

    pub(super) fn is_borrowed(&self, object: HostObjectId) -> bool {
        self.state.borrow().object_borrows.contains_key(&object)
    }

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
            .ok_or_else(|| invariant(resources, EngineFault::HostFrameIdentityExhausted))?;
        let next_epoch = state
            .next_epoch
            .checked_add(1)
            .ok_or_else(|| invariant(resources, EngineFault::HostBorrowEpochExhausted))?;
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
            frame: Some(HostFrameAccess::Owned(HostFrameMark { frame_id, epoch })),
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

    pub(super) fn borrow(
        &self,
        frame: &HostCallGuard<'_>,
        object_id: HostObjectId,
        borrow_kind: HostBorrowKind,
        type_id: TypeId,
    ) -> Result<FrameHostBorrowToken, RuntimeError> {
        let frame_id = frame.frame_id();
        let epoch = frame.epoch();
        let resources = frame.resources;
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
        let object = state.object_borrows.entry(object_id).or_default();
        if object.unique || (borrow_kind == HostBorrowKind::Unique && object.shared_count != 0) {
            return Err(RuntimeError::host_borrow_conflict(format!(
                "host object {} has an incompatible borrow",
                object_id.0
            )));
        }
        match borrow_kind {
            HostBorrowKind::Shared => {
                object.shared_count = object.shared_count.checked_add(1).ok_or_else(|| {
                    invariant(resources, EngineFault::HostSharedBorrowCountExhausted)
                })?;
            }
            HostBorrowKind::Unique => object.unique = true,
        }

        state
            .active_frames
            .get_mut(&frame_id)
            .expect("checked active host borrow frame before recording token")
            .borrows
            .push(FrameBorrowRecord {
                object_id,
                borrow_kind,
                type_id,
            });
        Ok(FrameHostBorrowToken::new(
            self.owner,
            frame_id,
            object_id,
            borrow_kind,
            type_id,
            epoch,
        ))
    }

    pub(super) fn leave_frame(
        &self,
        frame_id: HostFrameId,
        epoch: BorrowEpoch,
        resources: Option<&ResourceState>,
    ) {
        let mut state = self.state.borrow_mut();
        let Some(frame) = state.active_frames.remove(&frame_id) else {
            invariant(
                resources,
                EngineFault::HostBorrowFrameDisappearedDuringCleanup,
            );
            return;
        };
        if frame.epoch != epoch {
            invariant(
                resources,
                EngineFault::HostBorrowFrameEpochChangedDuringCleanup,
            );
        }

        for record in frame.borrows {
            let Some(object) = state.object_borrows.get_mut(&record.object_id) else {
                invariant(
                    resources,
                    EngineFault::HostObjectBorrowDisappearedDuringCleanup,
                );
                continue;
            };
            match record.borrow_kind {
                HostBorrowKind::Shared => {
                    object.shared_count = object.shared_count.checked_sub(1).unwrap_or_else(|| {
                        invariant(resources, EngineFault::HostSharedBorrowCountUnderflow);
                        0
                    });
                }
                HostBorrowKind::Unique => {
                    if !object.unique {
                        invariant(
                            resources,
                            EngineFault::HostUniqueBorrowDisappearedDuringCleanup,
                        );
                    }
                    object.unique = false;
                }
            }
            if object.shared_count == 0 && !object.unique {
                state.object_borrows.remove(&record.object_id);
            }
        }
    }
}

fn invariant(resources: Option<&ResourceState>, message: EngineFault) -> RuntimeError {
    resources.map_or_else(
        || RuntimeError::from(message),
        |resources| resources.quarantine(message),
    )
}

fn capacity_error(resources: Option<&ResourceState>) -> RuntimeError {
    resources.map_or_else(
        || RuntimeError::resource_limit("host borrow capacity"),
        |resources| resources.limit("host borrow capacity"),
    )
}
