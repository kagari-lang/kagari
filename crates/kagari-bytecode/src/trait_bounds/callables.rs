//! Selected native callbacks must have a matching concrete executable target.
use crate::module::BytecodeModule;
use kagari_abi::{
    callable::CallableImplementation,
    native_import::callables::{NativeCallableApplication, NativeCallableOrigin},
};

pub(super) fn target_valid(call: &NativeCallableApplication, closure: &[&BytecodeModule]) -> bool {
    let Some(owner) = closure
        .iter()
        .find(|owner| owner.identity == call.instance.declaration.module)
    else {
        return false;
    };
    match &call.implementation {
        CallableImplementation::Script => owner
            .functions
            .iter()
            .find(|function| function.identity.as_ref() == Some(&call.instance))
            .is_some_and(|function| {
                (match call.origin {
                    NativeCallableOrigin::Implementation => {
                        function.metadata.semantic.protocol_adapter.is_none()
                    }
                    NativeCallableOrigin::ProtocolAdapter => {
                        function.metadata.semantic.protocol_adapter.as_ref()
                            == Some(&call.requirement)
                    }
                }) && function.metadata.params.len() == call.signature.params.len()
                    && function
                        .metadata
                        .params
                        .iter()
                        .zip(&call.signature.params)
                        .all(|(physical, semantic)| *physical == semantic.representation())
                    && function.metadata.return_type == call.signature.result.representation()
                    && function.metadata.semantic.params.len() == call.signature.params.len()
                    && call.signature.params.iter().enumerate().all(|(index, ty)| {
                        function.metadata.semantic.params.get(&index) == Some(ty)
                    })
                    && function.metadata.semantic.result.as_ref() == Some(&call.signature.result)
            }),
        CallableImplementation::Native(binding) => owner.native_imports.iter().any(|import| {
            import.instance == call.instance
                && &import.binding == binding
                && import.signature == call.signature
                && import.host.is_none()
        }),
        CallableImplementation::Required | CallableImplementation::NativeDefault(_) => false,
    }
}
