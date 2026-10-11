//! Bound portable host declaration sequences before reading their elements.
use kagari_common::decode_limits::bounded_vec;
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
    use crate::host_interface::test_support::{
        FingerprintFixture, InterfaceFixture, ValidateFixture, decode_fixture,
        encode_unchecked_fixture,
    };

    use super::*;
    use crate::host_interface::{
        HostFunctionDeclaration, HostInterface, HostInterfaceError, MAGIC, VERSION, codec,
        path::{HostPathDeclaration, HostPathSegmentDeclaration},
        type_declaration::{HostFieldDeclaration, HostTypeDeclaration, PathAccess},
        value_type::HostValueType,
    };
    use bincode::Options;
    use kagari_common::identity::table::{
        DefinitionId,
        wire::{PortableDefinitionRef, PortableDefinitionTable},
    };

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
        let mut bytes = HostInterface::default().encode_fixture().unwrap();
        bytes[6..14].copy_from_slice(&u64::MAX.to_le_bytes());
        assert_eq!(decode_fixture(&bytes), Err(HostInterfaceError::Encoding));

        let owner = HostTypeDeclaration::new("demo.Player");
        let field = HostFieldDeclaration::new(&owner.id, "score", HostValueType::I32);
        let path = HostPathDeclaration {
            root: owner.id,
            segments: vec![HostPathSegmentDeclaration::Field(field.id); MAX_MEMBERS + 1],
            access: PathAccess::ReadOnly,
            schema_epoch: 0,
        };
        let interface = HostInterface {
            paths: vec![path.clone()],
            ..Default::default()
        };
        assert_eq!(
            interface.validate_fixture(),
            Err(HostInterfaceError::TooLarge)
        );
        let bytes = encode_unchecked_fixture(&interface).unwrap();
        assert_eq!(decode_fixture(&bytes), Err(HostInterfaceError::Encoding));
    }

    #[test]
    fn khi_file_references_must_resolve_before_host_validation() {
        let interface = HostInterface {
            types: vec![HostTypeDeclaration::new("demo.Player")],
            ..Default::default()
        };
        let bytes = interface.encode_fixture().unwrap();
        let (mut records, table): (
            HostInterface<PortableDefinitionRef>,
            PortableDefinitionTable,
        ) = codec().deserialize(&bytes[6..]).unwrap();
        records.types[0].id = codec().deserialize(&u32::MAX.to_le_bytes()).unwrap();
        let mut invalid = MAGIC.to_vec();
        invalid.extend(VERSION.to_le_bytes());
        invalid.extend(codec().serialize(&(records, table)).unwrap());
        assert!(matches!(
            HostInterface::<DefinitionId>::from_bytes(&invalid),
            Err(HostInterfaceError::InvalidDeclaration)
        ));
    }

    #[test]
    fn in_memory_host_identity_path_is_bounded_before_encoding() {
        let mut function =
            HostFunctionDeclaration::new("demo.read", Vec::new(), HostValueType::I32);
        function.id.module.path =
            vec!["part".into(); kagari_common::identity::MAX_IDENTITY_PATH_SEGMENTS + 1];
        assert_eq!(
            function.fingerprint_fixture(),
            Err(HostInterfaceError::TooLarge)
        );
        let interface = HostInterface {
            functions: vec![function],
            ..Default::default()
        };
        assert_eq!(
            interface.encode_fixture(),
            Err(HostInterfaceError::TooLarge)
        );
    }
}
