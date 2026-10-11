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
    use crate::host_interface::test_support::{decode_fixture, encode_unchecked_fixture};

    use super::*;
    use crate::host_interface::{
        HostFunctionDeclaration, HostInterface, HostInterfaceError, MAGIC, VERSION, codec,
        path::{HostPathDeclaration, HostPathSegmentDeclaration},
        type_declaration::{HostFieldDeclaration, HostTypeDeclaration, PathAccess},
        value_type::HostValueType,
    };
    use bincode::Options;
    use kagari_common::identity::{
        MAX_IDENTITY_PATH_SEGMENTS,
        map::DefinitionContext,
        table::{
            DefinitionId, DefinitionTableError,
            wire::{PortableDefinitionRef, PortableDefinitionTable},
        },
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
        let context = DefinitionContext::new().unwrap();
        let mut bytes = HostInterface::default()
            .to_bytes(&context.snapshot())
            .unwrap();
        bytes[6..14].copy_from_slice(&u64::MAX.to_le_bytes());
        assert_eq!(
            decode_fixture(&bytes, &context),
            Err(HostInterfaceError::Encoding)
        );

        let owner = HostTypeDeclaration::new(&context, "demo.Player").unwrap();
        let field =
            HostFieldDeclaration::new(&context, owner.id, "score", HostValueType::I32).unwrap();
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
            interface.validate(&context.snapshot()),
            Err(HostInterfaceError::TooLarge)
        );
        let bytes = encode_unchecked_fixture(&interface, &context.snapshot()).unwrap();
        assert_eq!(
            decode_fixture(&bytes, &context),
            Err(HostInterfaceError::Encoding)
        );
    }

    #[test]
    fn khi_file_references_must_resolve_before_host_validation() {
        let context = DefinitionContext::new().unwrap();
        let interface = HostInterface {
            types: vec![HostTypeDeclaration::new(&context, "demo.Player").unwrap()],
            ..Default::default()
        };
        let bytes = interface.to_bytes(&context.snapshot()).unwrap();
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
    fn host_identity_path_is_bounded_during_registration() {
        let context = DefinitionContext::new().unwrap();
        let symbol = std::iter::repeat_n("part", MAX_IDENTITY_PATH_SEGMENTS + 2)
            .collect::<Vec<_>>()
            .join(".");
        // A too-deep identity cannot reach fingerprints or encoding: the shared
        // context rejects it before a Host declaration can be constructed.
        assert_eq!(
            HostFunctionDeclaration::new(&context, symbol, vec![], HostValueType::I32),
            Err(DefinitionTableError::PathLimit)
        );
    }
}
