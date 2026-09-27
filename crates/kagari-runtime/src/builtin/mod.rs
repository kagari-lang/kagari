pub mod standard;

use kagari_ir::builtin::surface::StandardIntrinsic;

use crate::{
    error::{RuntimeError, RuntimeErrorKind},
    gc::GcHeap,
    value::Value,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BuiltinError {
    error: RuntimeError,
}

impl BuiltinError {
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            error: RuntimeError::new(RuntimeErrorKind::ScriptTrap, message),
        }
    }

    pub fn message(&self) -> &str {
        self.error.message()
    }
    pub fn into_runtime_error(self) -> RuntimeError {
        self.error
    }

    pub fn kind(&self) -> RuntimeErrorKind {
        self.error.kind()
    }

    fn with_context(self, name: &str) -> Self {
        RuntimeError::new(self.error.kind(), format!("{name}: {}", self.message())).into()
    }
}

impl From<RuntimeError> for BuiltinError {
    fn from(error: RuntimeError) -> Self {
        Self { error }
    }
}

pub fn invoke_standard(
    gc: &GcHeap,
    intrinsic: StandardIntrinsic,
    args: &[Value],
) -> Result<Value, BuiltinError> {
    standard::invoke(gc, intrinsic, args)
        .map_err(|err| err.with_context(standard_intrinsic_name(intrinsic)))
}

pub fn invoke_standard_with_callbacks(
    gc: &GcHeap,
    intrinsic: StandardIntrinsic,
    args: &[Value],
    callbacks: &mut dyn standard::BuiltinCallbacks,
) -> Result<Value, BuiltinError> {
    standard::invoke_with_callbacks(gc, intrinsic, args, callbacks)
        .map_err(|err| err.with_context(standard_intrinsic_name(intrinsic)))
}

fn standard_intrinsic_name(intrinsic: StandardIntrinsic) -> &'static str {
    use StandardIntrinsic::*;

    match intrinsic {
        ArrayNew | MutableArrayNew | MutableMapNew | MutableSetNew => "collection constructor",
        ArrayFrom | MutableArrayFrom | MapFrom | MutableMapFrom | SetFrom | MutableSetFrom => {
            "collection factory"
        }
        ArrayLen => "std::array::Array::len",
        ArrayIsEmpty => "std::array::Array::is_empty",
        ArrayGet => "std::array::Array::get",
        ArrayPush => "std::array::MutableArray::push",
        ArrayPop => "std::array::MutableArray::pop",
        ArrayInsert => "std::array::MutableArray::insert",
        ArrayRemove => "std::array::MutableArray::remove",
        ArrayJoin => "std::array::Array::join",
        ArrayClear => "std::array::MutableArray::clear",
        MapNew => "std::map::Map::new",
        MapLen => "std::map::Map::len",
        MapIsEmpty => "std::map::Map::is_empty",
        MapContainsKey => "std::map::Map::contains_key",
        MapGet => "std::map::Map::get",
        MapInsert => "std::map::MutableMap::insert",
        MapRemove => "std::map::MutableMap::remove",
        MapClear => "std::map::MutableMap::clear",
        MapKeys => "std::map::Map::keys",
        MapValues => "std::map::Map::values",
        MapEntries => "std::map::Map::entries",
        SetNew => "std::set::Set::new",
        SetLen => "std::set::Set::len",
        SetIsEmpty => "std::set::Set::is_empty",
        SetContains => "std::set::Set::contains",
        SetInsert => "std::set::MutableSet::insert",
        SetRemove => "std::set::MutableSet::remove",
        SetClear => "std::set::MutableSet::clear",
        SetToArray => "std::set::Set::to_array",
        SetUnion => "std::set::Set::union",
        SetIntersection => "std::set::Set::intersection",
        SetDifference => "std::set::Set::difference",
        StringLenBytes => "std::string::String::len_bytes",
        StringLenChars => "std::string::String::len_chars",
        StringIsEmpty => "std::string::String::is_empty",
        StringConcat => "std::string::String::concat",
        StringContains => "std::string::String::contains",
        StringStartsWith => "std::string::String::starts_with",
        StringEndsWith => "std::string::String::ends_with",
        StringSlice => "std::string::String::slice",
        OptionIsSome => "std::option::Option::is_some",
        OptionIsNone => "std::option::Option::is_none",
        OptionUnwrapOr => "std::option::Option::unwrap_or",
        OptionMap => "std::option::Option::map",
        OptionAndThen => "std::option::Option::and_then",
        OptionOkOr => "std::option::Option::ok_or",
        OptionOkOrElse => "std::option::Option::ok_or_else",
        ResultIsOk => "std::result::Result::is_ok",
        ResultIsErr => "std::result::Result::is_err",
        ResultUnwrapOr => "std::result::Result::unwrap_or",
        ResultMap => "std::result::Result::map",
        ResultMapErr => "std::result::Result::map_err",
        ResultAndThen => "std::result::Result::and_then",
        MathMin => "std::math::min",
        MathMax => "std::math::max",
        MathClamp => "std::math::clamp",
        MathAbs => "std::math::abs",
        MathFloor => "std::math::floor",
        MathCeil => "std::math::ceil",
        MathRound => "std::math::round",
        MathSqrt => "std::math::sqrt",
        MathSin => "std::math::sin",
        MathCos => "std::math::cos",
        MathTan => "std::math::tan",
        DebugPrint => "std::debug::print",
        DebugAssert => "std::debug::assert",
        DebugAssertEq => "std::debug::assert_eq",
        DebugPanic => "std::debug::panic",
        ValuePartialCmp => "std::cmp::PartialOrd::partial_cmp",
        ValueCmp => "std::cmp::Ord::cmp",
        ValueEq => "std::cmp::PartialEq::eq",
        ValueHash => "std::hash::Hash::hash",
        ValueDebug => "std::fmt::Debug::debug",
        ValueDisplay => "std::fmt::Display::display",
        KeyLookupBegin | KeyCandidates | KeyMapGet | KeyMapInsert | KeyMapRemove
        | KeySetContains | KeySetInsert | KeySetRemove => "internal key operation",
    }
}
