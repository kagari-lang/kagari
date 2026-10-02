//! A trait declaration is a Kagari contract, independent of any Rust trait.
use crate::{
    error::RuntimeError,
    native::{
        binding::NativeResult,
        builder::ModuleBuilder,
        declarations::MethodDecl,
        types::{AppliedTrait, ParameterRef, TraitRef, Type},
    },
};
use kagari_abi::types::{AbiType, AssociatedTypeAbi, ConstraintAbi, GenericParameterAbi, TraitAbi};
use kagari_common::identity::{DefinitionId, DefinitionKind, associated_type_id};
use std::sync::Arc;

pub struct TraitBuilder<'module> {
    module: &'module mut ModuleBuilder,
    id: DefinitionId,
    declaration: TraitAbi,
    parameter_names: Vec<String>,
}
impl<'module> TraitBuilder<'module> {
    pub(crate) fn new(module: &'module mut ModuleBuilder, name: String) -> Self {
        let id = module.declaration.definition(DefinitionKind::Trait, &name);
        Self {
            module,
            id,
            declaration: TraitAbi {
                name,
                supertraits: vec![],
                generic_params: vec![],
                bounds: vec![],
                methods: vec![],
                associated_types: vec![],
                associated_consts: vec![],
            },
            parameter_names: vec![],
        }
    }
    pub fn type_parameter(&mut self, name: impl Into<String>) -> NativeResult<ParameterRef> {
        let name = name.into();
        if self.parameter_names.contains(&name) {
            return Err(RuntimeError::metadata_conflict("duplicate trait parameter"));
        }
        let parameter = GenericParameterAbi {
            owner: self.id.clone(),
            position: self.parameter_names.len(),
        };
        self.parameter_names.push(name);
        self.declaration.generic_params.push(parameter.clone());
        Ok(ParameterRef {
            ty: Type(parameter.as_type()),
        })
    }
    pub fn receiver(&self) -> Type {
        Type(AbiType::SelfType(self.id.clone()))
    }
    pub fn parent(&mut self, parent: AppliedTrait) {
        self.declaration.supertraits.push(parent.ty);
    }
    pub fn associated_type(
        &mut self,
        name: &str,
        bounds: impl IntoIterator<Item = AppliedTrait>,
    ) -> NativeResult<Type> {
        let id = associated_type_id(&self.id, name);
        if self
            .declaration
            .associated_types
            .iter()
            .any(|member| member.declaration == id)
        {
            return Err(RuntimeError::metadata_conflict(
                "duplicate associated type declaration",
            ));
        }
        self.declaration.associated_types.push(AssociatedTypeAbi {
            declaration: id.clone(),
            generic_params: vec![],
            parameter_bounds: vec![],
            bounds: bounds
                .into_iter()
                .map(|bound| ConstraintAbi::Trait(bound.ty))
                .collect(),
        });
        let interface: Vec<_> = self
            .declaration
            .generic_params
            .iter()
            .map(GenericParameterAbi::as_type)
            .collect();
        let trait_ref = TraitRef {
            id: self.id.clone(),
            contract: Arc::new(self.declaration.clone()),
        };
        Ok(Type(AbiType::Projection {
            receiver: Box::new(self.receiver().0),
            interface: Box::new(trait_ref.apply(interface.into_iter().map(Type)).ty),
            member: id,
            arguments: vec![],
        }))
    }
    pub fn define_method(&mut self, declaration: MethodDecl) -> NativeResult<()> {
        if self
            .declaration
            .methods
            .iter()
            .any(|method| method.name == declaration.signature.name)
        {
            return Err(RuntimeError::metadata_conflict(
                "duplicate trait method declaration",
            ));
        }
        let receiver = self.receiver();
        self.declaration.methods.push(declaration.lower(receiver));
        Ok(())
    }
    pub fn finish(self) -> NativeResult<TraitRef> {
        if self
            .module
            .declaration
            .traits
            .iter()
            .any(|contract| contract.name == self.declaration.name)
        {
            return Err(RuntimeError::metadata_conflict(
                "duplicate trait declaration",
            ));
        }
        let result = TraitRef {
            id: self.id.clone(),
            contract: Arc::new(self.declaration.clone()),
        };
        self.module
            .providers
            .insert(self.id, self.declaration.clone())?;
        self.module.declaration.traits.push(self.declaration);
        Ok(result)
    }
}
