//! Disjoint backing attribution belongs beside the physical storage owner.
use crate::{
    diagnostics::storage::{BufferUsage, StorageUsage},
    gc::array_storage::ArrayPayload,
    gc::directory::ObjectStorage,
};

impl ObjectStorage {
    pub(crate) fn storage_usage(
        &self,
        interface_snapshots: BufferUsage,
        graph_pending: BufferUsage,
        graph_credits: usize,
    ) -> StorageUsage {
        let mut native_payload_bytes = 0;
        let mut array_element_bytes = 0;
        let mut array_capacity_bytes = 0;
        let mut initialized_payload_bytes = 0;
        let mut object_header_bytes = 0;
        for (_, location) in self.pages.occupied() {
            let (_, descriptor) = self.pages.get(location).expect("occupied descriptor");
            initialized_payload_bytes += descriptor.layout.size();
            object_header_bytes += descriptor.payload_offset();
            if let Some(object) = self.native(location) {
                native_payload_bytes += object.payload_bytes();
                if let Ok(sequence) = object.payload::<ArrayPayload>() {
                    let (used, capacity) = sequence.values.backing_bytes();
                    array_element_bytes += used;
                    array_capacity_bytes += capacity;
                }
            }
        }
        let usage = self.pages.usage();
        let (
            descriptor_bytes,
            representation_descriptors,
            descriptor_directory_bytes,
            descriptor_registry_entries,
            descriptor_registry_capacity,
        ) = self.pages.descriptors.usage();
        let (struct_identity_bytes, struct_identities) =
            self.pages.descriptors.struct_identity_usage();
        StorageUsage {
            buffers: [
                BufferUsage::new("directory", &self.slots),
                BufferUsage::new("interface_wrappers", &self.interface_directories),
                graph_pending,
            ],
            graph_credits,
            graph_work_credits: 0,
            struct_mark_pages: usage.mark_pages,
            struct_mark_bytes: usage.mark_bytes,
            retirement: Default::default(),
            slab_bytes: usage.slabs,
            large_bytes: usage.large,
            slot_metadata_bytes: usage.slots,
            bitmap_bytes: usage.bitmaps,
            page_directory_bytes: usage.directory,
            page_metadata_bytes: usage.small_controls,
            large_metadata_bytes: usage.large_controls,
            backing_control_bytes: usage.backing_controls,
            allocation_padding_bytes: usage.padding,
            descriptor_bytes,
            representation_descriptors,
            struct_identity_bytes,
            struct_identities,
            descriptor_directory_bytes,
            descriptor_registry_entries,
            descriptor_registry_capacity,
            initialized_payload_bytes,
            object_header_bytes,
            unused_backing_bytes: usage.slabs + usage.large - initialized_payload_bytes,
            interface_snapshots,
            distinct_struct_layouts: self.pages.descriptors.structures(),
            initialization_owners: self.initialization_owners.len(),
            initialization_owner_capacity: self.initialization_owners.capacity(),
            native_payload_bytes,
            array_element_bytes,
            array_capacity_bytes,
        }
    }
}
