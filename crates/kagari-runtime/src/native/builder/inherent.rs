//! Inherent methods belong to the module that owns the nominal type.
use crate::{
    error::RuntimeError,
    native::{
        binding::{Codec, NativeBinding, NativeResult},
        declarations::MethodDecl,
        functions::NativeFunction,
        types::{FunctionRef, Type},
    },
};
use kagari_abi::{callable::CallableImplementation, declaration::ModuleDecl, types::FunctionAbi};
use kagari_common::identity::DefinitionPath;
use std::collections::BTreeMap;

pub struct InherentMethodsBuilder {
    pub(crate) owner: DefinitionPath,
    pub(crate) receiver: Type,
    pub(crate) receiver_codec: Option<Codec>,
    pub(crate) methods: BTreeMap<DefinitionPath, FunctionAbi>,
    pub(crate) bindings: BTreeMap<DefinitionPath, NativeBinding>,
}

impl InherentMethodsBuilder {
    pub fn define_method(&mut self, declaration: MethodDecl) -> NativeResult<FunctionRef> {
        let mut signature = declaration.lower(self.receiver.clone());
        let id = ModuleDecl::method_id(&self.owner, &signature.name);
        if self.methods.contains_key(&id) {
            return Err(RuntimeError::metadata_conflict(
                "duplicate inherent method declaration",
            ));
        }
        signature.implementation = CallableImplementation::Native(id.clone());
        self.methods.insert(id.clone(), signature);
        Ok(FunctionRef { id })
    }

    pub fn bind<A, R>(
        &mut self,
        method: FunctionRef,
        entry: impl NativeFunction<A, R>,
    ) -> NativeResult<()> {
        self.bind_with(method, entry.binding())
    }

    pub fn bind_with(&mut self, method: FunctionRef, binding: NativeBinding) -> NativeResult<()> {
        let signature = self.methods.get(&method.id).ok_or_else(|| {
            RuntimeError::metadata_conflict("unknown inherent method declaration")
        })?;
        if signature
            .params
            .first()
            .is_some_and(|parameter| parameter.name == "self")
            && self.receiver_codec.as_ref().is_some_and(|codec| {
                binding
                    .arguments
                    .first()
                    .is_none_or(|method| !codec.receiver_shape_matches(method))
            })
        {
            return Err(RuntimeError::metadata_conflict(
                "binding receiver differs from the configured receiver codec",
            ));
        }
        if self.bindings.contains_key(&method.id) {
            return Err(RuntimeError::metadata_conflict(
                "duplicate inherent method binding",
            ));
        }
        self.bindings.insert(method.id, binding);
        Ok(())
    }
}
