//! Bound portable host declaration sequences before reading their elements.
use crate::decode_limits::bounded_vec;
use serde::{Deserialize, Deserializer};

pub(super) const MAX_DECLARATIONS: usize = 1_000_000;
pub(super) const MAX_MEMBERS: usize = 4_096;

pub(super) fn declarations<'de, D, T>(deserializer: D) -> Result<Vec<T>, D::Error>
where
    D: Deserializer<'de>,
    T: Deserialize<'de>,
{
    bounded_vec(deserializer, MAX_DECLARATIONS, "host declaration")
}

pub(super) fn members<'de, D, T>(deserializer: D) -> Result<Vec<T>, D::Error>
where
    D: Deserializer<'de>,
    T: Deserialize<'de>,
{
    bounded_vec(deserializer, MAX_MEMBERS, "host member")
}

#[cfg(test)]
mod tests {
    use crate::host_interface::{
        HostFunctionDeclaration, HostInterface, HostInterfaceError, MAGIC, VERSION,
        path::{HostPathDeclaration, HostPathSegmentDeclaration},
        type_declaration::{HostFieldDeclaration, HostTypeDeclaration, PathAccess},
        value_type::HostValueType,
    };

    use super::*;

    use bincode::Options;

    #[derive(Debug, Deserialize)]
    struct Counts {
        #[serde(deserialize_with = "members")]
        _members: Vec<u8>,
    }

    #[test]
    fn member_count_is_rejected_before_reading_elements() {
        let error = bincode::DefaultOptions::new()
            .with_fixint_encoding()
            .with_little_endian()
            .deserialize::<Counts>(&u64::MAX.to_le_bytes())
            .unwrap_err();
        assert!(
            error
                .to_string()
                .contains("host member count limit exceeded")
        );
    }

    #[test]
    fn khi_rejects_forged_declaration_and_path_lengths() {
        let mut bytes = HostInterface::default().to_bytes().unwrap();
        bytes[6..14].copy_from_slice(&u64::MAX.to_le_bytes());
        assert_eq!(
            HostInterface::from_bytes(&bytes),
            Err(HostInterfaceError::Encoding)
        );

        let owner = HostTypeDeclaration::new("demo.Player");
        let field = HostFieldDeclaration::new(&owner.id, "score", HostValueType::I32);
        let path = HostPathDeclaration {
            root: owner.id,
            segments: vec![HostPathSegmentDeclaration::Field(field.id); MAX_MEMBERS + 1],
            access: PathAccess::ReadOnly,
            schema_epoch: 0,
            capabilities: Default::default(),
        };
        let interface = HostInterface {
            paths: vec![path.clone()],
            ..Default::default()
        };
        assert_eq!(interface.validate(), Err(HostInterfaceError::TooLarge));
        let bytes = crate::host_interface::codec()
            .serialize(&(
                MAGIC,
                VERSION,
                Vec::<HostTypeDeclaration>::new(),
                Vec::<HostFunctionDeclaration>::new(),
                vec![path],
            ))
            .unwrap();
        assert_eq!(
            HostInterface::from_bytes(&bytes),
            Err(HostInterfaceError::Encoding)
        );
    }

    #[test]
    fn in_memory_host_identity_path_is_bounded_before_encoding() {
        let mut function =
            HostFunctionDeclaration::new("demo.read", Vec::new(), HostValueType::I32);
        function.id.module.path =
            vec!["part".into(); crate::identity::MAX_IDENTITY_PATH_SEGMENTS + 1];
        assert_eq!(function.validate(), Err(HostInterfaceError::TooLarge));
        let interface = HostInterface {
            functions: vec![function],
            ..Default::default()
        };
        assert_eq!(interface.to_bytes(), Err(HostInterfaceError::TooLarge));
    }
}
