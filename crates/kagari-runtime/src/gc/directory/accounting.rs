//! Shallow attribution belongs beside the concrete storage owner.
use crate::{
    diagnostics::storage::{BufferUsage, StorageUsage},
    gc::directory::ObjectStorage,
    native::sequence::SequencePayload,
};
use std::sync::Arc;

impl ObjectStorage {
    pub(crate) fn storage_usage(&self, free_identities: BufferUsage) -> StorageUsage {
        let stores = &self.payloads;
        let distinct_struct_layouts = stores
            .structs
            .iter()
            .enumerate()
            .filter(|(index, entry)| {
                !stores.structs[..*index].iter().any(|prior| {
                    Arc::ptr_eq(&prior.payload.description, &entry.payload.description)
                })
            })
            .count();
        let mut native_payload_bytes = 0;
        let mut sequence_element_bytes = 0;
        let mut sequence_capacity_bytes = 0;
        for entry in &stores.natives {
            native_payload_bytes += entry.payload.payload_bytes();
            if let Ok(sequence) = entry.payload.payload::<SequencePayload>() {
                let (used, capacity) = sequence.values.backing_bytes();
                sequence_element_bytes += used;
                sequence_capacity_bytes += capacity;
            }
        }
        StorageUsage {
            buffers: [
                BufferUsage::new("directory", &self.slots),
                BufferUsage::new("strings", &stores.strings),
                BufferUsage::new("tuples", &stores.tuples),
                BufferUsage::new("ranges", &stores.ranges),
                BufferUsage::new("host_roots", &stores.host_roots),
                BufferUsage::new("host_paths", &stores.host_paths),
                BufferUsage::new("ephemerals", &stores.ephemerals),
                BufferUsage::new("natives", &stores.natives),
                BufferUsage::new("enums", &stores.enums),
                BufferUsage::new("struct_records", &stores.structs),
                BufferUsage::new("struct_fields", &stores.struct_bytes),
                BufferUsage::new("interface_wrappers", &stores.interfaces),
                BufferUsage::new("closures", &stores.closures),
                BufferUsage::new("cells", &stores.cells),
                free_identities,
            ],
            distinct_struct_layouts,
            initialization_owners: self.initialization_owners.len(),
            initialization_owner_capacity: self.initialization_owners.capacity(),
            native_payload_bytes,
            sequence_element_bytes,
            sequence_capacity_bytes,
        }
    }
}
