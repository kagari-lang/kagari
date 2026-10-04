use crate::{numeric::NumericOperation, representation::semantic_representation};
use kagari_abi::representation::ValueType;
use kagari_common::identity::{reference::DefinitionReference, table::DefinitionTable};
use kagari_types::{
    declaration::verify::{concrete_type_valid, types_in_scope_in},
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
    Close,
}

impl IterOp {
    pub fn contract(self, ty: &Ty) -> Option<(Option<ValueType>, ValueType)> {
        if !ty.within_wire_limits() || !concrete_type_valid(ty, &Default::default()) {
            return None;
        }
        let input = match self {
            Self::String(kind) => {
                if !kind.valid_source(ty) {
                    return None;
                }
                ValueType::HeapObject
            }
            Self::New => match ty {
                Ty::Range(_, kind) if kind.has_start() => ValueType::HeapObject,
                Ty::Array(_, _) | Ty::Map { .. } | Ty::Set(_, _) => ValueType::HeapObject,
                Ty::Builtin(BuiltinType::String) => ValueType::Str,
                _ => return None,
            },
            Self::Next | Self::Close => {
                if !matches!(ty, Ty::Iter(_)) {
                    return None;
                }
                ValueType::HeapObject
            }
        };
        Some((
            Some(input),
            if self == Self::Close {
                ValueType::Unit
            } else {
                ValueType::HeapObject
            },
        ))
    }
}

/// Validate endpoint presence and physical types using the range's semantic type.
pub fn range_operands_valid(ty: &Ty, start: Option<ValueType>, end: Option<ValueType>) -> bool {
    let Ty::Range(item, kind) = ty else {
        return false;
    };
    ty.within_wire_limits()
        && concrete_type_valid(ty, &Default::default())
        && start == kind.has_start().then(|| semantic_representation(item))
        && end == kind.has_end().then(|| semantic_representation(item))
}

pub fn range_bound_valid(range: &Ty, bound: &Ty) -> bool {
    range_bound_valid_in(range, bound, None)
}

pub fn range_bound_valid_in<I: DefinitionReference>(
    range: &Ty<I>,
    bound: &Ty<I>,
    table: Option<&DefinitionTable>,
) -> bool {
    let (Ty::Range(item, kind), Ty::Enum(nominal)) = (range, bound) else {
        return false;
    };
    range.within_wire_limits()
        && bound.within_wire_limits()
        && types_in_scope_in([range, bound], &[], &Default::default(), table)
        && binding::matches(&nominal.declaration, &binding::bound_declaration(), table)
        && nominal.arguments.len() == 1
        && nominal.associated_types.is_empty()
        && (*kind == RangeKind::Full || nominal.arguments[0] == **item)
}

/// Native string traversal has a typed tuple of constructor arguments.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum StringIterKind {
    Bytes,
    CharIndices,
    Split,
    SplitN,
    Whitespace,
    Lines,
}

impl StringIterKind {
    pub fn source_type<I: DefinitionReference>(self) -> Ty<I> {
        let string = Ty::Builtin(BuiltinType::String);
        Ty::Tuple(match self {
            Self::Split => vec![string.clone(), string],
            Self::SplitN => vec![string.clone(), Ty::Builtin(BuiltinType::USize), string],
            Self::Whitespace | Self::Lines | Self::Bytes | Self::CharIndices => vec![string],
        })
    }

    pub fn item_type<I: DefinitionReference>(self) -> Ty<I> {
        match self {
            Self::Bytes => Ty::Builtin(BuiltinType::U8),
            Self::CharIndices => Ty::Tuple(vec![
                Ty::Builtin(BuiltinType::USize),
                Ty::Builtin(BuiltinType::String),
            ]),
            _ => Ty::Builtin(BuiltinType::String),
        }
    }

    pub fn valid_source<I: DefinitionReference>(self, ty: &Ty<I>) -> bool {
        *ty == self.source_type()
    }
}
