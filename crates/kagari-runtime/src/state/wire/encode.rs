use crate::state::data::StateValue;
use serde::{Serialize, Serializer, ser::SerializeTuple};

impl Serialize for StateValue {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        // Tuple lengths are Serde bookkeeping, not bytes in the bincode framing.
        // Tags are explicit protocol constants, independent of Rust variant order.
        let fields = match self {
            Self::Unit => 1,
            Self::Record { .. } => 3,
            Self::Enum { .. } => 4,
            _ => 2,
        };
        let mut tuple = serializer.serialize_tuple(fields)?;
        macro_rules! scalar {
            ($tag:expr, $value:expr) => {{
                tuple.serialize_element(&$tag)?;
                tuple.serialize_element($value)?;
            }};
        }
        match self {
            Self::Unit => tuple.serialize_element(&0_u32)?,
            Self::Bool(value) => scalar!(1_u32, value),
            Self::Char(value) => scalar!(2_u32, &u32::from(*value)),
            Self::I8(value) => scalar!(3_u32, value),
            Self::I16(value) => scalar!(4_u32, value),
            Self::I32(value) => scalar!(5_u32, value),
            Self::I64(value) => scalar!(6_u32, value),
            Self::U8(value) => scalar!(7_u32, value),
            Self::U16(value) => scalar!(8_u32, value),
            Self::U32(value) => scalar!(9_u32, value),
            Self::U64(value) => scalar!(10_u32, value),
            Self::F32Bits(value) => scalar!(11_u32, value),
            Self::F64Bits(value) => scalar!(12_u32, value),
            Self::String(value) => scalar!(13_u32, value),
            Self::Tuple(values) => scalar!(14_u32, values),
            Self::Sequence(values) => scalar!(15_u32, values),
            Self::Record { key, fields } => {
                scalar!(16_u32, key);
                tuple.serialize_element(fields)?;
            }
            Self::Enum {
                key,
                variant,
                fields,
            } => {
                scalar!(17_u32, key);
                tuple.serialize_element(variant)?;
                tuple.serialize_element(fields)?;
            }
        }
        tuple.end()
    }
}
