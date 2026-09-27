//! Opaque optional compiler input. Bytecode loading never interprets MIR semantics.
use serde::de::{Error, SeqAccess, Visitor};
use serde::{Deserialize, Deserializer, Serialize};
use std::fmt;

use crate::artifact::MAX_ARTIFACT_BYTES;

/// Presence is not proof of native eligibility. Native preparation must decode,
/// reverify and establish correspondence with the artifact's bytecode program.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PortableMir {
    #[serde(deserialize_with = "decode_bytes")]
    pub bytes: Vec<u8>,
}

fn decode_bytes<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Vec<u8>, D::Error> {
    struct BoundedBytes;
    impl<'de> Visitor<'de> for BoundedBytes {
        type Value = Vec<u8>;

        fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
            formatter.write_str("a bounded portable MIR payload")
        }

        fn visit_seq<A: SeqAccess<'de>>(self, mut sequence: A) -> Result<Vec<u8>, A::Error> {
            let limit = MAX_ARTIFACT_BYTES as usize;
            if sequence.size_hint().is_some_and(|count| count > limit) {
                return Err(Error::custom("portable MIR byte limit exceeded"));
            }
            let mut bytes = Vec::new();
            while let Some(byte) = sequence.next_element()? {
                if bytes.len() >= limit {
                    return Err(Error::custom("portable MIR byte limit exceeded"));
                }
                bytes.push(byte);
            }
            Ok(bytes)
        }
    }
    deserializer.deserialize_seq(BoundedBytes)
}
