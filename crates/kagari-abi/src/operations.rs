use crate::numeric::NumericOperation;
use crate::representation::ValueType;
use crate::scalar::BuiltinType;
use crate::standard::surface::StandardEnum as StandardEnumKind;
use crate::types::AbiType;
use crate::types::verify;
use kagari_common::range::RangeKind;
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
pub enum StandardEnumOp {
    Make(u32),
    Test(u32),
    Read(u32),
}

impl StandardEnumOp {
    pub fn contract(self, ty: &AbiType) -> Option<(Option<ValueType>, ValueType)> {
        if !ty.within_wire_limits() || !verify::concrete_type_valid(ty, &Default::default()) {
            return None;
        }
        let AbiType::StandardEnum { kind, args } = ty else {
            return None;
        };
        if args.len() != kind.spec().arity {
            return None;
        }
        let variant = match self {
            Self::Make(v) | Self::Test(v) | Self::Read(v) => v,
        };
        let spec = kind.spec().variants.get(variant as usize)?;
        let payload = if spec.payload_arity == 0 {
            None
        } else {
            Some(args[if args.len() == 2 { variant as usize } else { 0 }].representation())
        };
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
    FromClosure,
    Next,
    Close,
}

impl IterOp {
    pub fn contract(self, ty: &AbiType) -> Option<(Option<ValueType>, ValueType)> {
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
            Self::FromClosure => {
                Self::closure_item(ty)?;
                ValueType::HeapObject
            }
            Self::New => match ty {
                AbiType::Range(_, kind) if kind.has_start() => ValueType::HeapObject,
                AbiType::Array(_, _) | AbiType::Map { .. } | AbiType::Set(_, _) => {
                    ValueType::HeapObject
                }
                AbiType::Builtin(BuiltinType::String) => ValueType::Str,
                _ => return None,
            },
            Self::Next | Self::Close => {
                if !matches!(ty, AbiType::Iter(_)) {
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

    pub fn closure_item(ty: &AbiType) -> Option<&AbiType> {
        let AbiType::Tuple(fields) = ty else {
            return None;
        };
        let AbiType::Function { params, result } = fields.first()? else {
            return None;
        };
        if !params.is_empty() {
            return None;
        }
        for dependency in fields.iter().skip(1) {
            if let AbiType::Array(element, _) = dependency
                && !matches!(element.as_ref(), AbiType::StandardEnum { kind: StandardEnumKind::Option, args } if matches!(args.as_slice(), [AbiType::Iter(_)]))
            {
                return None;
            }
        }
        let AbiType::StandardEnum {
            kind: StandardEnumKind::Option,
            args,
        } = result.as_ref()
        else {
            return None;
        };
        (args.len() == 1).then(|| &args[0])
    }
}

pub fn mapped_error_payload(ty: &AbiType) -> Option<ValueType> {
    if !matches!(
        ty,
        AbiType::StandardEnum {
            kind: StandardEnumKind::Result,
            ..
        }
    ) {
        return None;
    }
    StandardEnumOp::Make(1).contract(ty)?.0
}

/// Validate endpoint presence and physical types using the range's semantic type.
pub fn range_operands_valid(
    ty: &AbiType,
    start: Option<ValueType>,
    end: Option<ValueType>,
) -> bool {
    let AbiType::Range(item, kind) = ty else {
        return false;
    };
    ty.within_wire_limits()
        && verify::concrete_type_valid(ty, &Default::default())
        && start == kind.has_start().then(|| item.representation())
        && end == kind.has_end().then(|| item.representation())
}

pub fn range_bound_valid(range: &AbiType, bound: &AbiType) -> bool {
    let (
        AbiType::Range(item, kind),
        AbiType::StandardEnum {
            kind: StandardEnumKind::Bound,
            args,
        },
    ) = (range, bound)
    else {
        return false;
    };
    range.within_wire_limits()
        && bound.within_wire_limits()
        && verify::concrete_type_valid(range, &Default::default())
        && verify::concrete_type_valid(bound, &Default::default())
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
    pub fn source_type(self) -> AbiType {
        let string = AbiType::Builtin(BuiltinType::String);
        AbiType::Tuple(match self {
            Self::Split => vec![string.clone(), string],
            Self::SplitN => vec![string.clone(), AbiType::Builtin(BuiltinType::USize), string],
            Self::Whitespace | Self::Lines | Self::Bytes | Self::CharIndices => vec![string],
        })
    }
    pub fn item_type(self) -> AbiType {
        match self {
            Self::Bytes => AbiType::Builtin(BuiltinType::U8),
            Self::CharIndices => AbiType::Tuple(vec![
                AbiType::Builtin(BuiltinType::USize),
                AbiType::Builtin(BuiltinType::String),
            ]),
            _ => AbiType::Builtin(BuiltinType::String),
        }
    }
    pub fn valid_source(self, ty: &AbiType) -> bool {
        *ty == self.source_type()
    }
}
