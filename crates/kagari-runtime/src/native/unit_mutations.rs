//! Checked storage mutation followed by the original charged unit publication.
use crate::{Runtime, RuntimeError, RuntimeErrorKind, native::NativeAction, value::Value};
use kagari_abi::{
    callable::EngineNativeBinding, native_import::EngineNativeImport,
    standard::bindings::NativeProtocolMethod,
};

pub(super) fn initialize(
    runtime: &Runtime,
    contract: &EngineNativeImport,
    arguments: &[Value],
) -> Result<NativeAction, RuntimeError> {
    match contract.binding {
        EngineNativeBinding::Intrinsic(operation) => {
            if let Err(error) = runtime.invoke_standard_builtin(operation, arguments) {
                return Ok(NativeAction::BuiltinFailure(error));
            }
        }
        EngineNativeBinding::Protocol(NativeProtocolMethod::CollectionSet) => {
            let [Value::Array(target), Value::U64(index), value] = arguments else {
                return Err(RuntimeError::module_validation(
                    "native slot write operands",
                ));
            };
            if !value.is_default_heap_payload() {
                return Ok(NativeAction::TypeMismatch(
                    "write_index expects default-storable value",
                ));
            }
            let index = *index as usize;
            if let Err(error) = runtime.gc().array_set(*target, index, value.clone()) {
                if error.kind() == RuntimeErrorKind::IndexOutOfBounds {
                    return Ok(NativeAction::InvalidIndex(index));
                }
                return Err(error);
            }
        }
        _ => {
            return Err(RuntimeError::module_validation(
                "native unit mutation binding",
            ));
        }
    }
    Ok(NativeAction::Continue)
}
