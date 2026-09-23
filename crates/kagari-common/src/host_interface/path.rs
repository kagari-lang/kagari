//! Portable resolved path contracts shared by offline tools and runtime registration.
use super::{
    CapabilitySet, DefinitionId, HostInterface, HostInterfaceError, HostTypeOwnership,
    HostValueType, PathAccess, Visibility,
};

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct HostFieldPathDeclaration {
    pub root: DefinitionId,
    #[serde(deserialize_with = "super::decode_limits::members")]
    pub fields: Vec<DefinitionId>,
    pub access: PathAccess,
    pub schema_epoch: u64,
    pub capabilities: CapabilitySet,
}
impl HostFieldPathDeclaration {
    pub fn contract(
        &self,
        interface: &HostInterface,
    ) -> Result<HostPathContract, HostInterfaceError> {
        interface.field_path_contract(
            &self.root,
            &self.fields,
            self.access,
            self.schema_epoch,
            self.capabilities,
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HostPathInput {
    Field {
        owner: HostValueType,
    },
    Index {
        slot: u64,
        collection: HostValueType,
        index: HostValueType,
    },
    Virtual {
        name: String,
    },
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HostPathSegmentContract {
    pub input: HostPathInput,
    pub result: HostValueType,
    pub access: PathAccess,
    pub member_fingerprint: u64,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HostPathContract {
    pub root_fingerprint: u64,
    pub result: HostValueType,
    pub schema_epoch: u64,
    pub access: PathAccess,
    pub capabilities: CapabilitySet,
    pub segments: Vec<HostPathSegmentContract>,
}
impl HostPathContract {
    pub fn fingerprint(&self) -> Result<u64, HostInterfaceError> {
        if self.segments.is_empty()
            || self.access == PathAccess::None
            || self
                .segments
                .last()
                .is_none_or(|segment| segment.result != self.result)
        {
            return Err(HostInterfaceError::InvalidDeclaration);
        }
        let mut encoded = Fingerprint::new();
        encoded.number(self.root_fingerprint);
        encoded.number(self.result.fingerprint()?);
        encoded.number(self.schema_epoch);
        encoded.access(self.access);
        encoded.capabilities(self.capabilities);
        encoded.number(self.segments.len() as u64);
        for segment in &self.segments {
            if !allows(segment.access, self.access) {
                return Err(HostInterfaceError::InvalidDeclaration);
            }
            match &segment.input {
                HostPathInput::Field { owner } => {
                    encoded.bytes(&[0]);
                    encoded.number(owner.fingerprint()?);
                }
                HostPathInput::Index {
                    slot,
                    collection,
                    index,
                } => {
                    encoded.bytes(&[1]);
                    encoded.number(*slot);
                    encoded.number(collection.fingerprint()?);
                    encoded.number(index.fingerprint()?);
                }
                HostPathInput::Virtual { name } => {
                    encoded.bytes(&[2]);
                    encoded.number(name.len() as u64);
                    encoded.bytes(name.as_bytes());
                }
            }
            encoded.number(segment.result.fingerprint()?);
            encoded.access(segment.access);
            encoded.number(segment.member_fingerprint);
        }
        Ok(encoded.0)
    }
}
impl HostInterface {
    /// Resolve a field chain entirely from declarations, without invoking runtime bindings.
    pub fn field_path_contract(
        &self,
        root: &DefinitionId,
        fields: &[DefinitionId],
        access: PathAccess,
        schema_epoch: u64,
        capabilities: CapabilitySet,
    ) -> Result<HostPathContract, HostInterfaceError> {
        self.validate()?;
        self.resolve_field_path(root, fields, access, schema_epoch, capabilities)
    }
    pub(super) fn resolve_field_path(
        &self,
        root: &DefinitionId,
        fields: &[DefinitionId],
        access: PathAccess,
        schema_epoch: u64,
        capabilities: CapabilitySet,
    ) -> Result<HostPathContract, HostInterfaceError> {
        if fields.len() > 256 {
            return Err(HostInterfaceError::TooLarge);
        }
        let root = self
            .types
            .iter()
            .find(|ty| &ty.id == root)
            .ok_or(HostInterfaceError::InvalidDeclaration)?;
        if root.ownership != HostTypeOwnership::HostRoot
            || !allows(root.path_access, access)
            || access == PathAccess::None
            || fields.is_empty()
        {
            return Err(HostInterfaceError::InvalidDeclaration);
        }
        let mut current = HostValueType::Opaque(root.id.clone());
        let mut segments = Vec::with_capacity(fields.len());
        for field_id in fields {
            let HostValueType::Opaque(owner) = &current else {
                return Err(HostInterfaceError::InvalidDeclaration);
            };
            let owner = self
                .types
                .iter()
                .find(|ty| &ty.id == owner)
                .ok_or(HostInterfaceError::InvalidDeclaration)?;
            let field = owner
                .fields
                .iter()
                .find(|field| &field.id == field_id)
                .ok_or(HostInterfaceError::InvalidDeclaration)?;
            if field.visibility != Visibility::Public || !allows(field.path_access, access) {
                return Err(HostInterfaceError::InvalidDeclaration);
            }
            segments.push(HostPathSegmentContract {
                input: HostPathInput::Field { owner: current },
                result: field.ty.clone(),
                access: field.path_access,
                member_fingerprint: field.fingerprint()?,
            });
            current = field.ty.clone();
        }
        Ok(HostPathContract {
            root_fingerprint: root.fingerprint()?,
            result: current,
            schema_epoch,
            access,
            capabilities,
            segments,
        })
    }
}
fn allows(available: PathAccess, requested: PathAccess) -> bool {
    matches!(
        (available, requested),
        (
            PathAccess::ReadWrite,
            PathAccess::ReadOnly | PathAccess::ReadWrite
        ) | (PathAccess::ReadOnly, PathAccess::ReadOnly)
    )
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::host_interface::{HostFieldDeclaration, HostTypeDeclaration};

    #[test]
    fn nested_field_contracts_resolve_nominal_owners_without_runtime_registration() {
        let mut child = HostTypeDeclaration::new("game.Child");
        let mut count = HostFieldDeclaration::new(&child.id, "count", HostValueType::I32);
        count.path_access = PathAccess::ReadOnly;
        child.fields.push(count.clone());
        let mut root = HostTypeDeclaration::new("game.Root");
        root.ownership = HostTypeOwnership::HostRoot;
        root.path_access = PathAccess::ReadOnly;
        let mut nested =
            HostFieldDeclaration::new(&root.id, "child", HostValueType::Opaque(child.id.clone()));
        nested.path_access = PathAccess::ReadOnly;
        root.fields.push(nested.clone());
        let catalog = HostInterface {
            field_paths: vec![],
            types: vec![root.clone(), child],
            functions: vec![],
        };
        let fields = [nested.id.clone(), count.id.clone()];
        let contract = catalog
            .field_path_contract(
                &root.id,
                &fields,
                PathAccess::ReadOnly,
                7,
                CapabilitySet::default(),
            )
            .unwrap();
        let declaration = HostFieldPathDeclaration {
            root: root.id.clone(),
            fields: fields.to_vec(),
            access: PathAccess::ReadOnly,
            schema_epoch: 7,
            capabilities: CapabilitySet::default(),
        };
        let mut published = catalog.clone();
        let mut next_schema = declaration.clone();
        next_schema.schema_epoch = 8;
        published.field_paths = vec![declaration.clone(), next_schema];
        let encoded = published.to_bytes().unwrap();
        published.field_paths.reverse();
        assert_eq!(encoded, published.to_bytes().unwrap());
        let decoded_paths = HostInterface::from_bytes(&encoded).unwrap();
        assert_eq!(decoded_paths.field_paths.len(), 2);
        assert!(
            decoded_paths
                .field_paths
                .iter()
                .all(|path| path.contract(&decoded_paths).is_ok())
        );
        published.field_paths.push(declaration.clone());
        assert_eq!(
            published.validate(),
            Err(HostInterfaceError::DuplicateDeclaration)
        );
        published.field_paths = vec![declaration];
        published.field_paths[0].fields = vec![count.id.clone(); 257];
        assert_eq!(published.validate(), Err(HostInterfaceError::TooLarge));
        assert_eq!(contract.result, HostValueType::I32);
        assert_eq!(contract.segments.len(), 2);
        let decoded = HostInterface::from_bytes(&catalog.to_bytes().unwrap()).unwrap();
        assert_eq!(
            contract.fingerprint().unwrap(),
            decoded
                .field_path_contract(
                    &root.id,
                    &fields,
                    PathAccess::ReadOnly,
                    7,
                    CapabilitySet::default()
                )
                .unwrap()
                .fingerprint()
                .unwrap()
        );
        for invalid in [
            vec![],
            vec![count.id.clone()],
            vec![nested.id.clone(), nested.id],
            vec![fields[0].clone(), count.id.clone(), count.id],
        ] {
            assert!(
                catalog
                    .field_path_contract(
                        &root.id,
                        &invalid,
                        PathAccess::ReadOnly,
                        7,
                        CapabilitySet::default()
                    )
                    .is_err()
            );
        }
    }
}
