use crate::{
    hir::expr::literal::{Literal, LiteralKind},
    types::TypeId,
};
use kagari_common::{
    literal,
    numeric::{self, Number},
};
use kagari_contract::{scalar::BuiltinType, standard::surface::builtin_type};

/// A checked scalar fact shared by literals, const evaluation and code generation.
#[derive(Debug, Clone, PartialEq)]
pub enum ScalarValue {
    Unit,
    Bool(bool),
    I32(i32),
    Integer { value: i128, ty: BuiltinType },
    F32(f32),
    F64(f64),
    String(String),
}

impl ScalarValue {
    pub fn ty(&self) -> TypeId {
        TypeId::Builtin(match self {
            Self::Unit => BuiltinType::Unit,
            Self::Bool(_) => BuiltinType::Bool,
            Self::I32(_) => BuiltinType::I32,
            Self::Integer { ty, .. } => *ty,
            Self::F32(_) => BuiltinType::F32,
            Self::F64(_) => BuiltinType::F64,
            Self::String(_) => BuiltinType::String,
        })
    }

    pub(crate) fn parse(literal: &Literal) -> Result<Self, &'static str> {
        Self::parse_expected(literal, None, false)
    }

    pub(crate) fn parse_expected(
        literal: &Literal,
        expected: Option<BuiltinType>,
        negative: bool,
    ) -> Result<Self, &'static str> {
        let (digits, suffix) = literal::numeric_literal_parts(&literal.text);
        let suffix_type = suffix.and_then(builtin_type);
        let expected = expected.filter(|ty| {
            if literal.kind == LiteralKind::Float {
                matches!(ty, BuiltinType::F32 | BuiltinType::F64)
            } else {
                matches!(
                    ty,
                    BuiltinType::I8
                        | BuiltinType::I16
                        | BuiltinType::I32
                        | BuiltinType::I64
                        | BuiltinType::ISize
                        | BuiltinType::U8
                        | BuiltinType::U16
                        | BuiltinType::U32
                        | BuiltinType::U64
                        | BuiltinType::USize
                )
            }
        });
        match literal.kind {
            LiteralKind::Number => {
                let ty = suffix_type.or(expected).unwrap_or(BuiltinType::I32);
                if negative
                    && matches!(
                        ty,
                        BuiltinType::U8
                            | BuiltinType::U16
                            | BuiltinType::U32
                            | BuiltinType::U64
                            | BuiltinType::USize
                    )
                {
                    return Err("unsigned integers do not support negation");
                }
                let value = i128::from(literal::parse_integer_literal(&literal.text)?);
                Self::integer(if negative { -value } else { value }, ty)
            }
            LiteralKind::Float => {
                let compact = digits.replace('_', "");
                let ty = suffix_type.or(expected).unwrap_or(BuiltinType::F64);
                if ty == BuiltinType::F32 {
                    let value: f32 = compact.parse().map_err(|_| "invalid f32 literal")?;
                    if !value.is_finite() {
                        return Err("float literal is outside the finite f32 range");
                    }
                    Ok(Self::F32(if negative { -value } else { value }))
                } else if ty == BuiltinType::F64 {
                    let value: f64 = compact.parse().map_err(|_| "invalid f64 literal")?;
                    if !value.is_finite() {
                        return Err("float literal is outside the finite f64 range");
                    }
                    Ok(Self::F64(if negative { -value } else { value }))
                } else {
                    Err("floating-point literal requires f32 or f64")
                }
            }
            LiteralKind::Bool => match literal.text.as_str() {
                "true" => Ok(Self::Bool(true)),
                "false" => Ok(Self::Bool(false)),
                _ => Err("invalid bool literal"),
            },
            LiteralKind::String => literal::decode_string_literal(&literal.text).map(Self::String),
        }
    }

    pub fn integer(value: i128, ty: BuiltinType) -> Result<Self, &'static str> {
        let (min, max) = ty
            .integer_bounds()
            .ok_or("integer literal requires an integer type")?;
        if !(min..=max).contains(&value) {
            return Err("integer literal is outside the target type range");
        }
        if ty == BuiltinType::I32 {
            Ok(Self::I32(value as i32))
        } else {
            Ok(Self::Integer { value, ty })
        }
    }
}

impl ScalarValue {
    pub fn cast_numeric(self, target: BuiltinType) -> Option<Self> {
        let TypeId::Builtin(source) = self.ty() else {
            return None;
        };
        if !source.can_cast_to(target) {
            return None;
        }
        let input = match self {
            Self::Bool(v) => Number::Integer(i128::from(v)),
            Self::I32(v) => Number::Integer(i128::from(v)),
            Self::Integer { value, .. } => Number::Integer(value),
            Self::F32(v) => Number::F32(v),
            Self::F64(v) => Number::F64(v),
            _ => return None,
        };
        Some(match numeric::cast(input, target.number_type()?) {
            Number::Integer(v) => Self::integer(v, target).ok()?,
            Number::F32(v) => Self::F32(v),
            Number::F64(v) => Self::F64(v),
        })
    }
}
