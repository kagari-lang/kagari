use crate::{integer, numeric::NumberType};

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, serde::Serialize, serde::Deserialize,
)]
pub enum BuiltinType {
    /// Uninhabited type: evaluating an expression of this type cannot return a value.
    Never,
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

/// Integer scalar identity, excluding non-integer builtin types.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum IntegerType {
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
}

impl IntegerType {
    pub fn from_builtin(ty: BuiltinType) -> Option<Self> {
        Some(match ty {
            BuiltinType::I8 => Self::I8,
            BuiltinType::I16 => Self::I16,
            BuiltinType::I32 => Self::I32,
            BuiltinType::I64 => Self::I64,
            BuiltinType::ISize => Self::ISize,
            BuiltinType::U8 => Self::U8,
            BuiltinType::U16 => Self::U16,
            BuiltinType::U32 => Self::U32,
            BuiltinType::U64 => Self::U64,
            BuiltinType::USize => Self::USize,
            _ => return None,
        })
    }

    pub fn builtin_type(self) -> BuiltinType {
        match self {
            Self::I8 => BuiltinType::I8,
            Self::I16 => BuiltinType::I16,
            Self::I32 => BuiltinType::I32,
            Self::I64 => BuiltinType::I64,
            Self::ISize => BuiltinType::ISize,
            Self::U8 => BuiltinType::U8,
            Self::U16 => BuiltinType::U16,
            Self::U32 => BuiltinType::U32,
            Self::U64 => BuiltinType::U64,
            Self::USize => BuiltinType::USize,
        }
    }

    pub fn layout(self) -> (u32, bool) {
        match self {
            Self::I8 => (8, true),
            Self::I16 => (16, true),
            Self::I32 => (32, true),
            Self::I64 | Self::ISize => (64, true),
            Self::U8 => (8, false),
            Self::U16 => (16, false),
            Self::U32 => (32, false),
            Self::U64 | Self::USize => (64, false),
        }
    }

    pub fn bounds(self) -> (i128, i128) {
        let (bits, signed) = self.layout();
        integer::bounds(bits, signed)
    }
}

impl BuiltinType {
    pub fn integer_bounds(self) -> Option<(i128, i128)> {
        self.integer_layout()
            .map(|(bits, signed)| integer::bounds(bits, signed))
    }

    pub fn integer_layout(self) -> Option<(u32, bool)> {
        IntegerType::from_builtin(self).map(IntegerType::layout)
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
