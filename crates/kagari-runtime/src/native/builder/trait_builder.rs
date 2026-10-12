//! A trait declaration is a Kagari contract, independent of any Rust trait.
use crate::{
    error::RuntimeError,
    native::{
        binding::{NativeBinding, NativeResult},
        builder::ModuleBuilder,
        conversion::{FromKagari, IntoKagari, arguments::FromKagariArguments},
        declarations::{CallableRequirement, FunctionBuilder, MethodDecl, normalize_bounds},
        functions::NativeFunction,
        typed::NativeContext,
        types::{AppliedTrait, FunctionRef, ParameterRef, TraitRef, Type},
    },
};
use kagari_common::{
    cancellation::CancellationToken,
    identity::{DefinitionKind, metadata::DefinitionMetadata, table::DefinitionId},
};
use kagari_types::{
    declaration::{
        AssociatedTypeDef, TraitDef, module::ModuleDecl, requirement::NativeCallableRequirement,
    },
    ty::{Constraint, GenericParam, NominalTy, Ty},
};
use std::{collections::BTreeMap, fmt, sync::Arc};

mod defaults;

pub struct TraitBuilder<'module> {
    module: &'module mut ModuleBuilder,
    id: DefinitionId,
    declaration: TraitDef,
    parameter_names: Vec<String>,
    method_parameters: BTreeMap<DefinitionId, Vec<String>>,
    requirements: BTreeMap<DefinitionId, Vec<NativeCallableRequirement>>,
    documentation: BTreeMap<DefinitionId, String>,
    defaults: BTreeMap<DefinitionId, NativeBinding>,
    concrete_results: BTreeMap<DefinitionId, Ty>,
}

impl<'module> TraitBuilder<'module> {
    pub(crate) fn new(module: &'module mut ModuleBuilder, name: String) -> NativeResult<Self> {
        let id = module.definition(DefinitionKind::Trait, &name)?;
        Ok(Self {
            module,
            id,
            declaration: TraitDef {
                conversion_adapter: None,
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
        })
    }

    /// Set the complete trait-level Markdown documentation.
    pub fn documentation(&mut self, text: impl Into<String>) {
        self.documentation.insert(self.id, text.into());
    }

    /// Document an already declared associated type; names are checked against ownership.
    pub fn associated_type_documentation(
        &mut self,
        name: &str,
        text: impl Into<String>,
    ) -> NativeResult<()> {
        let id = self
            .module
            .providers
            .definitions()
            .lookup_child(self.id, DefinitionKind::AssociatedType, name, 0)
            .ok_or_else(|| conflict("unknown associated type documentation target"))?;
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

    pub fn type_parameter(&mut self, name: impl Into<String>) -> NativeResult<ParameterRef> {
        let name = name.into();
        if self.parameter_names.contains(&name) {
            return Err(RuntimeError::metadata_conflict("duplicate trait parameter"));
        }
        let parameter = GenericParam {
            owner: self.id,
            position: self.parameter_names.len(),
        };
        self.parameter_names.push(name);
        self.declaration.generic_params.push(parameter.clone());
        Ok(ParameterRef {
            ty: Type::from_semantic(parameter.as_type(), &self.module.providers.definitions())?,
        })
    }

    pub fn receiver(&self) -> NativeResult<Type> {
        Type::from_semantic(Ty::SelfType(self.id), &self.module.providers.definitions())
    }

    /// Refer to an operation on this default body's actual receiver. The final
    /// declaration, including any later method edits, is checked at installation.
    pub fn operation(&self, method: &FunctionRef) -> NativeResult<CallableRequirement> {
        if !self.declaration.methods.iter().any(|signature| {
            ModuleDecl::method_id(
                self.id,
                &signature.name,
                &self.module.providers.definitions(),
            )
            .ok()
                == Some(method.id)
        }) {
            return Err(RuntimeError::metadata_conflict(
                "unknown trait method declaration",
            ));
        }
        Ok(CallableRequirement {
            requirement: DefinitionMetadata::checked(
                self.module.providers.definitions(),
                NativeCallableRequirement {
                    receiver: Ty::SelfType(self.id),
                    interface: NominalTy {
                        declaration: self.id,
                        arguments: self
                            .declaration
                            .generic_params
                            .iter()
                            .map(GenericParam::as_type)
                            .collect(),
                        associated_types: BTreeMap::new(),
                    },
                    member: method.id,
                    arguments: vec![],
                },
                &CancellationToken::default(),
            )
            .map_err(conflict)?,
        })
    }

    pub fn parent(&mut self, parent: AppliedTrait) -> NativeResult<()> {
        let Ty::Trait(parent) = parent.ty().adopt(self.module.context())? else {
            unreachable!("trait input")
        };
        self.declaration.supertraits.push(parent);
        Ok(())
    }

    pub fn associated_type(
        &mut self,
        name: &str,
        bounds: impl IntoIterator<Item = AppliedTrait>,
    ) -> NativeResult<Type> {
        let id = self
            .module
            .context()
            .intern_child(self.id, DefinitionKind::AssociatedType, name, 0)
            .map_err(conflict)?;
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
            declaration: id,
            generic_params: vec![],
            parameter_bounds: vec![],
            bounds: bounds
                .into_iter()
                .map(|bound| {
                    let Ty::Trait(bound) = bound.ty().adopt(self.module.context())? else {
                        unreachable!("trait input")
                    };
                    Ok(Constraint::Trait(bound))
                })
                .collect::<NativeResult<_>>()?,
        });
        let interface: Vec<_> = self
            .declaration
            .generic_params
            .iter()
            .map(GenericParam::as_type)
            .collect();
        Type::from_semantic(
            Ty::Projection {
                receiver: Box::new(Ty::SelfType(self.id)),
                interface: Box::new(NominalTy {
                    declaration: self.id,
                    arguments: interface,
                    associated_types: BTreeMap::new(),
                }),
                member: id,
                arguments: vec![],
            },
            &self.module.providers.definitions(),
        )
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
        let receiver = self.receiver()?;
        let id = self
            .module
            .context()
            .intern_child(
                self.id,
                DefinitionKind::Method,
                &declaration.signature.name,
                0,
            )
            .map_err(conflict)?;
        if let Some(text) = declaration.signature.documentation.take() {
            self.documentation.insert(id, text);
        }
        self.declaration
            .methods
            .push(declaration.lower(receiver, self.module.context())?);
        Ok(FunctionRef {
            id,
            definitions: self.module.providers.definitions(),
        })
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
            .find(|signature| {
                ModuleDecl::method_id(
                    self.id,
                    &signature.name,
                    &self.module.providers.definitions(),
                )
                .ok()
                    == Some(method.id)
            })
            .ok_or_else(|| RuntimeError::metadata_conflict("unknown trait method declaration"))?;
        let result = configure(&mut FunctionBuilder {
            id: method.id,
            context: self.module.context(),
            concrete_results: &mut self.concrete_results,
            signature,
            requirements: self.requirements.entry(method.id).or_default(),
            names: self.method_parameters.entry(method.id).or_default(),
        })?;
        normalize_bounds(signature, &self.module.providers.definitions())?;
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
        mut binding: NativeBinding,
    ) -> NativeResult<()> {
        if !self.declaration.methods.iter().any(|signature| {
            ModuleDecl::method_id(
                self.id,
                &signature.name,
                &self.module.providers.definitions(),
            )
            .ok()
                == Some(method.id)
        }) {
            return Err(RuntimeError::metadata_conflict(
                "unknown trait method declaration",
            ));
        }
        if self.defaults.contains_key(&method.id) {
            return Err(RuntimeError::metadata_conflict(
                "duplicate trait default binding",
            ));
        }
        // Lowering gives the body a private function identity. Requirement
        // tokens continue to name the authored trait method.
        binding.requirement_owner = Some((method.id, self.module.providers.definitions()));
        self.defaults.insert(method.id, binding);
        Ok(())
    }

    /// Bind an instance default against its declared, possibly generic signature.
    /// The receiver is separate from the argument tuple. Concrete Rust mappings
    /// are checked before callback effects.
    pub fn bind_default_method<S: FromKagari, A: FromKagariArguments, R: IntoKagari>(
        &mut self,
        method: FunctionRef,
        entry: impl for<'call> Fn(&mut NativeContext<'call>, S, A) -> NativeResult<R>
        + Send
        + Sync
        + 'static,
    ) -> NativeResult<()> {
        let signature = self
            .declaration
            .methods
            .iter()
            .find(|signature| {
                ModuleDecl::method_id(
                    self.id,
                    &signature.name,
                    &self.module.providers.definitions(),
                )
                .ok()
                    == Some(method.id)
            })
            .ok_or_else(|| RuntimeError::metadata_conflict("unknown trait method declaration"))?;
        if !signature
            .params
            .first()
            .is_some_and(|param| param.name == "self")
        {
            return Err(RuntimeError::metadata_conflict(
                "default requires an instance method",
            ));
        }
        let arity = signature.params.len();
        self.bind_default_with(method, NativeBinding::contextual_method(arity, entry))
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
            id: self.id,
            definitions: self.module.providers.definitions(),
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

fn conflict(message: impl fmt::Display) -> RuntimeError {
    RuntimeError::metadata_conflict(message.to_string())
}
