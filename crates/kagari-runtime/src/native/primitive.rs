//! Runtime-owned native bodies with closed, nonallocating script-heap effects.
use crate::{
    Runtime,
    error::RuntimeError,
    execution::roots::ExecutionRoots,
    native::{
        binding::{BindingEntry, Codec, LinkedNativeFunction, NativeBinding},
        conversion::arguments::ArgumentValues,
        types::Type,
    },
    value::{carriers::ScopedValue, strings::StringObject, view::ValueView},
};

use kagari_gc::{access::Scope, error::GcError};
use kagari_types::{callable::Signature, scalar::BuiltinType, ty::Ty};

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
            prepare_mapping: None,
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
    /// The relationship between element, value and result types is part of the
    /// body contract, including in generic declarations before specialization.
    pub(crate) fn accepts_signature(self, signature: &Signature) -> bool {
        let params = &signature.params;
        match self {
            Self::StringByteLength => {
                params == &[Ty::Builtin(BuiltinType::String)]
                    && signature.result == Ty::Builtin(BuiltinType::U64)
            }
        }
    }

    /// Shared by scoped native calls and prepared interpreter operations. The
    /// caller owns roots and cancellation polls before/after this bounded operation.
    pub(crate) fn execute<'call>(
        self,
        runtime: &mut Runtime,
        arguments: ArgumentValues<'_>,
    ) -> Result<ValueView<'call>, Box<RuntimeError>> {
        let result = runtime.memory.scope(&runtime.resources, |cx, roots| {
            ExecutionRoots::with(roots, cx, |slots| {
                let argument = arguments.scoped(slots, 0)?;
                let value = match self {
                    Self::StringByteLength => string_byte_length(slots.scope(), argument)?,
                };
                ValueView::protected(value, None)
            })
        });
        result.map_err(|error| {
            Box::new(match error {
                GcError::ForeignHeap
                | GcError::InvalidRoot
                | GcError::Index
                | GcError::Representation => RuntimeError::module_validation(error.to_string()),
                error => runtime.managed_memory_error(error),
            })
        })
    }
}

/// This closed scalar result needs no element-type signature or argument table.
#[inline(always)]
pub(crate) fn string_byte_length<'gc>(
    cx: &Scope<'gc>,
    value: ScopedValue<'gc>,
) -> Result<ScopedValue<'gc>, GcError> {
    let object = cx.downcast::<StringObject>(value.object().ok_or(GcError::Representation)?)?;
    Ok(ScopedValue::from(
        cx.read(object, |text| text.as_str().len()) as u64,
    ))
}
