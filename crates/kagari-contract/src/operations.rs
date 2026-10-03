use kagari_common::{
    identity::{reference::DefinitionReference, table::DefinitionTable},
    range::RangeKind,
};
use serde::{Deserialize, Serialize};
use {
    crate::{
        numeric::NumericOperation,
        scalar::BuiltinType,
        standard::surface::StandardEnum as StandardEnumKind,
        types::{GenericParam, Ty, verify},
    },
    kagari_abi::representation::ValueType,
};

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
pub enum StandardEnumOp {
    Make(u32),
    Test(u32),
    Read(u32),
}

impl StandardEnumOp {
    pub fn contract(self, ty: &Ty) -> Option<(Option<ValueType>, ValueType)> {
        self.contract_in(ty, &[])
    }

    pub fn contract_in(
        self,
        ty: &Ty,
        parameters: &[GenericParam],
    ) -> Option<(Option<ValueType>, ValueType)> {
        if !verify::types_in_scope([ty], parameters, &Default::default()) {
            return None;
        }
        let Ty::StandardEnum { kind, args } = ty else {
            return None;
        };
        if args.len() != kind.arity() {
            return None;
        }
        let variant = match self {
            Self::Make(v) | Self::Test(v) | Self::Read(v) => v,
        };
        let variant = kind.variants().get(variant as usize)?;
        let payload = variant.payload().map(|slot| args[slot].representation());
        match self {
            Self::Make(_) => Some((payload, ValueType::HeapObject)),
            Self::Test(_) => Some((Some(ValueType::HeapObject), ValueType::Bool)),
            Self::Read(_) => Some((Some(ValueType::HeapObject), payload?)),
        }
    }
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
        if !ty.within_wire_limits() || !verify::concrete_type_valid(ty, &Default::default()) {
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

pub fn mapped_error_payload(ty: &Ty) -> Option<ValueType> {
    if !matches!(
        ty,
        Ty::StandardEnum {
            kind: StandardEnumKind::Result,
            ..
        }
    ) {
        return None;
    }
    StandardEnumOp::Make(1).contract(ty)?.0
}

/// Validate endpoint presence and physical types using the range's semantic type.
pub fn range_operands_valid(ty: &Ty, start: Option<ValueType>, end: Option<ValueType>) -> bool {
    let Ty::Range(item, kind) = ty else {
        return false;
    };
    ty.within_wire_limits()
        && verify::concrete_type_valid(ty, &Default::default())
        && start == kind.has_start().then(|| item.representation())
        && end == kind.has_end().then(|| item.representation())
}

pub fn range_bound_valid(range: &Ty, bound: &Ty) -> bool {
    range_bound_valid_in(range, bound, None)
}

pub fn range_bound_valid_in<I: DefinitionReference>(
    range: &Ty<I>,
    bound: &Ty<I>,
    table: Option<&DefinitionTable>,
) -> bool {
    let (
        Ty::Range(item, kind),
        Ty::StandardEnum {
            kind: StandardEnumKind::Bound,
            args,
        },
    ) = (range, bound)
    else {
        return false;
    };
    range.within_wire_limits()
        && bound.within_wire_limits()
        && verify::types_in_scope_in([range, bound], &[], &Default::default(), table)
        && args.len() == 1
        && (*kind == RangeKind::Full || args[0] == **item)
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
