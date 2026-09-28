//! Generated standard declaration surface shared by source analysis and executable contracts.
use super::declarations::{
    ApiAssociatedType, ApiBound, ApiFunction, ApiItem, ApiMethod, ApiParameter, ApiTrait, ApiType,
};
use crate::{scalar::BuiltinType, standard::StandardIntrinsic};
use kagari_common::range::RangeKind;
use serde::{Deserialize, Serialize};
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BuiltinTypeFamily {
    Never,
    Unit,
    Boolean,
    SignedInteger,
    UnsignedInteger,
    Float,
    String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BuiltinTypeSpec {
    pub ty: BuiltinType,
    pub name: &'static str,
    pub family: BuiltinTypeFamily,
    pub const_safe: bool,
    pub heap_backed: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum StandardEnum {
    Bound,
    ParseError,
    TryFromIntError,
    Infallible,
    Option,
    Result,
    Ordering,
}

impl StandardEnum {
    pub fn spec(self) -> &'static StandardEnumSpec {
        standard_enums()
            .iter()
            .find(|spec| spec.kind == self)
            .expect("standard enum specification")
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum StandardVariant {
    ParseEmpty,
    ParseInvalidDigit,
    ParseOutOfRange,
    ParseInvalidRadix,
    ParseInvalidSyntax,

    Included,
    Excluded,
    Unbounded,
    OutOfRange,
    Less,
    Equal,
    Greater,
    Some,
    None,
    Ok,
    Err,
}

impl StandardVariant {
    pub fn kind(self) -> StandardEnum {
        match self {
            Self::ParseEmpty
            | Self::ParseInvalidDigit
            | Self::ParseOutOfRange
            | Self::ParseInvalidRadix
            | Self::ParseInvalidSyntax => StandardEnum::ParseError,
            Self::Included | Self::Excluded | Self::Unbounded => StandardEnum::Bound,
            Self::OutOfRange => StandardEnum::TryFromIntError,
            Self::Less | Self::Equal | Self::Greater => StandardEnum::Ordering,
            Self::Some | Self::None => StandardEnum::Option,
            Self::Ok | Self::Err => StandardEnum::Result,
        }
    }
    pub fn index(self) -> usize {
        match self {
            Self::ParseEmpty => 0,
            Self::ParseInvalidDigit => 1,
            Self::ParseOutOfRange => 2,
            Self::ParseInvalidRadix => 3,
            Self::ParseInvalidSyntax => 4,

            Self::Included => 0,
            Self::Excluded => 1,
            Self::Unbounded => 2,
            Self::OutOfRange | Self::Less => 0,
            Self::Equal => 1,
            Self::Greater => 2,
            Self::Some | Self::Ok => 0,
            Self::None | Self::Err => 1,
        }
    }
    pub fn payload(self) -> Option<usize> {
        match self {
            Self::ParseEmpty
            | Self::ParseInvalidDigit
            | Self::ParseOutOfRange
            | Self::ParseInvalidRadix
            | Self::ParseInvalidSyntax
            | Self::Unbounded
            | Self::OutOfRange
            | Self::None
            | Self::Less
            | Self::Equal
            | Self::Greater => None,
            Self::Err => Some(1),
            _ => Some(0),
        }
    }
}

pub fn standard_variant(path: &str) -> Option<StandardVariant> {
    Some(match path {
        "ParseError::Empty" | "std::string::ParseError::Empty" => StandardVariant::ParseEmpty,
        "ParseError::InvalidDigit" | "std::string::ParseError::InvalidDigit" => {
            StandardVariant::ParseInvalidDigit
        }
        "ParseError::OutOfRange" | "std::string::ParseError::OutOfRange" => {
            StandardVariant::ParseOutOfRange
        }
        "ParseError::InvalidRadix" | "std::string::ParseError::InvalidRadix" => {
            StandardVariant::ParseInvalidRadix
        }
        "ParseError::InvalidSyntax" | "std::string::ParseError::InvalidSyntax" => {
            StandardVariant::ParseInvalidSyntax
        }

        "Bound::Included" | "std::ops::Bound::Included" => StandardVariant::Included,
        "Bound::Excluded" | "std::ops::Bound::Excluded" => StandardVariant::Excluded,
        "Bound::Unbounded" | "std::ops::Bound::Unbounded" => StandardVariant::Unbounded,
        "TryFromIntError::OutOfRange" | "std::convert::TryFromIntError::OutOfRange" => {
            StandardVariant::OutOfRange
        }
        "Ordering::Less" | "std::cmp::Ordering::Less" => StandardVariant::Less,
        "Ordering::Equal" | "std::cmp::Ordering::Equal" => StandardVariant::Equal,
        "Ordering::Greater" | "std::cmp::Ordering::Greater" => StandardVariant::Greater,
        "Some" | "Option::Some" | "std::option::Some" => StandardVariant::Some,
        "None" | "Option::None" | "std::option::None" => StandardVariant::None,
        "Ok" | "Result::Ok" | "std::result::Ok" => StandardVariant::Ok,
        "Err" | "Result::Err" | "std::result::Err" => StandardVariant::Err,
        _ => return None,
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StandardTypeConstructor {
    Bound,
    Range,
    RangeInclusive,
    RangeFrom,
    RangeTo,
    RangeToInclusive,
    RangeFull,
    ArrayList,
    LinkedHashMap,
    LinkedHashSet,
    Iter,
    Option,
    Result,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum StandardModule {
    ParseError,
    TryFromIntError,
    Infallible,
    Numeric,
    Convert,
    Ordering,
    Ops,
    Cmp,
    Hash,
    Fmt,
    Debug,
    Math,
    Array,
    Map,
    Set,
    String,
    Option,
    Result,
    Iter,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StandardVariantSpec {
    pub name: &'static str,
    pub payload_arity: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StandardEnumSpec {
    pub kind: StandardEnum,
    pub name: &'static str,
    pub arity: usize,
    pub variants: &'static [StandardVariantSpec],
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StandardTypeConstructorSpec {
    pub kind: StandardTypeConstructor,
    pub name: &'static str,
    pub arity: usize,
    pub heap_backed: bool,
    pub const_safe: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StandardModuleSpec {
    pub kind: StandardModule,
    pub path: &'static str,
}

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, serde::Serialize, serde::Deserialize,
)]
// Operation predicates for intrinsic signatures. HashKey and Comparable delegate
// to canonical standard traits and are not source-level bound names.
pub enum StandardTypeConstraint {
    HashKey,
    OrderedNumber,
    SignedNumber,
    Comparable,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StandardConstraintSpec {
    pub param: &'static str,
    pub constraint: StandardTypeConstraint,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StandardFunctionSpec {
    pub api: &'static ApiFunction,
    pub module: StandardModule,
    pub name: &'static str,
    pub intrinsic: StandardIntrinsic,
    pub type_params: &'static [&'static str],
    pub arity: usize,
    pub constraints: &'static [StandardConstraintSpec],
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StandardMethodReceiver {
    Builtin(BuiltinType),
    Array,
    Map,
    Set,
    String,
    Option,
    Result,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StandardMethodSpec {
    pub receiver: StandardMethodReceiver,
    pub name: &'static str,
    pub intrinsic: StandardIntrinsic,
    pub type_params: &'static [&'static str],
    pub arity: usize,
    pub constraints: &'static [StandardConstraintSpec],
}

const BUILTIN_TYPES: &[BuiltinTypeSpec] = &[
    BuiltinTypeSpec {
        ty: BuiltinType::Never,
        name: "!",
        family: BuiltinTypeFamily::Never,
        const_safe: false,
        heap_backed: false,
    },
    BuiltinTypeSpec {
        ty: BuiltinType::Unit,
        name: "()",
        family: BuiltinTypeFamily::Unit,
        const_safe: true,
        heap_backed: false,
    },
    BuiltinTypeSpec {
        ty: BuiltinType::Bool,
        name: "bool",
        family: BuiltinTypeFamily::Boolean,
        const_safe: true,
        heap_backed: false,
    },
    BuiltinTypeSpec {
        ty: BuiltinType::I8,
        name: "i8",
        family: BuiltinTypeFamily::SignedInteger,
        const_safe: true,
        heap_backed: false,
    },
    BuiltinTypeSpec {
        ty: BuiltinType::I16,
        name: "i16",
        family: BuiltinTypeFamily::SignedInteger,
        const_safe: true,
        heap_backed: false,
    },
    BuiltinTypeSpec {
        ty: BuiltinType::I32,
        name: "i32",
        family: BuiltinTypeFamily::SignedInteger,
        const_safe: true,
        heap_backed: false,
    },
    BuiltinTypeSpec {
        ty: BuiltinType::I64,
        name: "i64",
        family: BuiltinTypeFamily::SignedInteger,
        const_safe: true,
        heap_backed: false,
    },
    BuiltinTypeSpec {
        ty: BuiltinType::ISize,
        name: "isize",
        family: BuiltinTypeFamily::SignedInteger,
        const_safe: true,
        heap_backed: false,
    },
    BuiltinTypeSpec {
        ty: BuiltinType::U8,
        name: "u8",
        family: BuiltinTypeFamily::UnsignedInteger,
        const_safe: true,
        heap_backed: false,
    },
    BuiltinTypeSpec {
        ty: BuiltinType::U16,
        name: "u16",
        family: BuiltinTypeFamily::UnsignedInteger,
        const_safe: true,
        heap_backed: false,
    },
    BuiltinTypeSpec {
        ty: BuiltinType::U32,
        name: "u32",
        family: BuiltinTypeFamily::UnsignedInteger,
        const_safe: true,
        heap_backed: false,
    },
    BuiltinTypeSpec {
        ty: BuiltinType::U64,
        name: "u64",
        family: BuiltinTypeFamily::UnsignedInteger,
        const_safe: true,
        heap_backed: false,
    },
    BuiltinTypeSpec {
        ty: BuiltinType::USize,
        name: "usize",
        family: BuiltinTypeFamily::UnsignedInteger,
        const_safe: true,
        heap_backed: false,
    },
    BuiltinTypeSpec {
        ty: BuiltinType::F32,
        name: "f32",
        family: BuiltinTypeFamily::Float,
        const_safe: true,
        heap_backed: false,
    },
    BuiltinTypeSpec {
        ty: BuiltinType::F64,
        name: "f64",
        family: BuiltinTypeFamily::Float,
        const_safe: true,
        heap_backed: false,
    },
    BuiltinTypeSpec {
        ty: BuiltinType::String,
        name: "String",
        family: BuiltinTypeFamily::String,
        const_safe: false,
        heap_backed: true,
    },
];

const STANDARD_MODULES: &[StandardModuleSpec] = &[
    StandardModuleSpec {
        kind: StandardModule::Convert,
        path: "std::convert",
    },
    StandardModuleSpec {
        kind: StandardModule::ParseError,
        path: "std::string::ParseError",
    },
    StandardModuleSpec {
        kind: StandardModule::TryFromIntError,
        path: "std::convert::TryFromIntError",
    },
    StandardModuleSpec {
        kind: StandardModule::Infallible,
        path: "std::convert::Infallible",
    },
    StandardModuleSpec {
        kind: StandardModule::Ordering,
        path: "std::cmp::Ordering",
    },
    StandardModuleSpec {
        kind: StandardModule::Ops,
        path: "std::ops",
    },
    StandardModuleSpec {
        kind: StandardModule::Cmp,
        path: "std::cmp",
    },
    StandardModuleSpec {
        kind: StandardModule::Hash,
        path: "std::hash",
    },
    StandardModuleSpec {
        kind: StandardModule::Fmt,
        path: "std::fmt",
    },
    StandardModuleSpec {
        kind: StandardModule::Debug,
        path: "std::debug",
    },
    StandardModuleSpec {
        kind: StandardModule::Numeric,
        path: "std::numeric",
    },
    StandardModuleSpec {
        kind: StandardModule::Math,
        path: "std::math",
    },
    StandardModuleSpec {
        kind: StandardModule::Array,
        path: "std::array",
    },
    StandardModuleSpec {
        kind: StandardModule::Map,
        path: "std::map",
    },
    StandardModuleSpec {
        kind: StandardModule::Set,
        path: "std::set",
    },
    StandardModuleSpec {
        kind: StandardModule::String,
        path: "std::string",
    },
    StandardModuleSpec {
        kind: StandardModule::Option,
        path: "std::option",
    },
    StandardModuleSpec {
        kind: StandardModule::Result,
        path: "std::result",
    },
    StandardModuleSpec {
        kind: StandardModule::Iter,
        path: "std::iter",
    },
];

include!(concat!(env!("OUT_DIR"), "/standard_api.rs"));

pub fn builtin_types() -> &'static [BuiltinTypeSpec] {
    BUILTIN_TYPES
}

pub fn builtin_type(name: &str) -> Option<BuiltinType> {
    builtin_types()
        .iter()
        .find(|spec| spec.name == name)
        .map(|spec| spec.ty)
}

pub fn builtin_type_spec(ty: BuiltinType) -> Option<&'static BuiltinTypeSpec> {
    builtin_types().iter().find(|spec| spec.ty == ty)
}

pub fn standard_enums() -> &'static [StandardEnumSpec] {
    STANDARD_ENUMS
}

pub fn standard_enum(name: &str) -> Option<&'static StandardEnumSpec> {
    let name = if name == "std::ops::Bound" {
        "Bound"
    } else {
        name
    };
    standard_enums().iter().find(|spec| spec.name == name)
}

pub fn standard_type_constructors() -> &'static [StandardTypeConstructorSpec] {
    STANDARD_TYPE_CONSTRUCTORS
}

pub fn standard_type_constructor(name: &str) -> Option<&'static StandardTypeConstructorSpec> {
    let name = if let Some(member) = name.strip_prefix("std::ops::") {
        if member != "Bound" && range_kind(member).is_none() {
            return None;
        }
        member
    } else {
        name
    };
    standard_type_constructors()
        .iter()
        .find(|spec| spec.name == name)
}

pub fn standard_modules() -> &'static [StandardModuleSpec] {
    STANDARD_MODULES
}

pub fn standard_module(path: &str) -> Option<&'static StandardModuleSpec> {
    standard_modules().iter().find(|spec| spec.path == path)
}

pub fn standard_functions() -> &'static [StandardFunctionSpec] {
    STANDARD_FUNCTIONS
}

pub fn standard_function_by_intrinsic(
    intrinsic: StandardIntrinsic,
) -> Option<&'static StandardFunctionSpec> {
    standard_functions()
        .iter()
        .find(|spec| spec.intrinsic == intrinsic)
}

pub fn standard_functions_in_module(
    module: StandardModule,
) -> impl Iterator<Item = &'static StandardFunctionSpec> {
    standard_functions()
        .iter()
        .filter(move |spec| spec.module == module)
}

pub fn standard_function(
    module: StandardModule,
    name: &str,
) -> Option<&'static StandardFunctionSpec> {
    standard_functions_in_module(module).find(|spec| spec.name == name)
}

pub fn standard_methods() -> &'static [StandardMethodSpec] {
    STANDARD_METHODS
}

pub fn standard_method_by_intrinsic(
    intrinsic: StandardIntrinsic,
) -> Option<&'static StandardMethodSpec> {
    standard_methods()
        .iter()
        .find(|spec| spec.intrinsic == intrinsic)
}

pub fn standard_methods_for_receiver(
    receiver: StandardMethodReceiver,
) -> impl Iterator<Item = &'static StandardMethodSpec> {
    standard_methods()
        .iter()
        .filter(move |spec| spec.receiver == receiver)
}

pub fn standard_method(
    receiver: StandardMethodReceiver,
    name: &str,
) -> Option<&'static StandardMethodSpec> {
    standard_methods_for_receiver(receiver).find(|spec| spec.name == name)
}

pub fn standard_constraint(name: &str) -> Option<StandardTypeConstraint> {
    match name {
        "OrderedNumber" => Some(StandardTypeConstraint::OrderedNumber),
        "SignedNumber" => Some(StandardTypeConstraint::SignedNumber),
        _ => None,
    }
}

pub fn standard_constraint_name(constraint: StandardTypeConstraint) -> &'static str {
    match constraint {
        StandardTypeConstraint::HashKey => "Eq + Hash",
        StandardTypeConstraint::OrderedNumber => "OrderedNumber",
        StandardTypeConstraint::SignedNumber => "SignedNumber",
        StandardTypeConstraint::Comparable => "PartialEq",
    }
}

pub fn standard_variants_in_module(
    module: StandardModule,
) -> &'static [(&'static str, StandardVariant)] {
    match module {
        StandardModule::ParseError => &[
            ("Empty", StandardVariant::ParseEmpty),
            ("InvalidDigit", StandardVariant::ParseInvalidDigit),
            ("OutOfRange", StandardVariant::ParseOutOfRange),
            ("InvalidRadix", StandardVariant::ParseInvalidRadix),
            ("InvalidSyntax", StandardVariant::ParseInvalidSyntax),
        ],
        StandardModule::TryFromIntError => &[("OutOfRange", StandardVariant::OutOfRange)],
        StandardModule::Ordering => &[
            ("Less", StandardVariant::Less),
            ("Equal", StandardVariant::Equal),
            ("Greater", StandardVariant::Greater),
        ],
        StandardModule::Option => &[
            ("Some", StandardVariant::Some),
            ("None", StandardVariant::None),
        ],
        StandardModule::Result => &[("Ok", StandardVariant::Ok), ("Err", StandardVariant::Err)],
        _ => &[],
    }
}

pub fn standard_variant_in_module(module: StandardModule, name: &str) -> Option<StandardVariant> {
    standard_variants_in_module(module)
        .iter()
        .find(|(member, _)| *member == name)
        .map(|(_, variant)| *variant)
}

pub fn standard_associated_function(path: &str) -> Option<&'static StandardFunctionSpec> {
    STANDARD_FUNCTIONS.iter().find(|spec| {
        spec.name.contains("::") && (spec.name == path || spec.api.qualified_name == path)
    })
}

pub fn range_kind(name: &str) -> Option<RangeKind> {
    Some(match name.strip_prefix("std::ops::").unwrap_or(name) {
        "Range" => RangeKind::Exclusive,
        "RangeInclusive" => RangeKind::Inclusive,
        "RangeFrom" => RangeKind::From,
        "RangeTo" => RangeKind::To,
        "RangeToInclusive" => RangeKind::ToInclusive,
        "RangeFull" => RangeKind::Full,
        _ => return None,
    })
}

pub fn collection_read_method(intrinsic: StandardIntrinsic) -> bool {
    matches!(
        intrinsic,
        StandardIntrinsic::ArrayCapacity
            | StandardIntrinsic::MapCapacity
            | StandardIntrinsic::SetCapacity
            | StandardIntrinsic::ArrayLen
            | StandardIntrinsic::ArrayIsEmpty
            | StandardIntrinsic::ArrayGet
            | StandardIntrinsic::ArrayJoin
            | StandardIntrinsic::MapLen
            | StandardIntrinsic::MapIsEmpty
            | StandardIntrinsic::MapContainsKey
            | StandardIntrinsic::MapGet
            | StandardIntrinsic::MapKeys
            | StandardIntrinsic::MapValues
            | StandardIntrinsic::MapEntries
            | StandardIntrinsic::SetLen
            | StandardIntrinsic::SetIsEmpty
            | StandardIntrinsic::SetContains
            | StandardIntrinsic::SetToArray
    )
}
