//! Opaque return packets authenticate one execution-owned local snapshot.
use crate::execution::space;
use crate::{
    execution::roots::{SessionId, Slots, ValueBank, ValueLocation, frames::FrameMark},
    value::carriers::ScopedValue,
};
use kagari_gc::error::GcError;

/// No heap address or owning root. Ordinary calls need no record; only a produced
/// protected return has this token, which may outlive its public driver step.
#[derive(Debug, Clone, Copy)]
pub(crate) struct ReturnSnapshot {
    execution: SessionId,
    record: usize,
    sequence: u64,
}

pub(super) struct ReturnRecord {
    sequence: u64,
    frame_depth: usize,
    local: usize,
}

impl<'gc> Slots<'_, 'gc> {
    /// Transfer a completed local-stack result into its surrounding execution
    /// owner before removing the callee suffix. The parent may be a Native call
    /// or the activation itself (depth zero). No independent Host lease is made.
    pub(crate) fn publish_parent_return(
        &mut self,
        mark: &mut FrameMark,
        value: ScopedValue<'gc>,
    ) -> Result<ReturnSnapshot, GcError> {
        self.check_frame_exit(mark)?;
        let execution = mark.banks().execution;
        let storage = self.storage(execution)?;
        let sequence = storage
            .return_sequence
            .checked_add(1)
            .ok_or(GcError::Limit("return snapshot identity"))?;
        let local_top = mark.local_top.checked_add(1).ok_or(GcError::SourceLimit)?;
        let return_top = mark.return_top.checked_add(1).ok_or(GcError::SourceLimit)?;
        let additional = usize::from(mark.return_top == storage.returns.len());
        space::reserve(
            &mut storage.returns,
            additional,
            storage.space.as_deref().expect("admitted execution space"),
        )?;
        let location = ValueLocation {
            execution,
            bank: ValueBank::Local,
            slot: mark.local_top,
        };
        if mark.local_top == storage.banks[ValueBank::Local.index()].len() {
            self.push_local(execution, value)?;
        } else {
            self.set(location, value)?;
        }
        // All admission/reservation/publication succeeded. Commit scalar
        // coordinates together; retirement preserves exactly this parent cell.
        let record = ReturnRecord {
            sequence,
            frame_depth: mark.frame_depth - 1,
            local: mark.local_top,
        };
        let storage = self.storage(execution)?;
        if mark.return_top == storage.returns.len() {
            storage.returns.push(record);
        } else {
            storage.returns[mark.return_top] = record;
        }
        storage.return_sequence = sequence;
        let snapshot = ReturnSnapshot {
            execution,
            record: mark.return_top,
            sequence,
        };
        mark.local_top = local_top;
        mark.return_top = return_top;
        Ok(snapshot)
    }

    pub(crate) fn snapshot_return(
        &mut self,
        execution: SessionId,
        value: ScopedValue<'gc>,
    ) -> Result<ReturnSnapshot, GcError> {
        let storage = self.storage(execution)?;
        // Depth zero is the owned activation's completed async result. Its
        // execution source remains live until the external consumer retires it.
        let sequence = storage
            .return_sequence
            .checked_add(1)
            .ok_or(GcError::Limit("return snapshot identity"))?;
        let record = storage.returns.len();
        space::reserve(
            &mut storage.returns,
            1,
            storage.space.as_deref().expect("admitted execution space"),
        )?;
        // Both buffers are reserved before publication. A failed edge import
        // creates neither a record nor a visible partially initialized result.
        let location = self.push_local(execution, value)?;
        let storage = self.storage(execution)?;
        storage.return_sequence = sequence;
        storage.returns.push(ReturnRecord {
            sequence,
            frame_depth: storage.frame_depth,
            local: location.slot,
        });
        Ok(ReturnSnapshot {
            execution,
            record,
            sequence,
        })
    }

    pub(crate) fn return_location(
        &mut self,
        snapshot: ReturnSnapshot,
        execution: SessionId,
    ) -> Result<ValueLocation, GcError> {
        self.storage(execution)?;
        if snapshot.execution.owner != execution.owner {
            return Err(GcError::ForeignHeap);
        }
        if snapshot.execution != execution {
            return Err(GcError::Representation);
        }
        let storage = self.storage(snapshot.execution)?;
        let record = storage
            .returns
            .get(snapshot.record)
            .filter(|record| record.sequence == snapshot.sequence)
            .ok_or(GcError::Index)?;
        if record.frame_depth != storage.frame_depth {
            return Err(GcError::Storage("return belongs to another current frame"));
        }
        storage.banks[ValueBank::Local.index()]
            .get(record.local)
            .ok_or(GcError::Index)?;
        Ok(ValueLocation {
            execution: snapshot.execution,
            bank: ValueBank::Local,
            slot: record.local,
        })
    }
}
