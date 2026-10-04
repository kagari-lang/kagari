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
use kagari_common::identity::{DefinitionKind, DefinitionPath, associated_type_id};
use kagari_types::{
    collection::CollectionAccess,
    declaration::{
        AssociatedTypeDef, TraitDef, module::ModuleDecl, requirement::NativeCallableRequirement,
    },
    ty::{Constraint, GenericParam, NominalTy, Ty},
};
use std::{collections::BTreeMap, sync::Arc};

mod defaults;

pub struct TraitBuilder<'module> {
    module: &'module mut ModuleBuilder,
    id: DefinitionPath,
    declaration: TraitDef,
    parameter_names: Vec<String>,
    method_parameters: BTreeMap<DefinitionPath, Vec<String>>,
    requirements: BTreeMap<DefinitionPath, Vec<NativeCallableRequirement>>,
    documentation: BTreeMap<DefinitionPath, String>,
    defaults: BTreeMap<DefinitionPath, NativeBinding>,
    concrete_results: BTreeMap<DefinitionPath, Ty>,
}

impl<'module> TraitBuilder<'module> {
    pub(crate) fn new(module: &'module mut ModuleBuilder, name: String) -> Self {
        let id = module.declaration.definition(DefinitionKind::Trait, &name);
        Self {
            module,
            id,
            declaration: TraitDef {
                conversion_adapter: None,
                storage_access: None,
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
            documentation: BTreeMap::new(),
            defaults: BTreeMap::new(),
            concrete_results: BTreeMap::new(),
        }
    }

    /// Set the complete trait-level Markdown documentation.
    pub fn documentation(&mut self, text: impl Into<String>) {
        self.documentation.insert(self.id.clone(), text.into());
    }

    /// Document an already declared associated type; names are checked against ownership.
    pub fn associated_type_documentation(
        &mut self,
        name: &str,
        text: impl Into<String>,
    ) -> NativeResult<()> {
        let id = associated_type_id(&self.id, name);
        if !self
            .declaration
            .associated_types
            .iter()
            .any(|member| member.declaration == id)
        {
            return Err(RuntimeError::metadata_conflict(
                "unknown associated type documentation target",
            ));
        }
        self.documentation.insert(id, text.into());
        Ok(())
    }

    /// Declare the access capability of an installed storage interface. Readonly
    /// interfaces may view mutable storage without granting mutable operations;
    /// generic arguments remain invariant. Source traits cannot author this fact.
    pub fn storage_view(&mut self, access: CollectionAccess) {
        self.declaration.storage_access = Some(access);
    }

    pub fn type_parameter(&mut self, name: impl Into<String>) -> NativeResult<ParameterRef> {
        let name = name.into();
        if self.parameter_names.contains(&name) {
            return Err(RuntimeError::metadata_conflict("duplicate trait parameter"));
        }
        let parameter = GenericParam {
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
        Type(Ty::SelfType(self.id.clone()))
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
                interface: NominalTy {
                    declaration: self.id.clone(),
                    arguments: self
                        .declaration
                        .generic_params
                        .iter()
                        .map(GenericParam::as_type)
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
        self.declaration.associated_types.push(AssociatedTypeDef {
            declaration: id.clone(),
            generic_params: vec![],
            parameter_bounds: vec![],
            bounds: bounds
                .into_iter()
                .map(|bound| Constraint::Trait(bound.ty))
                .collect(),
        });
        let interface: Vec<_> = self
            .declaration
            .generic_params
            .iter()
            .map(GenericParam::as_type)
            .collect();
        let trait_ref = TraitRef {
            id: self.id.clone(),
            contract: Arc::new(self.declaration.clone()),
        };
        Ok(Type(Ty::Projection {
            receiver: Box::new(self.receiver().0),
            interface: Box::new(trait_ref.apply(interface.into_iter().map(Type)).ty),
            member: id,
            arguments: vec![],
        }))
    }

    pub fn define_method(&mut self, mut declaration: MethodDecl) -> NativeResult<FunctionRef> {
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
        if let Some(text) = declaration.signature.documentation.take() {
            self.documentation.insert(id.clone(), text);
        }
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
        Arc::make_mut(&mut self.module.providers.documentation).extend(self.documentation.clone());
        self.module
            .declaration
            .documentation
            .extend(self.documentation);
        self.module.declaration.traits.push(self.declaration);
        Ok(result)
    }
}
