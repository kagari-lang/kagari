use super::declarations::{
    ApiAssociatedType, ApiBound, ApiFunction, ApiItem, ApiMethod, ApiParameter, ApiTrait, ApiType,
};
use crate::types::{BuiltinType, TypeId};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BuiltinTypeFamily {
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
            Self::Less | Self::Equal | Self::Greater => StandardEnum::Ordering,
            Self::Some | Self::None => StandardEnum::Option,
            Self::Ok | Self::Err => StandardEnum::Result,
        }
    }
    pub fn index(self) -> usize {
        match self {
            Self::Less => 0,
            Self::Equal => 1,
            Self::Greater => 2,
            Self::Some | Self::Ok => 0,
            Self::None | Self::Err => 1,
        }
    }
    pub fn payload(self) -> Option<usize> {
        match self {
            Self::None | Self::Less | Self::Equal | Self::Greater => None,
            Self::Err => Some(1),
            _ => Some(0),
        }
    }
}

pub fn standard_variant(path: &str) -> Option<StandardVariant> {
    Some(match path {
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
    Cursor,
    Option,
    Result,
    Map,
    Set,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum StandardModule {
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum StandardIntrinsic {
    KeyLookupBegin,
    KeyCandidates,
    KeyMapGet,
    KeyMapInsert,
    KeyMapRemove,
    KeySetContains,
    KeySetInsert,
    KeySetRemove,
    ValuePartialCmp,
    ValueCmp,
    ValueEq,
    ValueHash,
    ValueDebug,
    ValueDisplay,
    ArrayLen,
    ArrayIsEmpty,
    ArrayGet,
    ArrayPush,
    ArrayPop,
    ArrayInsert,
    ArrayRemove,
    ArrayClear,
    ArrayJoin,
    MapNew,
    MapLen,
    MapIsEmpty,
    MapContainsKey,
    MapGet,
    MapInsert,
    MapRemove,
    MapClear,
    MapKeys,
    MapValues,
    MapEntries,
    SetNew,
    SetLen,
    SetIsEmpty,
    SetContains,
    SetInsert,
    SetRemove,
    SetClear,
    SetToArray,
    SetUnion,
    SetIntersection,
    SetDifference,
    StringLenBytes,
    StringLenChars,
    StringIsEmpty,
    StringConcat,
    StringContains,
    StringStartsWith,
    StringEndsWith,
    StringSlice,
    OptionIsSome,
    OptionIsNone,
    OptionUnwrapOr,
    OptionMap,
    OptionAndThen,
    OptionOkOr,
    OptionOkOrElse,
    ResultIsOk,
    ResultIsErr,
    ResultUnwrapOr,
    ResultMap,
    ResultMapErr,
    ResultAndThen,
    IterLen,
    IterIsEmpty,
    IterGet,
    IterToArray,
    IterForEach,
    MathMin,
    MathMax,
    MathClamp,
    MathAbs,
    MathFloor,
    MathCeil,
    MathRound,
    MathSqrt,
    MathSin,
    MathCos,
    MathTan,
    DebugPrint,
    DebugAssert,
    DebugAssertEq,
    DebugPanic,
}

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, serde::Serialize, serde::Deserialize,
)]
// Operation predicates for intrinsic signatures. HashKey and Comparable delegate
// to canonical standard traits and are not source-level bound names.
pub enum StandardTypeConstraint {
    HashKey,
    Iterable,
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
    Array,
    Map,
    Set,
    String,
    Option,
    Result,
    Iterable,
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IterableProtocol {
    Array { item: TypeId },
    Map { key: TypeId, value: TypeId },
    Set { item: TypeId },
    String { item: BuiltinType },
}

const BUILTIN_TYPES: &[BuiltinTypeSpec] = &[
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
    standard_enums().iter().find(|spec| spec.name == name)
}

pub fn standard_type_constructors() -> &'static [StandardTypeConstructorSpec] {
    STANDARD_TYPE_CONSTRUCTORS
}

pub fn standard_type_constructor(name: &str) -> Option<&'static StandardTypeConstructorSpec> {
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
        "Iterable" => Some(StandardTypeConstraint::Iterable),
        "OrderedNumber" => Some(StandardTypeConstraint::OrderedNumber),
        "SignedNumber" => Some(StandardTypeConstraint::SignedNumber),
        _ => None,
    }
}

pub fn standard_constraint_name(constraint: StandardTypeConstraint) -> &'static str {
    match constraint {
        StandardTypeConstraint::HashKey => "Eq + Hash",
        StandardTypeConstraint::Iterable => "Iterable",
        StandardTypeConstraint::OrderedNumber => "OrderedNumber",
        StandardTypeConstraint::SignedNumber => "SignedNumber",
        StandardTypeConstraint::Comparable => "PartialEq",
    }
}

pub fn standard_enum_type(name: &str, args: Vec<TypeId>) -> Option<TypeId> {
    let spec = standard_enum(name)?;
    (args.len() == spec.arity).then_some(TypeId::StandardEnum {
        kind: spec.kind,
        args,
    })
}

pub fn standard_generic_type(name: &str, args: Vec<TypeId>) -> Option<TypeId> {
    let spec = standard_type_constructor(name)?;
    if args.len() != spec.arity {
        return None;
    }

    match spec.kind {
        StandardTypeConstructor::Option | StandardTypeConstructor::Result => {
            standard_enum_type(name, args)
        }
        StandardTypeConstructor::Map => {
            let [key, value] = args.try_into().ok()?;
            Some(TypeId::Map {
                key: Box::new(key),
                value: Box::new(value),
            })
        }
        StandardTypeConstructor::Cursor => {
            let [item] = args.try_into().ok()?;
            Some(TypeId::Cursor(Box::new(item)))
        }
        StandardTypeConstructor::Set => {
            let [item] = args.try_into().ok()?;
            Some(TypeId::Set(Box::new(item)))
        }
    }
}

pub fn is_numeric(ty: &TypeId) -> bool {
    matches!(
        builtin_family(ty),
        Some(
            BuiltinTypeFamily::SignedInteger
                | BuiltinTypeFamily::UnsignedInteger
                | BuiltinTypeFamily::Float
        )
    )
}

pub fn supports_unary_negation(ty: &TypeId) -> bool {
    matches!(
        builtin_family(ty),
        Some(BuiltinTypeFamily::SignedInteger | BuiltinTypeFamily::Float)
    )
}

pub fn supports_arithmetic(lhs: &TypeId, rhs: &TypeId) -> bool {
    lhs == rhs && is_numeric(lhs)
}

pub fn supports_ordering(lhs: &TypeId, rhs: &TypeId) -> bool {
    supports_arithmetic(lhs, rhs)
}

pub fn supports_boolean_logic(lhs: &TypeId, rhs: &TypeId) -> bool {
    lhs == &TypeId::Builtin(BuiltinType::Bool) && rhs == &TypeId::Builtin(BuiltinType::Bool)
}

pub fn supports_const_type(ty: &TypeId) -> bool {
    match ty {
        TypeId::Builtin(builtin) => builtin_type_spec(*builtin).is_some_and(|spec| spec.const_safe),
        _ => false,
    }
}

pub fn supports_hash_key(ty: &TypeId) -> bool {
    super::traits::intrinsic_holds(
        super::traits::StandardTrait::Eq,
        ty,
        None,
        &Default::default(),
    ) && super::traits::intrinsic_holds(
        super::traits::StandardTrait::Hash,
        ty,
        None,
        &Default::default(),
    )
}

pub fn iterable_protocol(ty: &TypeId) -> Option<IterableProtocol> {
    match ty {
        TypeId::Array(element) => Some(IterableProtocol::Array {
            item: (**element).clone(),
        }),
        TypeId::Map { key, value } => Some(IterableProtocol::Map {
            key: (**key).clone(),
            value: (**value).clone(),
        }),
        TypeId::Set(element) => Some(IterableProtocol::Set {
            item: (**element).clone(),
        }),
        TypeId::Builtin(BuiltinType::String) => Some(IterableProtocol::String {
            item: BuiltinType::String,
        }),
        _ => None,
    }
}

fn builtin_family(ty: &TypeId) -> Option<BuiltinTypeFamily> {
    let TypeId::Builtin(builtin) = ty else {
        return None;
    };
    builtin_type_spec(*builtin).map(|spec| spec.family)
}

pub fn standard_variants_in_module(
    module: StandardModule,
) -> &'static [(&'static str, StandardVariant)] {
    use StandardVariant::*;
    match module {
        StandardModule::Ordering => &[("Less", Less), ("Equal", Equal), ("Greater", Greater)],
        StandardModule::Option => &[("Some", Some), ("None", None)],
        StandardModule::Result => &[("Ok", Ok), ("Err", Err)],
        _ => &[],
    }
}
pub fn standard_variant_in_module(module: StandardModule, name: &str) -> Option<StandardVariant> {
    standard_variants_in_module(module)
        .iter()
        .find(|(member, _)| *member == name)
        .map(|(_, variant)| *variant)
}
