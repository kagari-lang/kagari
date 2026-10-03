//! A trait declaration is a Kagari contract, independent of any Rust trait.
use crate::{
    error::RuntimeError,
    native::{
        binding::{NativeBinding, NativeResult},
        builder::ModuleBuilder,
        declarations::{CallableRequirement, FunctionBuilder, MethodDecl, normalize_bounds},
        functions::NativeFunction,
        types::{AppliedTrait, FunctionRef, ParameterRef, TraitRef, Type},
    },
};
use kagari_abi::{
    declaration::ModuleDecl,
    native_import::callables::NativeCallableRequirement,
    types::{
        AbiType, AssociatedTypeAbi, ConstraintAbi, GenericParameterAbi, NominalAbiType, TraitAbi,
    },
};
use kagari_common::identity::{DefinitionId, DefinitionKind, associated_type_id};
use std::{collections::BTreeMap, sync::Arc};

mod defaults;

pub struct TraitBuilder<'module> {
    module: &'module mut ModuleBuilder,
    id: DefinitionId,
    declaration: TraitAbi,
    parameter_names: Vec<String>,
    method_parameters: BTreeMap<DefinitionId, Vec<String>>,
    requirements: BTreeMap<DefinitionId, Vec<NativeCallableRequirement>>,
    defaults: BTreeMap<DefinitionId, NativeBinding>,
    concrete_results: BTreeMap<DefinitionId, AbiType>,
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
            method_parameters: BTreeMap::new(),
            requirements: BTreeMap::new(),
            defaults: BTreeMap::new(),
            concrete_results: BTreeMap::new(),
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

    /// Refer to an operation on this default body's actual receiver. The final
    /// declaration, including any later method edits, is checked at installation.
    pub fn operation(&self, method: &FunctionRef) -> NativeResult<CallableRequirement> {
        if !self
            .declaration
            .methods
            .iter()
            .any(|signature| ModuleDecl::method_id(&self.id, &signature.name) == method.id)
        {
            return Err(RuntimeError::metadata_conflict(
                "unknown trait method declaration",
            ));
        }
        Ok(CallableRequirement {
            requirement: NativeCallableRequirement {
                receiver: self.receiver().0,
                interface: NominalAbiType {
                    declaration: self.id.clone(),
                    arguments: self
                        .declaration
                        .generic_params
                        .iter()
                        .map(GenericParameterAbi::as_type)
                        .collect(),
                    associated_types: BTreeMap::new(),
                },
                member: method.id.clone(),
                arguments: vec![],
            },
        })
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

    pub fn define_method(&mut self, declaration: MethodDecl) -> NativeResult<FunctionRef> {
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
        let id = ModuleDecl::method_id(&self.id, &declaration.signature.name);
        self.declaration.methods.push(declaration.lower(receiver));
        Ok(FunctionRef { id })
    }

    /// Configure method-local binders, bounds and the operations used by a default.
    pub fn method<T>(
        &mut self,
        method: &FunctionRef,
        configure: impl FnOnce(&mut FunctionBuilder<'_>) -> NativeResult<T>,
    ) -> NativeResult<T> {
        let signature = self
            .declaration
            .methods
            .iter_mut()
            .find(|signature| ModuleDecl::method_id(&self.id, &signature.name) == method.id)
            .ok_or_else(|| RuntimeError::metadata_conflict("unknown trait method declaration"))?;
        let result = configure(&mut FunctionBuilder {
            id: method.id.clone(),
            concrete_results: &mut self.concrete_results,
            signature,
            requirements: self.requirements.entry(method.id.clone()).or_default(),
            names: self.method_parameters.entry(method.id.clone()).or_default(),
        })?;
        normalize_bounds(signature);
        Ok(result)
    }

    /// Bind a default body to the declared signature. The portable template is
    /// derived at finalization, so its binders cannot drift from the method.
    pub fn bind_default<A, R>(
        &mut self,
        method: FunctionRef,
        entry: impl NativeFunction<A, R>,
    ) -> NativeResult<()> {
        self.bind_default_with(method, entry.binding())
    }

    pub fn bind_default_with(
        &mut self,
        method: FunctionRef,
        binding: NativeBinding,
    ) -> NativeResult<()> {
        if !self
            .declaration
            .methods
            .iter()
            .any(|signature| ModuleDecl::method_id(&self.id, &signature.name) == method.id)
        {
            return Err(RuntimeError::metadata_conflict(
                "unknown trait method declaration",
            ));
        }
        if self.defaults.contains_key(&method.id) {
            return Err(RuntimeError::metadata_conflict(
                "duplicate trait default binding",
            ));
        }
        self.defaults.insert(method.id, binding);
        Ok(())
    }

    pub fn finish(mut self) -> NativeResult<TraitRef> {
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
        self.lower_defaults()?;
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
