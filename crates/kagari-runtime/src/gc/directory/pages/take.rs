//! Extraction changes dispatch while preserving the occupied allocation and mark.
use crate::{
    error::RuntimeError,
    gc::{
        directory::{
            PayloadKind, PayloadLocation,
            descriptors::{Descriptor, DescriptorId},
            pages::{Allocation, Pages},
        },
        object_header::ObjectHeader,
    },
    native::{binding::NativeResult, storage::NativeObject},
};
use std::sync::Arc;

impl Pages {
    /// Move the checked Native record out and leave an edge-free header in its
    /// original slot. The caller takes T before releasing its storage borrow and
    /// destroys the returned metadata outside that borrow. No mark or occupancy
    /// changes, so active cursors and newborn exclusion keep their original owner.
    pub(in crate::gc::directory) fn take_native(
        &mut self,
        location: PayloadLocation,
    ) -> NativeResult<(NativeObject, Arc<Descriptor>)> {
        if location.kind != PayloadKind::Native {
            return Err(RuntimeError::module_validation(
                "invalid Native tombstone source",
            ));
        }
        let checked = self.get(location).ok_or_else(|| {
            RuntimeError::module_validation("unavailable Native tombstone allocation")
        })?;
        let payload = checked.pointer;
        // SAFETY: the occupied descriptor proves the initialized prefix and its
        // alignment. Keep the original pointer provenance for both regions.
        let base = unsafe { payload.sub(checked.descriptor.payload_offset()) };
        let header = unsafe { ObjectHeader::read(base) };
        let old_id =
            DescriptorId::from_header(header.descriptor_id).expect("checked occupied descriptor");
        let old = self
            .descriptors
            .get(old_id)
            .expect("admitted Native descriptor")
            .clone();
        // All allocation/admission can fail before moving any initialized bytes.
        let tombstone = self.descriptors.builtin::<()>(PayloadKind::Taken, false)?;
        let id = self.descriptors.admit(tombstone)?;

        // SAFETY: the exclusive storage owner has checked extraction and excluded
        // loans. This transfers the record, not T; its inline pointer remains in
        // the occupied allocation until the caller immediately takes T.
        let native = unsafe { payload.cast::<NativeObject>().as_ptr().read() };
        // SAFETY: the original prefix remains exclusively writable; the admitted
        // replacement descriptor owns only this header, with no initialized T.
        unsafe {
            ObjectHeader {
                descriptor_id: id.get(),
                ..header
            }
            .write(base)
        };
        match &mut self.allocations[location.page as usize] {
            Allocation::Small(_) => {}
            Allocation::Large(large) => *large.descriptor_mut() = Some(id),
            Allocation::Vacant(_) => unreachable!("checked occupied Native allocation"),
        }
        // The returned Arc prevents final provenance destruction under this borrow.
        self.descriptors.release(old_id);
        Ok((native, old))
    }
}
