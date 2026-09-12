use crate::{
    hir::{Literal, LiteralKind},
    types::{BuiltinType, TypeId},
};

/// A checked scalar fact shared by literals, const evaluation and code generation.
#[derive(Debug, Clone, PartialEq)]
pub enum ScalarValue {
    Unit,
    Bool(bool),
    I32(i32),
    F32(f32),
    String(String),
}

impl ScalarValue {
    pub fn ty(&self) -> TypeId {
        TypeId::Builtin(match self {
            Self::Unit => BuiltinType::Unit,
            Self::Bool(_) => BuiltinType::Bool,
            Self::I32(_) => BuiltinType::I32,
            Self::F32(_) => BuiltinType::F32,
            Self::String(_) => BuiltinType::String,
        })
    }

    pub(crate) fn parse(literal: &Literal) -> Result<Self, &'static str> {
        match literal.kind {
            LiteralKind::Number => literal
                .text
                .parse()
                .map(Self::I32)
                .map_err(|_| "integer literal is outside the i32 range"),
            LiteralKind::Float => {
                let value: f32 = literal.text.parse().map_err(|_| "invalid f32 literal")?;
                if !value.is_finite() {
                    return Err("float literal is outside the finite f32 range");
                }
                Ok(Self::F32(value))
            }
            LiteralKind::Bool => match literal.text.as_str() {
                "true" => Ok(Self::Bool(true)),
                "false" => Ok(Self::Bool(false)),
                _ => Err("invalid bool literal"),
            },
            LiteralKind::String => literal
                .text
                .strip_prefix('"')
                .and_then(|text| text.strip_suffix('"'))
                .map(|text| Self::String(text.to_owned()))
                .ok_or("unterminated String literal"),
        }
    }
}
