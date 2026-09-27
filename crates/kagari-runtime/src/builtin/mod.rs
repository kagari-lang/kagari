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
    match intrinsic {
        StandardIntrinsic::Integer(_, _) => {
            kagari_ir::builtin::surface::standard_function_by_intrinsic(intrinsic)
                .map_or("integer method", |spec| spec.api.qualified_name)
        }
        StandardIntrinsic::ArrayListNew
        | StandardIntrinsic::LinkedHashMapNew
        | StandardIntrinsic::LinkedHashSetNew => "collection constructor",
        StandardIntrinsic::ArrayListFromFn
        | StandardIntrinsic::ArrayListFrom
        | StandardIntrinsic::LinkedHashMapFrom
        | StandardIntrinsic::LinkedHashSetFrom => "collection factory",
        StandardIntrinsic::ArrayLen => "std::array::ArrayList::len",
        StandardIntrinsic::ArrayIsEmpty => "std::array::ArrayList::is_empty",
        StandardIntrinsic::ArrayGet => "std::array::ArrayList::get",
        StandardIntrinsic::ArrayPush => "std::array::ArrayList::push",
        StandardIntrinsic::ArrayPop => "std::array::ArrayList::pop",
        StandardIntrinsic::ArrayInsert => "std::array::ArrayList::insert",
        StandardIntrinsic::ArrayExtendStorage => "array.extend",
        StandardIntrinsic::ArraySwap => "array::swap",
        StandardIntrinsic::ArrayReverse => "array::reverse",
        StandardIntrinsic::ArrayTruncate => "array::truncate",
        StandardIntrinsic::ArrayExtend => "array::extend",
        StandardIntrinsic::ArraySwapRemove => "array::swap_remove",
        StandardIntrinsic::ArrayWithCapacity => "array::with_capacity",
        StandardIntrinsic::ArrayCapacity => "array::capacity",
        StandardIntrinsic::ArrayReserve => "array::reserve",
        StandardIntrinsic::MapWithCapacity => "map::with_capacity",
        StandardIntrinsic::MapCapacity => "map::capacity",
        StandardIntrinsic::MapReserve => "map::reserve",
        StandardIntrinsic::SetWithCapacity => "set::with_capacity",
        StandardIntrinsic::SetCapacity => "set::capacity",
        StandardIntrinsic::SetReserve => "set::reserve",
        StandardIntrinsic::ArrayRemove => "std::array::ArrayList::remove",
        StandardIntrinsic::ArrayJoin => "std::array::ArrayList::join",
        StandardIntrinsic::IterResume => "resume guarded iterator source",
        StandardIntrinsic::ArrayRemoveRange => "std::array::ArrayList::remove_range",
        StandardIntrinsic::ArrayRemoveRangePrepare => "array range removal preparation",
        StandardIntrinsic::ArrayCopyWithin => "array.copy_within",
        StandardIntrinsic::ArrayCopyWithinBounds => "array.copy_within bounds",
        StandardIntrinsic::ArrayFill => "array.fill",
        StandardIntrinsic::ArrayCopyFrom | StandardIntrinsic::ArrayCopyFromStorage => {
            "array.copy_from"
        }
        StandardIntrinsic::ArrayClear => "std::array::ArrayList::clear",
        StandardIntrinsic::MapLen => "std::map::LinkedHashMap::len",
        StandardIntrinsic::MapIsEmpty => "std::map::LinkedHashMap::is_empty",
        StandardIntrinsic::MapContainsKey => "std::map::LinkedHashMap::contains_key",
        StandardIntrinsic::MapGet => "std::map::LinkedHashMap::get",
        StandardIntrinsic::MapInsert => "std::map::LinkedHashMap::insert",
        StandardIntrinsic::MapRemove => "std::map::LinkedHashMap::remove",
        StandardIntrinsic::MapClear => "std::map::LinkedHashMap::clear",
        StandardIntrinsic::MapKeys | StandardIntrinsic::MapKeysStorage => {
            "std::map::LinkedHashMap::keys"
        }
        StandardIntrinsic::MapValues | StandardIntrinsic::MapValuesStorage => {
            "std::map::LinkedHashMap::values"
        }
        StandardIntrinsic::MapEntries | StandardIntrinsic::MapEntriesStorage => {
            "std::map::LinkedHashMap::entries"
        }
        StandardIntrinsic::SetLen => "std::set::LinkedHashSet::len",
        StandardIntrinsic::SetIsEmpty => "std::set::LinkedHashSet::is_empty",
        StandardIntrinsic::SetContains => "std::set::LinkedHashSet::contains",
        StandardIntrinsic::SetInsert => "std::set::LinkedHashSet::insert",
        StandardIntrinsic::SetRemove => "std::set::LinkedHashSet::remove",
        StandardIntrinsic::SetClear => "std::set::LinkedHashSet::clear",
        StandardIntrinsic::SetToArray => "std::set::LinkedHashSet::to_array",
        StandardIntrinsic::StringLenBytes => "std::string::String::len_bytes",
        StandardIntrinsic::StringLenChars => "std::string::String::len_chars",
        StandardIntrinsic::StringIsEmpty => "std::string::String::is_empty",
        StandardIntrinsic::StringConcat => "std::string::String::concat",
        StandardIntrinsic::StringContains => "std::string::String::contains",
        StandardIntrinsic::StringStartsWith => "std::string::String::starts_with",
        StandardIntrinsic::StringEndsWith => "std::string::String::ends_with",
        StandardIntrinsic::StringTrim => "std::string::String::trim",
        StandardIntrinsic::StringTrimStart => "std::string::String::trim_start",
        StandardIntrinsic::StringTrimEnd => "std::string::String::trim_end",
        StandardIntrinsic::StringFind => "std::string::String::find",
        StandardIntrinsic::StringRfind => "std::string::String::rfind",
        StandardIntrinsic::StringStripPrefix => "std::string::String::strip_prefix",
        StandardIntrinsic::StringStripSuffix => "std::string::String::strip_suffix",
        StandardIntrinsic::StringSplit => "std::string::String::split",
        StandardIntrinsic::StringSplitN => "std::string::String::splitn",
        StandardIntrinsic::StringSplitOnce => "std::string::String::split_once",
        StandardIntrinsic::StringRsplitOnce => "std::string::String::rsplit_once",
        StandardIntrinsic::StringSplitWhitespace => "std::string::String::split_whitespace",
        StandardIntrinsic::StringLines => "std::string::String::lines",
        StandardIntrinsic::StringReplace => "std::string::String::replace",
        StandardIntrinsic::StringReplaceN => "std::string::String::replacen",
        StandardIntrinsic::StringRepeat => "std::string::String::repeat",
        StandardIntrinsic::StringIsAscii => "std::string::String::is_ascii",
        StandardIntrinsic::StringEqIgnoreAsciiCase => "std::string::String::eq_ignore_ascii_case",
        StandardIntrinsic::StringToAsciiLowercase => "std::string::String::to_ascii_lowercase",
        StandardIntrinsic::StringToAsciiUppercase => "std::string::String::to_ascii_uppercase",
        StandardIntrinsic::StringToLowercase => "std::string::String::to_lowercase",
        StandardIntrinsic::StringToUppercase => "std::string::String::to_uppercase",
        StandardIntrinsic::StringBytes => "std::string::String::bytes",
        StandardIntrinsic::StringCharIndices => "std::string::String::char_indices",
        StandardIntrinsic::StringIsCharBoundary => "std::string::String::is_char_boundary",
        StandardIntrinsic::ParseNumber(_) => "std::string::FromStr::from_str",
        StandardIntrinsic::ParseRadix(_) => "std::numeric::from_str_radix",
        StandardIntrinsic::StringParse => "std::string::String::parse",
        StandardIntrinsic::StringSlice => "std::string::String::slice",
        StandardIntrinsic::OptionUnwrapOrElse => "std::option::Option::unwrap_or_else",
        StandardIntrinsic::OptionOrElse => "std::option::Option::or_else",
        StandardIntrinsic::OptionMapOr => "std::option::Option::map_or",
        StandardIntrinsic::OptionMapOrElse => "std::option::Option::map_or_else",
        StandardIntrinsic::OptionFilter => "std::option::Option::filter",
        StandardIntrinsic::OptionIsSomeAnd => "std::option::Option::is_some_and",
        StandardIntrinsic::OptionZip => "std::option::Option::zip",
        StandardIntrinsic::OptionFlatten => "std::option::Option::flatten",
        StandardIntrinsic::OptionTranspose => "std::option::Option::transpose",
        StandardIntrinsic::ResultUnwrapOrElse => "std::result::Result::unwrap_or_else",
        StandardIntrinsic::ResultOrElse => "std::result::Result::or_else",
        StandardIntrinsic::ResultMapOr => "std::result::Result::map_or",
        StandardIntrinsic::ResultMapOrElse => "std::result::Result::map_or_else",
        StandardIntrinsic::ResultOk => "std::result::Result::ok",
        StandardIntrinsic::ResultErr => "std::result::Result::err",
        StandardIntrinsic::ResultIsOkAnd => "std::result::Result::is_ok_and",
        StandardIntrinsic::ResultIsErrAnd => "std::result::Result::is_err_and",
        StandardIntrinsic::ResultFlatten => "std::result::Result::flatten",
        StandardIntrinsic::ResultTranspose => "std::result::Result::transpose",
        StandardIntrinsic::OptionIsSome => "std::option::Option::is_some",
        StandardIntrinsic::OptionIsNone => "std::option::Option::is_none",
        StandardIntrinsic::OptionUnwrapOr => "std::option::Option::unwrap_or",
        StandardIntrinsic::OptionMap => "std::option::Option::map",
        StandardIntrinsic::OptionAndThen => "std::option::Option::and_then",
        StandardIntrinsic::OptionOkOr => "std::option::Option::ok_or",
        StandardIntrinsic::OptionOkOrElse => "std::option::Option::ok_or_else",
        StandardIntrinsic::ResultIsOk => "std::result::Result::is_ok",
        StandardIntrinsic::ResultIsErr => "std::result::Result::is_err",
        StandardIntrinsic::ResultUnwrapOr => "std::result::Result::unwrap_or",
        StandardIntrinsic::ResultMap => "std::result::Result::map",
        StandardIntrinsic::ResultMapErr => "std::result::Result::map_err",
        StandardIntrinsic::ResultAndThen => "std::result::Result::and_then",
        StandardIntrinsic::MathMin => "std::math::min",
        StandardIntrinsic::MathMax => "std::math::max",
        StandardIntrinsic::MathClamp => "std::math::clamp",
        StandardIntrinsic::MathAbs => "std::math::abs",
        StandardIntrinsic::MathFloor => "std::math::floor",
        StandardIntrinsic::MathCeil => "std::math::ceil",
        StandardIntrinsic::MathRound => "std::math::round",
        StandardIntrinsic::MathSqrt => "std::math::sqrt",
        StandardIntrinsic::MathSin => "std::math::sin",
        StandardIntrinsic::MathCos => "std::math::cos",
        StandardIntrinsic::MathTan => "std::math::tan",
        StandardIntrinsic::DebugPrint => "std::debug::print",
        StandardIntrinsic::DebugAssert => "std::debug::assert",
        StandardIntrinsic::DebugAssertEq => "std::debug::assert_eq",
        StandardIntrinsic::DebugPanic => "std::debug::panic",
        StandardIntrinsic::ValuePartialCmp => "std::cmp::PartialOrd::partial_cmp",
        StandardIntrinsic::ValueCmp => "std::cmp::Ord::cmp",
        StandardIntrinsic::ValueEq => "std::cmp::PartialEq::eq",
        StandardIntrinsic::ValueHash => "std::hash::Hash::hash",
        StandardIntrinsic::ValueDebug => "std::fmt::Debug::debug",
        StandardIntrinsic::ValueDisplay => "std::fmt::Display::display",
        StandardIntrinsic::MapGetOrInsertWith => "std::map::LinkedHashMap::get_or_insert_with",
        StandardIntrinsic::ArrayRetain => "prepared collection ArrayRetain",
        StandardIntrinsic::MapRetain => "prepared collection MapRetain",
        StandardIntrinsic::SetRetain => "prepared collection SetRetain",
        StandardIntrinsic::ArraySort => "prepared collection ArraySort",
        StandardIntrinsic::ArraySortBy => "prepared collection ArraySortBy",
        StandardIntrinsic::ArraySortByKey => "prepared collection ArraySortByKey",
        StandardIntrinsic::ArrayDedup => "prepared collection ArrayDedup",
        StandardIntrinsic::ArrayReplaceStorage => "prepared collection ArrayReplaceStorage",
        StandardIntrinsic::CollectionRetainStorage => "prepared collection CollectionRetainStorage",
        StandardIntrinsic::MapUpdate => "std::map::LinkedHashMap::update",
        StandardIntrinsic::CollectionMutationBegin | StandardIntrinsic::CollectionMutationEnd => {
            "collection mutation guard"
        }
        StandardIntrinsic::KeyLookupBegin
        | StandardIntrinsic::KeyCandidates
        | StandardIntrinsic::KeyMapGet
        | StandardIntrinsic::KeyMapInsert
        | StandardIntrinsic::KeyMapRemove
        | StandardIntrinsic::KeySetContains
        | StandardIntrinsic::KeySetInsert
        | StandardIntrinsic::KeySetRemove => "internal key operation",
    }
}
