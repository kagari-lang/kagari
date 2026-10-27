use crate::{numeric::NumericOperation, representation::semantic_representation};
use kagari_abi::representation::ValueType;
use kagari_common::identity::table::DefinitionTable;
use kagari_types::{
    declaration::verify::{concrete_type_valid, types_in_scope},
    language::binding,
    range::RangeKind,
    scalar::BuiltinType,
    ty::Ty,
};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum UnaryOp {
    Neg,
    Not,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum BinaryOp {
    Numeric(NumericOperation),
    Add,
    Sub,
    Mul,
    Div,
    Rem,
    Eq,
    NotEq,
    IdentityEq,
    IdentityNotEq,
    Lt,
    Gt,
    Le,
    Ge,
    AndAnd,
    OrOr,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum IterOp {
    New,
    String(StringIterKind),
    Next,
}

impl IterOp {
    pub fn contract(
        self,
        ty: &Ty,
        definitions: &DefinitionTable,
    ) -> Option<(Option<ValueType>, ValueType)> {
        if !ty.within_wire_limits() || !concrete_type_valid(ty, &Default::default(), definitions) {
            return None;
        }
        let input = match self {
            Self::String(kind) => {
                if !kind.valid_source(ty) {
                    return None;
                }
                semantic_representation(ty)
            }
            Self::New => match ty {
                Ty::Range(_, kind) if kind.has_start() => ValueType::HeapObject,

                Ty::Array(_) => ValueType::HeapObject,
                Ty::Builtin(BuiltinType::String) => ValueType::Str,
                _ => return None,
            },
            Self::Next => {
                if !matches!(ty, Ty::Iter(_)) {
                    return None;
                }
                ValueType::HeapObject
            }
        };
        Some((Some(input), ValueType::HeapObject))
    }
}

/// Validate endpoint presence and physical types using the range's semantic type.
pub fn range_operands_valid(
    ty: &Ty,
    start: Option<ValueType>,
    end: Option<ValueType>,
    definitions: &DefinitionTable,
) -> bool {
    let Ty::Range(item, kind) = ty else {
        return false;
    };
    ty.within_wire_limits()
        && concrete_type_valid(ty, &Default::default(), definitions)
        && start == kind.has_start().then(|| semantic_representation(item))
        && end == kind.has_end().then(|| semantic_representation(item))
}

pub fn range_bound_valid(range: &Ty, bound: &Ty, table: &DefinitionTable) -> bool {
    let (Ty::Range(item, kind), Ty::Enum(nominal)) = (range, bound) else {
        return false;
    };
    range.within_wire_limits()
        && bound.within_wire_limits()
        && types_in_scope([range, bound], &[], &Default::default(), table)
        && binding::matches(&nominal.declaration, &binding::bound_declaration(), table)
        && nominal.arguments.len() == 1
        && nominal.associated_types.is_empty()
        && (*kind == RangeKind::Full || nominal.arguments[0] == **item)
}

/// Typed String traversal, with direct String input for scalar and byte cursors.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum StringIterKind {
    Chars,
    Bytes,
    CharIndices,
    Split,
    SplitN,
    Whitespace,
    Lines,
}

impl StringIterKind {
    pub fn source_type(self) -> Ty {
        let string = Ty::Builtin(BuiltinType::String);
        match self {
            Self::Chars | Self::Bytes | Self::CharIndices => string,
            Self::Split => Ty::Tuple(vec![string.clone(), string]),
            Self::SplitN => Ty::Tuple(vec![string.clone(), Ty::Builtin(BuiltinType::U64), string]),
            Self::Whitespace | Self::Lines => Ty::Tuple(vec![string]),
        }
    }

    pub fn item_type(self) -> Ty {
        match self {
            Self::Chars => Ty::Builtin(BuiltinType::Char),
            Self::Bytes => Ty::Builtin(BuiltinType::U8),
            Self::CharIndices => Ty::Tuple(vec![
                Ty::Builtin(BuiltinType::U64),
                Ty::Builtin(BuiltinType::Char),
            ]),
            Self::Split | Self::SplitN | Self::Whitespace | Self::Lines => {
                Ty::Builtin(BuiltinType::String)
            }
        }
    }

    pub fn valid_source(self, ty: &Ty) -> bool {
        *ty == self.source_type()
    }
}

/// Whether the static script type exposes object identity. Generic arguments
/// do not change a known object's category; interfaces hide that category.
pub fn object_identity_type(ty: &Ty) -> bool {
    matches!(ty, Ty::Struct(_) | Ty::NativeObject(_) | Ty::Array(_))
}

#[cfg(test)]
mod tests {
    use super::*;
    use kagari_common::identity::map::DefinitionContext;

    #[test]
    fn string_traversal_contracts_keep_scalar_and_byte_domains() {
        let definitions = DefinitionContext::new().unwrap().snapshot();
        let string = Ty::Builtin(BuiltinType::String);
        for (kind, item, representation) in [
            (
                StringIterKind::Chars,
                Ty::Builtin(BuiltinType::Char),
                ValueType::Char,
            ),
            (
                StringIterKind::CharIndices,
                Ty::Tuple(vec![
                    Ty::Builtin(BuiltinType::U64),
                    Ty::Builtin(BuiltinType::Char),
                ]),
                ValueType::HeapObject,
            ),
            (
                StringIterKind::Bytes,
                Ty::Builtin(BuiltinType::U8),
                ValueType::I64,
            ),
        ] {
            assert_eq!(kind.item_type(), item);
            assert_eq!(semantic_representation(&item), representation);
            assert_eq!(kind.source_type(), string);
            assert_eq!(
                IterOp::String(kind).contract(&kind.source_type(), &definitions),
                Some((Some(ValueType::Str), ValueType::HeapObject))
            );
            for invalid in [Ty::Tuple(vec![string.clone()]), Ty::Tuple(vec![]), item] {
                assert!(
                    IterOp::String(kind)
                        .contract(&invalid, &definitions)
                        .is_none()
                );
            }
        }
    }
}
