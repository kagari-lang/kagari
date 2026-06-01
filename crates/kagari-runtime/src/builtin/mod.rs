pub mod array;
pub mod iterable;
pub mod standard;
pub mod string;

use kagari_ir::builtin::{BuiltinMethod, surface::StandardIntrinsic};

use crate::{gc::GcHeap, value::Value};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BuiltinError {
    message: String,
}

impl BuiltinError {
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }

    pub fn message(&self) -> &str {
        &self.message
    }
}

pub fn invoke(gc: &GcHeap, method: BuiltinMethod, args: &[Value]) -> Result<Value, BuiltinError> {
    let spec = method.spec();

    match method {
        BuiltinMethod::Array(_) => array::invoke_method(gc, method, args).map_err(|err| {
            BuiltinError::new(format!(
                "{}.{}: {}",
                method.owner_name(),
                spec.name,
                err.message()
            ))
        }),
        BuiltinMethod::Iterable(_) => iterable::invoke_method(gc, method, args).map_err(|err| {
            BuiltinError::new(format!(
                "{}.{}: {}",
                method.owner_name(),
                spec.name,
                err.message()
            ))
        }),
        BuiltinMethod::String(_) => string::invoke_method(gc, method, args).map_err(|err| {
            BuiltinError::new(format!(
                "{}.{}: {}",
                method.owner_name(),
                spec.name,
                err.message()
            ))
        }),
    }
}

pub fn invoke_standard(
    gc: &GcHeap,
    intrinsic: StandardIntrinsic,
    args: &[Value],
) -> Result<Value, BuiltinError> {
    standard::invoke(gc, intrinsic, args).map_err(|err| {
        BuiltinError::new(format!(
            "{}: {}",
            standard_intrinsic_name(intrinsic),
            err.message()
        ))
    })
}

pub fn invoke_standard_with_callbacks(
    gc: &GcHeap,
    intrinsic: StandardIntrinsic,
    args: &[Value],
    callbacks: &mut dyn standard::BuiltinCallbacks,
) -> Result<Value, BuiltinError> {
    standard::invoke_with_callbacks(gc, intrinsic, args, callbacks).map_err(|err| {
        BuiltinError::new(format!(
            "{}: {}",
            standard_intrinsic_name(intrinsic),
            err.message()
        ))
    })
}

fn standard_intrinsic_name(intrinsic: StandardIntrinsic) -> &'static str {
    use StandardIntrinsic::*;

    match intrinsic {
        ArrayLen => "std::array::len",
        ArrayIsEmpty => "std::array::is_empty",
        ArrayGet => "std::array::get",
        ArrayPush => "std::array::push",
        ArrayPop => "std::array::pop",
        ArrayInsert => "std::array::insert",
        ArrayRemove => "std::array::remove",
        ArrayClear => "std::array::clear",
        MapNew => "std::map::new",
        MapLen => "std::map::len",
        MapIsEmpty => "std::map::is_empty",
        MapContainsKey => "std::map::contains_key",
        MapGet => "std::map::get",
        MapInsert => "std::map::insert",
        MapRemove => "std::map::remove",
        MapClear => "std::map::clear",
        MapKeys => "std::map::keys",
        MapValues => "std::map::values",
        MapEntries => "std::map::entries",
        SetNew => "std::set::new",
        SetLen => "std::set::len",
        SetIsEmpty => "std::set::is_empty",
        SetContains => "std::set::contains",
        SetInsert => "std::set::insert",
        SetRemove => "std::set::remove",
        SetClear => "std::set::clear",
        SetToArray => "std::set::to_array",
        SetUnion => "std::set::union",
        SetIntersection => "std::set::intersection",
        SetDifference => "std::set::difference",
        StringLenBytes => "std::string::len_bytes",
        StringLenChars => "std::string::len_chars",
        StringIsEmpty => "std::string::is_empty",
        StringConcat => "std::string::concat",
        StringContains => "std::string::contains",
        StringStartsWith => "std::string::starts_with",
        StringEndsWith => "std::string::ends_with",
        StringSlice => "std::string::slice",
        OptionIsSome => "std::option::is_some",
        OptionIsNone => "std::option::is_none",
        OptionUnwrapOr => "std::option::unwrap_or",
        OptionMap => "std::option::map",
        OptionAndThen => "std::option::and_then",
        ResultIsOk => "std::result::is_ok",
        ResultIsErr => "std::result::is_err",
        ResultUnwrapOr => "std::result::unwrap_or",
        ResultMap => "std::result::map",
        ResultMapErr => "std::result::map_err",
        ResultAndThen => "std::result::and_then",
        IterLen => "std::iter::len",
        IterIsEmpty => "std::iter::is_empty",
        IterGet => "std::iter::get",
        IterToArray => "std::iter::to_array",
        IterForEach => "std::iter::for_each",
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
    }
}
