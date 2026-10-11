//! Runtime-owned native bodies with closed, nonallocating script-heap effects.
use crate::{
    Runtime,
    error::{RuntimeError, RuntimeErrorKind},
    frame::types::arguments::ScopedSignature,
    gc::GcHeap,
    module::LoadedModule,
    native::{
        binding::{BindingEntry, Codec, LinkedNativeFunction, NativeBinding, NativeResult},
        context::operations::check_sequence_argument,
        scalar::NativeScalar,
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
    /// Read immutable UTF-8 storage and return its byte count as Kagari usize.
    StringByteLength,
    /// Read an element from a checked Vec; an absent index traps.
    VecIndex,
    /// Replace an element through a mutable Vec view and return Unit.
    VecSet,
    /// Replace an element and return the same mutable Vec view.
    VecSetFluent,
}

impl NativeBinding {
    pub fn primitive(primitive: NativePrimitive) -> Self {
        let (arguments, result) = match primitive {
            NativePrimitive::StringByteLength => (
                vec![Codec::Scalar(Ty::Builtin(BuiltinType::String))],
                Codec::Scalar(Ty::Builtin(BuiltinType::USize)),
            ),
            NativePrimitive::VecIndex => (
                vec![
                    Codec::Sequence,
                    Codec::Scalar(Ty::Builtin(BuiltinType::USize)),
                ],
                Codec::Value,
            ),
            NativePrimitive::VecSet | NativePrimitive::VecSetFluent => (
                vec![
                    Codec::MutableSequence,
                    Codec::Scalar(Ty::Builtin(BuiltinType::USize)),
                    Codec::Value,
                ],
                Codec::Value,
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
        primitive
            .accepts_signature(&self.signature)
            .then_some(primitive)
    }
}

impl NativePrimitive {
    /// The relationship between element, value and result types is part of the
    /// body contract, including in generic declarations before specialization.
    pub(crate) fn accepts_signature<D: PartialEq + Clone>(self, signature: &Signature<D>) -> bool {
        let params = &signature.params;
        match self {
            Self::StringByteLength => {
                params == &[Ty::Builtin(BuiltinType::String)]
                    && signature.result == Ty::Builtin(BuiltinType::USize)
            }
            Self::VecIndex => {
                matches!(params.as_slice(), [Ty::NativeObject(nominal), Ty::Builtin(BuiltinType::USize)]
                if nominal.arguments.as_slice() == [signature.result.clone()])
            }
            Self::VecSet | Self::VecSetFluent => {
                let [
                    Ty::NativeObject(nominal),
                    Ty::Builtin(BuiltinType::USize),
                    value,
                ] = params.as_slice()
                else {
                    return false;
                };
                nominal.arguments.as_slice() == [value.clone()]
                    && match self {
                        Self::VecSet => signature.result == Ty::Builtin(BuiltinType::Unit),
                        _ => signature.result == params[0],
                    }
            }
        }
    }

    /// Shared by scoped native calls and prepared interpreter operations. The
    /// caller owns roots and cancellation polls before/after this bounded operation.
    pub(crate) fn execute(
        self,
        runtime: &Runtime,
        owner: &LoadedModule,
        signature: &ScopedSignature,
        argument: impl Fn(usize) -> NativeResult<Value>,
    ) -> PrimitiveResult {
        let heap = runtime.gc();
        match self {
            Self::StringByteLength => string_byte_length(heap, argument(0)?),
            Self::VecIndex => {
                let base = argument(0)?;
                let id =
                    check_sequence_argument(runtime, owner, &signature.params[0], || Ok(base))?;
                let index = usize::decode(argument(1)?)?;
                heap.sequence_element(id, index)?.ok_or_else(|| {
                    Box::new(RuntimeError::new(
                        RuntimeErrorKind::IndexOutOfBounds,
                        "list index is out of bounds",
                    ))
                })
            }
            Self::VecSet | Self::VecSetFluent => {
                let base = argument(0)?;
                let id =
                    check_sequence_argument(runtime, owner, &signature.params[0], || Ok(base))?;
                let index = usize::decode(argument(1)?)?;
                let value = argument(2)?;
                if !signature.params[2].matches(runtime, &value, owner) {
                    return Err(RuntimeError::module_validation(
                        "native argument differs from its declared type",
                    )
                    .into());
                }
                // Match the SDK setter: argument conversion precedes callback
                // exclusion and bounds; no assignment occurs until all pass.
                // Operands already live in the caller's roots. Copying an admitted
                // Value needs neither an owning SDK handle nor a conversion GC
                // safepoint: this body cannot grow the heap or call foreign code.
                runtime.resources().poll_execution()?;
                heap.check_sequence_replacement(id, index)?;
                runtime.resources().poll_execution()?;
                heap.sequence_set(id, index, value)?;
                Ok(match self {
                    Self::VecSet => Value::Unit,
                    _ => base,
                })
            }
        }
    }
}

/// This closed scalar result needs no element-type signature or argument table.
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
