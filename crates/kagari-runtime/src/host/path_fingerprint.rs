//! Versioned fixed-width encoding of resolved path contracts, independent of slots.
use super::*;
use crate::metadata::TypeRegistry;

impl HostRegistry {
    pub(super) fn path_fingerprint(
        &self,
        registration: &HostPathDescriptorRegistration,
        segments: &[HostPathSegment],
        types: &TypeRegistry,
    ) -> Result<AbiFingerprint, RuntimeError> {
        let type_hash = |id| {
            let ty = self
                .host_type(id)
                .map(|info| HostValueType::Opaque(info.declaration.id.clone()))
                .or_else(|| types.host_value_type(id))
                .ok_or_else(|| {
                    RuntimeError::typed_path_validation("path type has no portable host contract")
                })?;
            ty.fingerprint()
                .map_err(|error| RuntimeError::typed_path_validation(error.to_string()))
        };
        let mut encoded = Fingerprint::new();
        // Root contract includes nominal identity, fields, methods and access, but no docs.
        let root = self
            .host_type(registration.root_type)
            .ok_or_else(|| RuntimeError::typed_path_validation("missing path root contract"))?;
        encoded.number(root.abi_fingerprint.0);
        encoded.number(type_hash(registration.result_type)?);
        encoded.number(registration.schema_epoch.0);
        encoded.access(registration.access);
        encoded.capabilities(registration.capability_requirements);
        encoded.number(segments.len() as u64);
        for segment in segments {
            match segment {
                HostPathSegment::Field { owner_type, .. } => {
                    encoded.bytes(&[0]);
                    encoded.number(type_hash(*owner_type)?);
                    // The member fingerprint includes its complete declaration identity.
                }
                HostPathSegment::Index {
                    slot,
                    collection_type,
                    index_type,
                    ..
                } => {
                    encoded.bytes(&[1]);
                    encoded.number(slot.index() as u64);
                    encoded.number(type_hash(*collection_type)?);
                    encoded.number(type_hash(*index_type)?);
                }
                HostPathSegment::Virtual { name, .. } => {
                    encoded.bytes(&[2]);
                    encoded.number(name.len() as u64);
                    encoded.bytes(name.as_bytes());
                }
            }
            encoded.number(type_hash(segment.result_type())?);
            encoded.access(segment.access());
            encoded.number(segment.abi_fingerprint().0);
        }
        Ok(AbiFingerprint(encoded.0))
    }
}

// FNV-1a 64; all counts and integers are u64 little-endian, tags are bytes.
struct Fingerprint(u64);
impl Fingerprint {
    fn new() -> Self {
        let mut result = Self(0xcbf29ce484222325);
        result.bytes(b"kagari-host-path-v1\0");
        result
    }
    fn bytes(&mut self, bytes: &[u8]) {
        for byte in bytes {
            self.0 = (self.0 ^ u64::from(*byte)).wrapping_mul(0x100000001b3);
        }
    }
    fn number(&mut self, value: u64) {
        self.bytes(&value.to_le_bytes());
    }
    fn access(&mut self, access: PathAccess) {
        self.bytes(&[match access {
            PathAccess::None => 0,
            PathAccess::ReadOnly => 1,
            PathAccess::ReadWrite => 2,
        }]);
    }
    fn capabilities(&mut self, capabilities: CapabilitySet) {
        let CapabilitySet {
            fs_read,
            fs_write,
            net,
            clock,
            random,
            host_calls,
            path_mutation,
            reflection_metadata,
            reflection_read,
            reflection_write,
            dynamic_invocation,
            downcast,
            module_loading,
            jit,
            debug_attach,
            debug_breakpoints,
            debug_pause,
            debug_stack_inspection,
            debug_value_inspection,
            debug_host_value_inspection,
            debug_watch_evaluation,
            debug_side_effecting_evaluation,
        } = capabilities;
        self.bytes(
            &[
                fs_read,
                fs_write,
                net,
                clock,
                random,
                host_calls,
                path_mutation,
                reflection_metadata,
                reflection_read,
                reflection_write,
                dynamic_invocation,
                downcast,
                module_loading,
                jit,
                debug_attach,
                debug_breakpoints,
                debug_pause,
                debug_stack_inspection,
                debug_value_inspection,
                debug_host_value_inspection,
                debug_watch_evaluation,
                debug_side_effecting_evaluation,
            ]
            .map(u8::from),
        );
    }
}
