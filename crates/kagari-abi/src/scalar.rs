use kagari_common::{integer, numeric::NumberType};
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, serde::Serialize, serde::Deserialize,
)]
pub enum BuiltinType {
    Unit,
    Bool,
    I8,
    I16,
    I32,
    I64,
    ISize,
    U8,
    U16,
    U32,
    U64,
    USize,
    F32,
    F64,
    String,
}

impl BuiltinType {
    pub fn integer_bounds(self) -> Option<(i128, i128)> {
        self.integer_layout()
            .map(|(bits, signed)| integer::bounds(bits, signed))
    }

    pub fn integer_layout(self) -> Option<(u32, bool)> {
        Some(match self {
            BuiltinType::I8 => (8, true),
            BuiltinType::I16 => (16, true),
            BuiltinType::I32 => (32, true),
            BuiltinType::I64 | BuiltinType::ISize => (64, true),
            BuiltinType::U8 => (8, false),
            BuiltinType::U16 => (16, false),
            BuiltinType::U32 => (32, false),
            BuiltinType::U64 | BuiltinType::USize => (64, false),
            _ => return None,
        })
    }
}

impl BuiltinType {
    pub fn number_type(self) -> Option<NumberType> {
        match self {
            Self::F32 => Some(NumberType::F32),
            Self::F64 => Some(NumberType::F64),
            _ => self
                .integer_layout()
                .map(|(bits, signed)| NumberType::Integer { bits, signed }),
        }
    }
    pub fn can_cast_to(self, target: Self) -> bool {
        (self.number_type().is_some() && target.number_type().is_some())
            || (self == Self::Bool && target.integer_layout().is_some())
    }
}
