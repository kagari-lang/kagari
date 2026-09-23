//! Bound portable host declaration sequences before reading their elements.
use serde::{Deserialize, Deserializer};

pub(super) const MAX_DECLARATIONS: usize = 1_000_000;
pub(super) const MAX_MEMBERS: usize = 4_096;

pub(super) fn declarations<'de, D, T>(deserializer: D) -> Result<Vec<T>, D::Error>
where
    D: Deserializer<'de>,
    T: Deserialize<'de>,
{
    crate::decode_limits::bounded_vec(deserializer, MAX_DECLARATIONS, "host declaration")
}

pub(super) fn members<'de, D, T>(deserializer: D) -> Result<Vec<T>, D::Error>
where
    D: Deserializer<'de>,
    T: Deserialize<'de>,
{
    crate::decode_limits::bounded_vec(deserializer, MAX_MEMBERS, "host member")
}

#[cfg(test)]
mod tests {
    use super::super::{
        HostFieldDeclaration, HostFieldPathDeclaration, HostFunctionDeclaration, HostInterface,
        HostInterfaceError, HostTypeDeclaration, MAGIC, PathAccess, VERSION,
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
        let field = HostFieldDeclaration::new(&owner.id, "score", super::super::HostValueType::I32);
        let path = HostFieldPathDeclaration {
            root: owner.id,
            fields: vec![field.id; MAX_MEMBERS + 1],
            access: PathAccess::ReadOnly,
            schema_epoch: 0,
            capabilities: Default::default(),
        };
        let interface = HostInterface {
            field_paths: vec![path.clone()],
            ..Default::default()
        };
        assert_eq!(interface.validate(), Err(HostInterfaceError::TooLarge));
        let bytes = super::super::codec()
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
}
