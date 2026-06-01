use crate::types::{BuiltinType, TypeId};

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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StandardEnum {
    Option,
    Result,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StandardTypeConstructor {
    Option,
    Result,
    Map,
    Set,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StandardModule {
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum StandardIntrinsic {
    ArrayLen,
    ArrayIsEmpty,
    ArrayGet,
    ArrayPush,
    ArrayPop,
    ArrayInsert,
    ArrayRemove,
    ArrayClear,
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
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

const OPTION_VARIANTS: &[StandardVariantSpec] = &[
    StandardVariantSpec {
        name: "Some",
        payload_arity: 1,
    },
    StandardVariantSpec {
        name: "None",
        payload_arity: 0,
    },
];

const RESULT_VARIANTS: &[StandardVariantSpec] = &[
    StandardVariantSpec {
        name: "Ok",
        payload_arity: 1,
    },
    StandardVariantSpec {
        name: "Err",
        payload_arity: 1,
    },
];

const STANDARD_ENUMS: &[StandardEnumSpec] = &[
    StandardEnumSpec {
        kind: StandardEnum::Option,
        name: "Option",
        arity: 1,
        variants: OPTION_VARIANTS,
    },
    StandardEnumSpec {
        kind: StandardEnum::Result,
        name: "Result",
        arity: 2,
        variants: RESULT_VARIANTS,
    },
];

const STANDARD_TYPE_CONSTRUCTORS: &[StandardTypeConstructorSpec] = &[
    StandardTypeConstructorSpec {
        kind: StandardTypeConstructor::Option,
        name: "Option",
        arity: 1,
        heap_backed: true,
        const_safe: false,
    },
    StandardTypeConstructorSpec {
        kind: StandardTypeConstructor::Result,
        name: "Result",
        arity: 2,
        heap_backed: true,
        const_safe: false,
    },
    StandardTypeConstructorSpec {
        kind: StandardTypeConstructor::Map,
        name: "Map",
        arity: 2,
        heap_backed: true,
        const_safe: false,
    },
    StandardTypeConstructorSpec {
        kind: StandardTypeConstructor::Set,
        name: "Set",
        arity: 1,
        heap_backed: true,
        const_safe: false,
    },
];

const STANDARD_MODULES: &[StandardModuleSpec] = &[
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

const HASH_KEY_K: &[StandardConstraintSpec] = &[StandardConstraintSpec {
    param: "K",
    constraint: StandardTypeConstraint::HashKey,
}];
const HASH_KEY_T: &[StandardConstraintSpec] = &[StandardConstraintSpec {
    param: "T",
    constraint: StandardTypeConstraint::HashKey,
}];
const ITERABLE_I: &[StandardConstraintSpec] = &[StandardConstraintSpec {
    param: "I",
    constraint: StandardTypeConstraint::Iterable,
}];
const ORDERED_T: &[StandardConstraintSpec] = &[StandardConstraintSpec {
    param: "T",
    constraint: StandardTypeConstraint::OrderedNumber,
}];
const SIGNED_T: &[StandardConstraintSpec] = &[StandardConstraintSpec {
    param: "T",
    constraint: StandardTypeConstraint::SignedNumber,
}];
const COMPARABLE_T: &[StandardConstraintSpec] = &[StandardConstraintSpec {
    param: "T",
    constraint: StandardTypeConstraint::Comparable,
}];

macro_rules! std_fn {
    ($module:ident, $name:literal, $intrinsic:ident, [$($type_param:literal),*], $arity:literal) => {
        StandardFunctionSpec {
            module: StandardModule::$module,
            name: $name,
            intrinsic: StandardIntrinsic::$intrinsic,
            type_params: &[$($type_param),*],
            arity: $arity,
            constraints: &[],
        }
    };
    ($module:ident, $name:literal, $intrinsic:ident, [$($type_param:literal),*], $arity:literal, $constraints:ident) => {
        StandardFunctionSpec {
            module: StandardModule::$module,
            name: $name,
            intrinsic: StandardIntrinsic::$intrinsic,
            type_params: &[$($type_param),*],
            arity: $arity,
            constraints: $constraints,
        }
    };
}

const STANDARD_FUNCTIONS: &[StandardFunctionSpec] = &[
    std_fn!(Array, "len", ArrayLen, ["T"], 1),
    std_fn!(Array, "is_empty", ArrayIsEmpty, ["T"], 1),
    std_fn!(Array, "get", ArrayGet, ["T"], 2),
    std_fn!(Array, "push", ArrayPush, ["T"], 2),
    std_fn!(Array, "pop", ArrayPop, ["T"], 1),
    std_fn!(Array, "insert", ArrayInsert, ["T"], 3),
    std_fn!(Array, "remove", ArrayRemove, ["T"], 2),
    std_fn!(Array, "clear", ArrayClear, ["T"], 1),
    std_fn!(Map, "new", MapNew, ["K", "V"], 0, HASH_KEY_K),
    std_fn!(Map, "len", MapLen, ["K", "V"], 1, HASH_KEY_K),
    std_fn!(Map, "is_empty", MapIsEmpty, ["K", "V"], 1, HASH_KEY_K),
    std_fn!(
        Map,
        "contains_key",
        MapContainsKey,
        ["K", "V"],
        2,
        HASH_KEY_K
    ),
    std_fn!(Map, "get", MapGet, ["K", "V"], 2, HASH_KEY_K),
    std_fn!(Map, "insert", MapInsert, ["K", "V"], 3, HASH_KEY_K),
    std_fn!(Map, "remove", MapRemove, ["K", "V"], 2, HASH_KEY_K),
    std_fn!(Map, "clear", MapClear, ["K", "V"], 1, HASH_KEY_K),
    std_fn!(Map, "keys", MapKeys, ["K", "V"], 1, HASH_KEY_K),
    std_fn!(Map, "values", MapValues, ["K", "V"], 1, HASH_KEY_K),
    std_fn!(Map, "entries", MapEntries, ["K", "V"], 1, HASH_KEY_K),
    std_fn!(Set, "new", SetNew, ["T"], 0, HASH_KEY_T),
    std_fn!(Set, "len", SetLen, ["T"], 1, HASH_KEY_T),
    std_fn!(Set, "is_empty", SetIsEmpty, ["T"], 1, HASH_KEY_T),
    std_fn!(Set, "contains", SetContains, ["T"], 2, HASH_KEY_T),
    std_fn!(Set, "insert", SetInsert, ["T"], 2, HASH_KEY_T),
    std_fn!(Set, "remove", SetRemove, ["T"], 2, HASH_KEY_T),
    std_fn!(Set, "clear", SetClear, ["T"], 1, HASH_KEY_T),
    std_fn!(Set, "to_array", SetToArray, ["T"], 1, HASH_KEY_T),
    std_fn!(Set, "union", SetUnion, ["T"], 2, HASH_KEY_T),
    std_fn!(Set, "intersection", SetIntersection, ["T"], 2, HASH_KEY_T),
    std_fn!(Set, "difference", SetDifference, ["T"], 2, HASH_KEY_T),
    std_fn!(String, "len_bytes", StringLenBytes, [], 1),
    std_fn!(String, "len_chars", StringLenChars, [], 1),
    std_fn!(String, "is_empty", StringIsEmpty, [], 1),
    std_fn!(String, "concat", StringConcat, [], 2),
    std_fn!(String, "contains", StringContains, [], 2),
    std_fn!(String, "starts_with", StringStartsWith, [], 2),
    std_fn!(String, "ends_with", StringEndsWith, [], 2),
    std_fn!(String, "slice", StringSlice, [], 3),
    std_fn!(Option, "is_some", OptionIsSome, ["T"], 1),
    std_fn!(Option, "is_none", OptionIsNone, ["T"], 1),
    std_fn!(Option, "unwrap_or", OptionUnwrapOr, ["T"], 2),
    std_fn!(Option, "map", OptionMap, ["T", "U"], 2),
    std_fn!(Option, "and_then", OptionAndThen, ["T", "U"], 2),
    std_fn!(Result, "is_ok", ResultIsOk, ["T", "E"], 1),
    std_fn!(Result, "is_err", ResultIsErr, ["T", "E"], 1),
    std_fn!(Result, "unwrap_or", ResultUnwrapOr, ["T", "E"], 2),
    std_fn!(Result, "map", ResultMap, ["T", "U", "E"], 2),
    std_fn!(Result, "map_err", ResultMapErr, ["T", "E", "F"], 2),
    std_fn!(Result, "and_then", ResultAndThen, ["T", "U", "E"], 2),
    std_fn!(Iter, "len", IterLen, ["I"], 1, ITERABLE_I),
    std_fn!(Iter, "is_empty", IterIsEmpty, ["I"], 1, ITERABLE_I),
    std_fn!(Iter, "get", IterGet, ["I"], 2, ITERABLE_I),
    std_fn!(Iter, "to_array", IterToArray, ["I"], 1, ITERABLE_I),
    std_fn!(Iter, "for_each", IterForEach, ["I"], 2, ITERABLE_I),
    std_fn!(Math, "min", MathMin, ["T"], 2, ORDERED_T),
    std_fn!(Math, "max", MathMax, ["T"], 2, ORDERED_T),
    std_fn!(Math, "clamp", MathClamp, ["T"], 3, ORDERED_T),
    std_fn!(Math, "abs", MathAbs, ["T"], 1, SIGNED_T),
    std_fn!(Math, "floor", MathFloor, [], 1),
    std_fn!(Math, "ceil", MathCeil, [], 1),
    std_fn!(Math, "round", MathRound, [], 1),
    std_fn!(Math, "sqrt", MathSqrt, [], 1),
    std_fn!(Math, "sin", MathSin, [], 1),
    std_fn!(Math, "cos", MathCos, [], 1),
    std_fn!(Math, "tan", MathTan, [], 1),
    std_fn!(Debug, "print", DebugPrint, [], 1),
    std_fn!(Debug, "assert", DebugAssert, [], 2),
    std_fn!(Debug, "assert_eq", DebugAssertEq, ["T"], 3, COMPARABLE_T),
    std_fn!(Debug, "panic", DebugPanic, [], 1),
];

macro_rules! std_method {
    ($receiver:ident, $name:literal, $intrinsic:ident, [$($type_param:literal),*], $arity:literal) => {
        StandardMethodSpec {
            receiver: StandardMethodReceiver::$receiver,
            name: $name,
            intrinsic: StandardIntrinsic::$intrinsic,
            type_params: &[$($type_param),*],
            arity: $arity,
            constraints: &[],
        }
    };
    ($receiver:ident, $name:literal, $intrinsic:ident, [$($type_param:literal),*], $arity:literal, $constraints:ident) => {
        StandardMethodSpec {
            receiver: StandardMethodReceiver::$receiver,
            name: $name,
            intrinsic: StandardIntrinsic::$intrinsic,
            type_params: &[$($type_param),*],
            arity: $arity,
            constraints: $constraints,
        }
    };
}

const STANDARD_METHODS: &[StandardMethodSpec] = &[
    std_method!(Array, "len", ArrayLen, ["T"], 0),
    std_method!(Array, "is_empty", ArrayIsEmpty, ["T"], 0),
    std_method!(Array, "get", ArrayGet, ["T"], 1),
    std_method!(Array, "push", ArrayPush, ["T"], 1),
    std_method!(Array, "pop", ArrayPop, ["T"], 0),
    std_method!(Array, "insert", ArrayInsert, ["T"], 2),
    std_method!(Array, "remove", ArrayRemove, ["T"], 1),
    std_method!(Array, "clear", ArrayClear, ["T"], 0),
    std_method!(Map, "len", MapLen, ["K", "V"], 0, HASH_KEY_K),
    std_method!(Map, "is_empty", MapIsEmpty, ["K", "V"], 0, HASH_KEY_K),
    std_method!(
        Map,
        "contains_key",
        MapContainsKey,
        ["K", "V"],
        1,
        HASH_KEY_K
    ),
    std_method!(Map, "get", MapGet, ["K", "V"], 1, HASH_KEY_K),
    std_method!(Map, "insert", MapInsert, ["K", "V"], 2, HASH_KEY_K),
    std_method!(Map, "remove", MapRemove, ["K", "V"], 1, HASH_KEY_K),
    std_method!(Map, "clear", MapClear, ["K", "V"], 0, HASH_KEY_K),
    std_method!(Map, "keys", MapKeys, ["K", "V"], 0, HASH_KEY_K),
    std_method!(Map, "values", MapValues, ["K", "V"], 0, HASH_KEY_K),
    std_method!(Map, "entries", MapEntries, ["K", "V"], 0, HASH_KEY_K),
    std_method!(Set, "len", SetLen, ["T"], 0, HASH_KEY_T),
    std_method!(Set, "is_empty", SetIsEmpty, ["T"], 0, HASH_KEY_T),
    std_method!(Set, "contains", SetContains, ["T"], 1, HASH_KEY_T),
    std_method!(Set, "insert", SetInsert, ["T"], 1, HASH_KEY_T),
    std_method!(Set, "remove", SetRemove, ["T"], 1, HASH_KEY_T),
    std_method!(Set, "clear", SetClear, ["T"], 0, HASH_KEY_T),
    std_method!(Set, "to_array", SetToArray, ["T"], 0, HASH_KEY_T),
    std_method!(Set, "union", SetUnion, ["T"], 1, HASH_KEY_T),
    std_method!(Set, "intersection", SetIntersection, ["T"], 1, HASH_KEY_T),
    std_method!(Set, "difference", SetDifference, ["T"], 1, HASH_KEY_T),
    std_method!(String, "len_bytes", StringLenBytes, [], 0),
    std_method!(String, "len_chars", StringLenChars, [], 0),
    std_method!(String, "is_empty", StringIsEmpty, [], 0),
    std_method!(String, "concat", StringConcat, [], 1),
    std_method!(String, "contains", StringContains, [], 1),
    std_method!(String, "starts_with", StringStartsWith, [], 1),
    std_method!(String, "ends_with", StringEndsWith, [], 1),
    std_method!(String, "slice", StringSlice, [], 2),
    std_method!(Option, "is_some", OptionIsSome, ["T"], 0),
    std_method!(Option, "is_none", OptionIsNone, ["T"], 0),
    std_method!(Option, "unwrap_or", OptionUnwrapOr, ["T"], 1),
    std_method!(Option, "map", OptionMap, ["T", "U"], 1),
    std_method!(Option, "and_then", OptionAndThen, ["T", "U"], 1),
    std_method!(Result, "is_ok", ResultIsOk, ["T", "E"], 0),
    std_method!(Result, "is_err", ResultIsErr, ["T", "E"], 0),
    std_method!(Result, "unwrap_or", ResultUnwrapOr, ["T", "E"], 1),
    std_method!(Result, "map", ResultMap, ["T", "U", "E"], 1),
    std_method!(Result, "map_err", ResultMapErr, ["T", "E", "F"], 1),
    std_method!(Result, "and_then", ResultAndThen, ["T", "U", "E"], 1),
    std_method!(Iterable, "len", IterLen, ["I"], 0, ITERABLE_I),
    std_method!(Iterable, "is_empty", IterIsEmpty, ["I"], 0, ITERABLE_I),
    std_method!(Iterable, "get", IterGet, ["I"], 1, ITERABLE_I),
    std_method!(Iterable, "to_array", IterToArray, ["I"], 0, ITERABLE_I),
    std_method!(Iterable, "for_each", IterForEach, ["I"], 1, ITERABLE_I),
];

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
        "HashKey" => Some(StandardTypeConstraint::HashKey),
        "Iterable" => Some(StandardTypeConstraint::Iterable),
        "OrderedNumber" => Some(StandardTypeConstraint::OrderedNumber),
        "SignedNumber" => Some(StandardTypeConstraint::SignedNumber),
        "Comparable" => Some(StandardTypeConstraint::Comparable),
        _ => None,
    }
}

pub fn standard_constraint_name(constraint: StandardTypeConstraint) -> &'static str {
    match constraint {
        StandardTypeConstraint::HashKey => "HashKey",
        StandardTypeConstraint::Iterable => "Iterable",
        StandardTypeConstraint::OrderedNumber => "OrderedNumber",
        StandardTypeConstraint::SignedNumber => "SignedNumber",
        StandardTypeConstraint::Comparable => "Comparable",
    }
}

pub fn standard_enum_type(name: &str, args: Vec<TypeId>) -> Option<TypeId> {
    let spec = standard_enum(name)?;
    (args.len() == spec.arity).then(|| TypeId::StandardEnum {
        name: spec.name.to_owned(),
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
    matches!(
        ty,
        TypeId::Builtin(
            BuiltinType::Bool
                | BuiltinType::I8
                | BuiltinType::I16
                | BuiltinType::I32
                | BuiltinType::I64
                | BuiltinType::ISize
                | BuiltinType::U8
                | BuiltinType::U16
                | BuiltinType::U32
                | BuiltinType::U64
                | BuiltinType::USize
                | BuiltinType::String
        )
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
