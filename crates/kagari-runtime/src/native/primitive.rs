//! Runtime-owned native bodies with closed, nonallocating script-heap effects.
use crate::{
    Runtime,
    error::RuntimeError,
    frame::types::arguments::ScopedSignature,
    gc::GcHeap,
    module::ExecutableModule,
    native::{
        binding::{BindingEntry, Codec, LinkedNativeFunction, NativeBinding, NativeResult},
        types::Type,
    },
    value::Value,
};
use kagari_types::{callable::Signature, scalar::BuiltinType, ty::Ty};

// A fixed kernel's successful result must not carry the cold diagnostic payload.
pub(crate) type PrimitiveResult = Result<Value, Box<RuntimeError>>;

/// Selecting a primitive installs its fixed implementation, never an arbitrary
/// callback annotated with a purity claim. Registration checks its exact codecs.
#[derive(Debug, Clone, Copy)]
pub enum NativePrimitive {
    /// Read immutable UTF-8 storage and return its byte count as Kagari u64.
    StringByteLength,
}

impl NativeBinding {
    pub fn primitive(primitive: NativePrimitive) -> Self {
        let (arguments, result) = match primitive {
            NativePrimitive::StringByteLength => (
                vec![Codec::Exact(Type::scalar(BuiltinType::String))],
                Codec::Exact(Type::scalar(BuiltinType::U64)),
            ),
        };
        Self {
            host_policy: None,
            arguments: arguments.into_boxed_slice(),
            result,
            entry: BindingEntry::Primitive(primitive),
            converted_result: false,
            requirement_owner: None,
            receiver_access: None,
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
        primitive
            .accepts_signature(&self.signature)
            .then_some(primitive)
    }
}

impl NativePrimitive {
    pub(crate) fn accepts_signature(self, signature: &Signature) -> bool {
        signature.params == [Ty::Builtin(BuiltinType::String)]
            && signature.result == Ty::Builtin(BuiltinType::U64)
    }

    /// The caller owns roots and cancellation polls around this bounded operation.
    pub(crate) fn execute(
        self,
        runtime: &Runtime,
        _owner: &ExecutableModule,
        _signature: &ScopedSignature,
        argument: impl Fn(usize) -> NativeResult<Value>,
    ) -> PrimitiveResult {
        string_byte_length(runtime.gc(), argument(0)?)
    }
}

#[inline(always)]
pub(crate) fn string_byte_length(heap: &GcHeap, value: Value) -> PrimitiveResult {
    let Value::Str(id) = value else {
        return Err(RuntimeError::module_validation("string argument").into());
    };
    let text = heap
        .string(id)
        .ok_or_else(|| RuntimeError::module_validation("invalid string argument"))?;
    Ok(Value::U64(text.len() as u64))
}
