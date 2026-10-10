//! Runtime-owned native bodies with closed, nonallocating script-heap effects.
use crate::{
    error::RuntimeError,
    gc::GcHeap,
    native::binding::{BindingEntry, Codec, LinkedNativeFunction, NativeBinding, NativeResult},
    value::Value,
};
use kagari_types::{scalar::BuiltinType, ty::Ty};

/// Selecting a primitive installs its fixed implementation, never an arbitrary
/// callback annotated with a purity claim. Registration checks its exact codecs.
#[derive(Debug, Clone, Copy)]
pub enum NativePrimitive {
    /// Read immutable UTF-8 storage and return its byte count as Kagari usize.
    StringByteLength,
}

impl NativeBinding {
    pub fn primitive(primitive: NativePrimitive) -> Self {
        let (arguments, result) = match primitive {
            NativePrimitive::StringByteLength => (
                vec![Codec::Scalar(Ty::Builtin(BuiltinType::String))],
                Codec::Scalar(Ty::Builtin(BuiltinType::USize)),
            ),
        };
        Self {
            arguments: arguments.into_boxed_slice(),
            result,
            entry: BindingEntry::Primitive(primitive),
            converted_result: false,
            requirement_owner: None,
        }
    }
}

impl LinkedNativeFunction {
    /// Selected operations and result adapters retain their ordinary boundary.
    /// Names, source syntax and host callback bodies do not confer this authority.
    pub(crate) fn closed_primitive(&self) -> Option<NativePrimitive> {
        self.prepared_signature.get()?;
        let BindingEntry::Primitive(primitive) = self.binding.entry else {
            return None;
        };
        if !self.selected.is_empty() || self.result_adapter.is_some() {
            return None;
        }
        match primitive {
            NativePrimitive::StringByteLength
                if self.signature.params == [Ty::Builtin(BuiltinType::String)]
                    && self.signature.result == Ty::Builtin(BuiltinType::USize) =>
            {
                Some(primitive)
            }
            _ => None,
        }
    }
}

impl NativePrimitive {
    /// Shared by scoped native calls and prepared interpreter operations. The
    /// caller owns roots and cancellation polls before/after this bounded read.
    pub(crate) fn execute(self, heap: &GcHeap, value: Value) -> NativeResult<Value> {
        match self {
            Self::StringByteLength => {
                let Value::Str(id) = value else {
                    return Err(RuntimeError::module_validation("string argument"));
                };
                let text = heap
                    .string(id)
                    .ok_or_else(|| RuntimeError::module_validation("invalid string argument"))?;
                Ok(Value::U64(text.len() as u64))
            }
        }
    }
}
