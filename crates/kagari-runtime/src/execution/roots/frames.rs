//! Ordinary calls own suffix marks in the same banks as Native/local protection.
#[cfg(feature = "execution-diagnostics")]
use super::diagnostics;
use super::reserve_bank;
#[cfg(test)]
mod tests;
use crate::{
    execution::roots::{ExecutionRoots, ExecutionStorage, LocalMark, SessionId, Slots, ValueBank},
    value::carriers::StoredValue,
};
use kagari_gc::error::GcError;

/// Non-owning frame coordinates. The owning frame keeps its linear mark alive;
/// cross-activation handles still validate the complete execution identity.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct FrameBanks {
    pub(crate) execution: SessionId,
    pub(crate) managed: usize,
    pub(crate) managed_count: usize,
    pub(crate) scalars: usize,
    pub(crate) scalar_count: usize,
}

/// One ordinary call's storage lifetime. No window directory/generation, managed
/// value copy or owning Host root is hidden in this control record.
pub(crate) struct FrameMark {
    banks: FrameBanks,
    pub(super) local_top: usize,
    dependency_top: usize,
    pub(super) frame_depth: usize,
    native_depth: usize,
    pub(super) return_top: usize,
}

impl FrameMark {
    pub(crate) fn banks(&self) -> FrameBanks {
        self.banks
    }
}

impl ExecutionRoots {
    pub(crate) fn contains_frame_banks(&self, banks: FrameBanks) -> bool {
        let Ok(storage) = self.inspect_storage(banks.execution) else {
            return false;
        };
        storage.contains_frame_banks(banks)
    }
}

impl ExecutionStorage {
    /// Geometry only, after the source owner admitted this execution's identity.
    pub(super) fn contains_frame_banks(&self, banks: FrameBanks) -> bool {
        let Some(managed_end) = banks.managed.checked_add(banks.managed_count) else {
            return false;
        };
        let Some(scalar_end) = banks.scalars.checked_add(banks.scalar_count) else {
            return false;
        };
        managed_end <= self.banks[ValueBank::Managed.index()].len()
            && scalar_end <= self.scalars.len()
            && scalar_end <= self.initialized.len()
    }
}

impl Slots<'_, '_> {
    /// The frame owner admits all original argument/dependency coordinates before
    /// growth. Reserve every bank first, then publish initialized empty storage.
    pub(crate) fn enter_frame<E: From<GcError>>(
        &mut self,
        execution: SessionId,
        managed_count: usize,
        scalar_count: usize,
        mut poll: impl FnMut() -> Result<(), E>,
    ) -> Result<FrameMark, E> {
        let storage = self.storage(execution)?;
        let managed = storage.banks[ValueBank::Managed.index()].len();
        let scalars = storage.scalars.len();
        if scalars != storage.initialized.len() {
            return Err(GcError::Storage("inconsistent scalar initialization bank").into());
        }
        let managed_end = managed
            .checked_add(managed_count)
            .ok_or(GcError::SourceLimit)?;
        let scalar_end = scalars
            .checked_add(scalar_count)
            .ok_or(GcError::SourceLimit)?;
        let frame_depth = storage
            .frame_depth
            .checked_add(1)
            .ok_or(GcError::SourceLimit)?;
        reserve_bank(
            &mut storage.banks[ValueBank::Managed.index()],
            managed_count,
            storage.space.as_deref().expect("admitted execution space"),
        )?;
        reserve_bank(
            &mut storage.scalars,
            scalar_count,
            storage.space.as_deref().expect("admitted execution space"),
        )?;
        reserve_bank(
            &mut storage.initialized,
            scalar_count,
            storage.space.as_deref().expect("admitted execution space"),
        )?;
        let mark = FrameMark {
            banks: FrameBanks {
                execution,
                managed,
                managed_count,
                scalars,
                scalar_count,
            },
            local_top: storage.banks[ValueBank::Local.index()].len(),
            dependency_top: storage.dependencies.len(),
            frame_depth,
            native_depth: storage.native_calls.len(),
            return_top: storage.returns.len(),
        };
        // All pushes are reserved and edge-free. Poll large initialization in
        // bounded chunks; a rejection removes the whole unpublished suffix.
        let initialize: Result<(), E> = (|| {
            while storage.banks[ValueBank::Managed.index()].len() < managed_end {
                let next = storage.banks[ValueBank::Managed.index()]
                    .len()
                    .saturating_add(1024)
                    .min(managed_end);
                #[cfg(feature = "execution-diagnostics")]
                let added = next - storage.banks[ValueBank::Managed.index()].len();
                storage.banks[ValueBank::Managed.index()].resize_with(next, StoredValue::unit);
                #[cfg(feature = "execution-diagnostics")]
                diagnostics::initialized::<StoredValue>(added);
                if next != managed_end {
                    poll()?;
                }
            }
            while storage.scalars.len() < scalar_end {
                let next = storage.scalars.len().saturating_add(1024).min(scalar_end);
                #[cfg(feature = "execution-diagnostics")]
                let added = next - storage.scalars.len();
                storage.scalars.resize(next, 0);
                storage.initialized.resize(next, false);
                #[cfg(feature = "execution-diagnostics")]
                {
                    diagnostics::initialized::<u64>(added);
                    diagnostics::initialized::<bool>(added);
                }
                if next != scalar_end {
                    poll()?;
                }
            }
            Ok(())
        })();
        if let Err(error) = initialize {
            storage.banks[ValueBank::Managed.index()].truncate(managed);
            storage.scalars.truncate(scalars);
            storage.initialized.truncate(scalars);
            return Err(error);
        }
        storage.frame_depth = frame_depth;
        Ok(mark)
    }

    /// Transfer a preparation prefix into the immediately created callee's
    /// linear owner. No buffers move and no source is retired across a live call.
    /// Validation leaves both marks unchanged; commit consumes the preparation.
    pub(crate) fn adopt_frame_preparation(
        &mut self,
        frame: &mut FrameMark,
        preparation: &mut Option<LocalMark>,
    ) -> Result<(), GcError> {
        self.check_frame_exit(frame)?;
        let prepared = preparation
            .as_ref()
            .ok_or(GcError::Storage("call preparation already adopted"))?;
        if prepared.execution != frame.banks.execution
            || prepared.frame_depth.checked_add(1) != Some(frame.frame_depth)
            || prepared.native_depth != frame.native_depth
            || prepared.top > frame.local_top
            || prepared.dependency_top > frame.dependency_top
            || prepared.return_top > frame.return_top
        {
            return Err(GcError::Storage(
                "call preparation does not precede this frame",
            ));
        }
        frame.local_top = prepared.top;
        frame.dependency_top = prepared.dependency_top;
        frame.return_top = prepared.return_top;
        preparation.take();
        Ok(())
    }

    /// Check before a caller result/control commit. A Native callback cannot
    /// retire its script caller, nor can a frame discard an unfinished child.
    pub(crate) fn check_frame_exit(&mut self, mark: &FrameMark) -> Result<(), GcError> {
        let storage = self.storage(mark.banks.execution)?;
        if storage.frame_depth != mark.frame_depth
            || storage.native_calls.len() != mark.native_depth
        {
            return Err(GcError::Storage("frame retirement is out of order"));
        }
        if storage.banks[ValueBank::Managed.index()].len()
            != mark.banks.managed + mark.banks.managed_count
            || storage.scalars.len() != mark.banks.scalars + mark.banks.scalar_count
            || storage.initialized.len() != storage.scalars.len()
            || storage.banks[ValueBank::Local.index()].len() < mark.local_top
            || storage.dependencies.len() < mark.dependency_top
            || storage.returns.len() < mark.return_top
        {
            return Err(GcError::Storage(
                "frame suffix was changed outside its owner",
            ));
        }
        Ok(())
    }

    /// The caller has published any returned value before consuming this mark.
    /// Whole-execution retirement may instead drop all banks together.
    pub(crate) fn retire_frame(&mut self, mark: FrameMark) -> Result<(), GcError> {
        self.check_frame_exit(&mark)?;
        let storage = self.storage(mark.banks.execution)?;
        storage.banks[ValueBank::Managed.index()].truncate(mark.banks.managed);
        storage.scalars.truncate(mark.banks.scalars);
        storage.initialized.truncate(mark.banks.scalars);
        storage.banks[ValueBank::Local.index()].truncate(mark.local_top);
        storage.dependencies.truncate(mark.dependency_top);
        storage.returns.truncate(mark.return_top);
        storage.frame_depth -= 1;
        Ok(())
    }

    pub(crate) fn check_frame_banks(&mut self, banks: FrameBanks) -> Result<(), GcError> {
        if !self.source.contains_frame_banks(banks) {
            return Err(GcError::Index);
        }
        Ok(())
    }
}
