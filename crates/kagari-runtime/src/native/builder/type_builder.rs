//! Native type declarations retain nominal identity and their own binder.
use crate::{
    error::RuntimeError,
    native::{
        binding::NativeResult,
        builder::ModuleBuilder,
        storage::NativeStorage,
        types::{ParameterRef, Type, TypeRef},
    },
};
use kagari_abi::types::{AbiType, GenericParameterAbi, TypeAbi, TypeAbiKind};
use kagari_common::identity::{DefinitionId, DefinitionKind};
use std::sync::Arc;

pub struct TypeBuilder<'module> {
    module: &'module mut ModuleBuilder,
    id: DefinitionId,
    name: String,
    parameter_names: Vec<String>,
    parameters: Vec<GenericParameterAbi>,
    storage: Option<NativeStorage>,
}
impl<'module> TypeBuilder<'module> {
    pub(crate) fn new(module: &'module mut ModuleBuilder, name: String) -> Self {
        let id = module
            .declaration
            .definition(DefinitionKind::AssociatedType, &name);
        Self {
            module,
            id,
            name,
            parameter_names: vec![],
            parameters: vec![],
            storage: None,
        }
    }
    pub fn type_parameter(&mut self, name: impl Into<String>) -> NativeResult<ParameterRef> {
        let name = name.into();
        if self.parameter_names.contains(&name) {
            return Err(RuntimeError::metadata_conflict(
                "duplicate native type parameter",
            ));
        }
        let parameter = GenericParameterAbi {
            owner: self.id.clone(),
            position: self.parameters.len(),
        };
        self.parameter_names.push(name);
        self.parameters.push(parameter.clone());
        Ok(ParameterRef {
            ty: Type(parameter.as_type()),
        })
    }
    pub fn sequence_storage(&mut self, element: &ParameterRef) -> NativeResult<()> {
        let AbiType::Parameter { owner, position } = element.ty.abi() else {
            return Err(RuntimeError::metadata_conflict(
                "sequence element must be a declared type parameter",
            ));
        };
        if *owner != self.id || *position >= self.parameters.len() {
            return Err(RuntimeError::metadata_conflict(
                "sequence element belongs to another declaration",
            ));
        }
        self.native_storage(NativeStorage::sequence(*position))
    }
    pub fn native_storage(&mut self, storage: NativeStorage) -> NativeResult<()> {
        if self.storage.is_some() {
            return Err(RuntimeError::metadata_conflict(
                "duplicate native storage contract",
            ));
        }
        self.storage = Some(storage);
        Ok(())
    }
    /// All native storage objects have shared reference semantics.
    pub fn finish(self) -> NativeResult<TypeRef> {
        let storage = self
            .storage
            .ok_or_else(|| RuntimeError::metadata_conflict("native type requires storage"))?;
        if !storage.layout().valid_parameters(self.parameters.len()) {
            return Err(RuntimeError::metadata_conflict(
                "native storage parameter positions",
            ));
        }
        if self
            .module
            .declaration
            .types
            .iter()
            .any(|ty| ty.name == self.name)
        {
            return Err(RuntimeError::metadata_conflict(
                "duplicate native type declaration",
            ));
        }
        let declaration = TypeAbi {
            name: self.name,
            kind: TypeAbiKind::NativeStorage(storage.layout()),
            generic_params: self.parameters,
            bounds: vec![],
            fields: vec![],
            variants: vec![],
        };
        self.module.storage.insert(self.id.clone(), storage);
        self.module.declaration.types.push(declaration.clone());
        self.module
            .providers
            .insert_type(self.id.clone(), declaration.clone())?;
        Ok(TypeRef {
            id: self.id,
            declaration: Arc::new(declaration),
            parameter_names: self.parameter_names,
        })
    }
}
