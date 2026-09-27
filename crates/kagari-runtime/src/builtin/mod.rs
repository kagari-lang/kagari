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
        Integer(_, _) => kagari_ir::builtin::surface::standard_function_by_intrinsic(intrinsic)
            .map_or("integer method", |spec| spec.api.qualified_name),
        ArrayListNew | LinkedHashMapNew | LinkedHashSetNew => "collection constructor",
        ArrayListFromFn | ArrayListFrom | LinkedHashMapFrom | LinkedHashSetFrom => {
            "collection factory"
        }
        ArrayLen => "std::array::ArrayList::len",
        ArrayIsEmpty => "std::array::ArrayList::is_empty",
        ArrayGet => "std::array::ArrayList::get",
        ArrayPush => "std::array::ArrayList::push",
        ArrayPop => "std::array::ArrayList::pop",
        ArrayInsert => "std::array::ArrayList::insert",
        ArrayRemove => "std::array::ArrayList::remove",
        ArrayJoin => "std::array::ArrayList::join",
        ArrayCopyWithin => "array.copy_within",
        ArrayCopyWithinBounds => "array.copy_within bounds",
        ArrayFill => "array.fill",
        ArrayCopyFromSlice | ArrayCopyFromStorage => "array.copy_from_slice",
        ArrayClear => "std::array::ArrayList::clear",
        MapLen => "std::map::LinkedHashMap::len",
        MapIsEmpty => "std::map::LinkedHashMap::is_empty",
        MapContainsKey => "std::map::LinkedHashMap::contains_key",
        MapGet => "std::map::LinkedHashMap::get",
        MapInsert => "std::map::LinkedHashMap::insert",
        MapRemove => "std::map::LinkedHashMap::remove",
        MapClear => "std::map::LinkedHashMap::clear",
        MapKeys | MapKeysStorage => "std::map::LinkedHashMap::keys",
        MapValues | MapValuesStorage => "std::map::LinkedHashMap::values",
        MapEntries | MapEntriesStorage => "std::map::LinkedHashMap::entries",
        SetLen => "std::set::LinkedHashSet::len",
        SetIsEmpty => "std::set::LinkedHashSet::is_empty",
        SetContains => "std::set::LinkedHashSet::contains",
        SetInsert => "std::set::LinkedHashSet::insert",
        SetRemove => "std::set::LinkedHashSet::remove",
        SetClear => "std::set::LinkedHashSet::clear",
        SetToArray => "std::set::LinkedHashSet::to_array",
        SetUnion => "std::set::LinkedHashSet::union",
        SetIntersection => "std::set::LinkedHashSet::intersection",
        SetDifference => "std::set::LinkedHashSet::difference",
        StringLenBytes => "std::string::String::len_bytes",
        StringLenChars => "std::string::String::len_chars",
        StringIsEmpty => "std::string::String::is_empty",
        StringConcat => "std::string::String::concat",
        StringContains => "std::string::String::contains",
        StringStartsWith => "std::string::String::starts_with",
        StringEndsWith => "std::string::String::ends_with",
        StringTrim => "std::string::String::trim",
        StringTrimStart => "std::string::String::trim_start",
        StringTrimEnd => "std::string::String::trim_end",
        StringFind => "std::string::String::find",
        StringRfind => "std::string::String::rfind",
        StringStripPrefix => "std::string::String::strip_prefix",
        StringStripSuffix => "std::string::String::strip_suffix",
        StringSplit => "std::string::String::split",
        StringSplitN => "std::string::String::splitn",
        StringSplitOnce => "std::string::String::split_once",
        StringRsplitOnce => "std::string::String::rsplit_once",
        StringSplitWhitespace => "std::string::String::split_whitespace",
        StringLines => "std::string::String::lines",
        StringReplace => "std::string::String::replace",
        StringReplaceN => "std::string::String::replacen",
        StringRepeat => "std::string::String::repeat",
        StringIsAscii => "std::string::String::is_ascii",
        StringEqIgnoreAsciiCase => "std::string::String::eq_ignore_ascii_case",
        StringToAsciiLowercase => "std::string::String::to_ascii_lowercase",
        StringToAsciiUppercase => "std::string::String::to_ascii_uppercase",
        StringToLowercase => "std::string::String::to_lowercase",
        StringToUppercase => "std::string::String::to_uppercase",
        StringBytes => "std::string::String::bytes",
        StringCharIndices => "std::string::String::char_indices",
        StringIsCharBoundary => "std::string::String::is_char_boundary",
        ParseNumber(_) => "std::string::FromStr::from_str",
        ParseRadix(_) => "std::numeric::from_str_radix",
        StringParse => "std::string::String::parse",
        StringSlice => "std::string::String::slice",
        OptionUnwrapOrElse => "std::option::Option::unwrap_or_else",
        OptionOrElse => "std::option::Option::or_else",
        OptionMapOr => "std::option::Option::map_or",
        OptionMapOrElse => "std::option::Option::map_or_else",
        OptionFilter => "std::option::Option::filter",
        OptionIsSomeAnd => "std::option::Option::is_some_and",
        OptionZip => "std::option::Option::zip",
        OptionFlatten => "std::option::Option::flatten",
        OptionTranspose => "std::option::Option::transpose",
        ResultUnwrapOrElse => "std::result::Result::unwrap_or_else",
        ResultOrElse => "std::result::Result::or_else",
        ResultMapOr => "std::result::Result::map_or",
        ResultMapOrElse => "std::result::Result::map_or_else",
        ResultOk => "std::result::Result::ok",
        ResultErr => "std::result::Result::err",
        ResultIsOkAnd => "std::result::Result::is_ok_and",
        ResultIsErrAnd => "std::result::Result::is_err_and",
        ResultFlatten => "std::result::Result::flatten",
        ResultTranspose => "std::result::Result::transpose",
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
