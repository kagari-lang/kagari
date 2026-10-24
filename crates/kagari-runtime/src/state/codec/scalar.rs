use crate::{
    frame::types::arguments::TypeArgument,
    native::conversion::{FromKagari, IntoKagari, context::ConversionContext},
    state::{
        codec::walk::Walk,
        data::{StateScalar, StateValue},
        error::{SnapshotErrorKind, SnapshotResult},
    },
    value::Value,
};
use kagari_types::{scalar::BuiltinType, ty::Ty};

pub(super) fn describe(ty: &Ty) -> Option<StateScalar> {
    let Ty::Builtin(ty) = ty else {
        return None;
    };
    Some(match ty {
        BuiltinType::Unit => StateScalar::Unit,
        BuiltinType::Bool => StateScalar::Bool,
        BuiltinType::Char => StateScalar::Char,
        BuiltinType::I8 => StateScalar::I8,
        BuiltinType::I16 => StateScalar::I16,
        BuiltinType::I32 => StateScalar::I32,
        BuiltinType::I64 => StateScalar::I64,
        BuiltinType::U8 => StateScalar::U8,
        BuiltinType::U16 => StateScalar::U16,
        BuiltinType::U32 => StateScalar::U32,
        BuiltinType::U64 => StateScalar::U64,
        BuiltinType::F32 => StateScalar::F32,
        BuiltinType::F64 => StateScalar::F64,
        BuiltinType::String => StateScalar::String,
        _ => return None,
    })
}

pub(super) fn export(
    cx: &mut ConversionContext<'_>,
    expected: &TypeArgument,
    value: &Value,
    scalar: StateScalar,
    walk: &mut Walk,
) -> SnapshotResult<StateValue> {
    macro_rules! read {
        ($ty:ty, $variant:ident) => {{
            walk.runtime(cx.check_type::<$ty>(expected))?;
            StateValue::$variant(walk.runtime(<$ty>::from_kagari(cx, expected, value))?)
        }};
    }
    Ok(match scalar {
        StateScalar::Unit => {
            walk.runtime(<()>::from_kagari(cx, expected, value))?;
            StateValue::Unit
        }
        StateScalar::Bool => read!(bool, Bool),
        StateScalar::Char => read!(char, Char),
        StateScalar::I8 => read!(i8, I8),
        StateScalar::I16 => read!(i16, I16),
        StateScalar::I32 => read!(i32, I32),
        StateScalar::I64 => read!(i64, I64),
        StateScalar::U8 => read!(u8, U8),
        StateScalar::U16 => read!(u16, U16),
        StateScalar::U32 => read!(u32, U32),
        StateScalar::U64 => read!(u64, U64),
        StateScalar::F32 => StateValue::F32Bits(
            walk.runtime(f32::from_kagari(cx, expected, value))?
                .to_bits(),
        ),
        StateScalar::F64 => StateValue::F64Bits(
            walk.runtime(f64::from_kagari(cx, expected, value))?
                .to_bits(),
        ),
        StateScalar::String => {
            let Value::Str(id) = value else {
                return Err(walk.error(SnapshotErrorKind::TypeMismatch, "expected String"));
            };
            let text = cx
                .runtime()
                .gc()
                .string(*id)
                .ok_or_else(|| walk.error(SnapshotErrorKind::TypeMismatch, "expired String"))?;
            // Inspect the borrowed string before either adapter or DTO allocation.
            walk.runtime(cx.charge_string(text.len()))?;
            StateValue::String(walk.copy_text(cx, &text, false)?)
        }
    })
}

pub(super) fn restore(
    cx: &mut ConversionContext<'_>,
    expected: &TypeArgument,
    value: &StateValue,
    walk: &mut Walk,
) -> SnapshotResult<Value> {
    macro_rules! write {
        ($value:expr) => {{
            let value = $value;
            walk.runtime(value.into_kagari(cx, expected))?
        }};
    }
    Ok(match value {
        StateValue::Unit => write!(()),
        StateValue::Bool(value) => write!(*value),
        StateValue::Char(value) => write!(*value),
        StateValue::I8(value) => write!(*value),
        StateValue::I16(value) => write!(*value),
        StateValue::I32(value) => write!(*value),
        StateValue::I64(value) => write!(*value),
        StateValue::U8(value) => write!(*value),
        StateValue::U16(value) => write!(*value),
        StateValue::U32(value) => write!(*value),
        StateValue::U64(value) => write!(*value),
        StateValue::F32Bits(value) => write!(f32::from_bits(*value)),
        StateValue::F64Bits(value) => write!(f64::from_bits(*value)),
        StateValue::String(value) => write!(walk.copy_text(cx, value, false)?),
        _ => return Err(walk.error(SnapshotErrorKind::TypeMismatch, "expected scalar payload")),
    })
}
