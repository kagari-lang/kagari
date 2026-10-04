//! Scoped implementation groups bind existing Kagari members to Rust entries.
use crate::{
    error::RuntimeError,
    native::{
        binding::{Codec, NativeBinding, NativeResult},
        builder::{ModuleBuilder, inherent::InherentMethodsBuilder},
        functions::NativeFunction,
        types::{AppliedTrait, ParameterRef, Receiver, Type},
    },
};
use kagari_common::{cancellation::CancellationToken, identity::DefinitionPath};
use kagari_types::{
    declaration::module::{ImplDecl, ModuleDecl},
    ty::{GenericParam, substitution::TypeSubstitution},
};
use std::collections::BTreeMap;

pub struct ImplementationBuilder<'module> {
    module: &'module mut ModuleBuilder,
    receiver: Type,
    receiver_codec: Option<Codec>,
    parameters: Vec<GenericParam>,
    parameter_names: Vec<String>,
}

impl<'module> ImplementationBuilder<'module> {
    pub fn inherent_impl<T>(
        &mut self,
        configure: impl FnOnce(&mut InherentMethodsBuilder) -> NativeResult<T>,
    ) -> NativeResult<T> {
        let owners = self
            .module
            .providers
            .receiver_owners(Some(&self.module.declaration))?;
        if !self
            .module
            .declaration
            .owns_inherent_receiver(&self.receiver.0, &|receiver| owners.owner(receiver))
        {
            return Err(RuntimeError::metadata_conflict(
                "inherent methods belong to the type's defining module",
            ));
        }
        let owner = self
            .module
            .declaration
            .implementation_id(self.module.declaration.implementations.len());
        let parameters: Vec<_> = self
            .parameters
            .iter()
            .map(|parameter| GenericParam {
                owner: owner.clone(),
                position: parameter.position,
            })
            .collect();
        let arguments: Vec<_> = parameters.iter().map(GenericParam::as_type).collect();
        let mut substitution = TypeSubstitution::default();
        for (source, target) in self.parameters.iter().zip(&arguments) {
            substitution.bind(&source.owner, source.position, target);
        }
        let cancel = CancellationToken::default();
        let receiver = substitution.apply(&self.receiver.0, &cancel).map_err(|_| {
            RuntimeError::metadata_conflict("invalid inherent receiver substitution")
        })?;
        let mut methods = InherentMethodsBuilder {
            owner,
            receiver: self.receiver.clone(),
            receiver_codec: self.receiver_codec.clone(),
            documentation: BTreeMap::new(),
            methods: BTreeMap::new(),
            bindings: BTreeMap::new(),
        };
        let result = configure(&mut methods)?;
        let mut signatures = Vec::new();
        for mut signature in methods.methods.into_values() {
            signature.generic_params = parameters.clone();
            for parameter in &mut signature.params {
                parameter.ty = substitution.apply(&parameter.ty, &cancel).map_err(|_| {
                    RuntimeError::metadata_conflict("invalid inherent argument substitution")
                })?;
            }
            signature.return_type = substitution
                .apply(&signature.return_type, &cancel)
                .map_err(|_| {
                    RuntimeError::metadata_conflict("invalid inherent result substitution")
                })?;
            signatures.push(signature);
        }
        self.module.declaration.implementations.push(ImplDecl {
            generic_params: parameters,
            bounds: vec![],
            trait_type: None,
            for_type: receiver,
            methods: signatures,
        });
        self.module
            .declaration
            .documentation
            .extend(methods.documentation);
        self.module.bindings.extend(methods.bindings);
        Ok(result)
    }

    pub(crate) fn new(
        module: &'module mut ModuleBuilder,
        receiver: Receiver,
    ) -> NativeResult<Self> {
        let owner = module
            .declaration
            .implementation_id(module.declaration.implementations.len());
        let (receiver, parameters, parameter_names) = match receiver {
            Receiver::Concrete(ty) => (ty, vec![], vec![]),
            Receiver::Declaration(reference) => {
                if module.providers.types.get(&reference.id)
                    != Some(&module.providers.scope(reference.declaration.as_ref())?)
                {
                    return Err(RuntimeError::metadata_conflict(
                        "native receiver declaration is not in the provider catalog",
                    ));
                }
                let parameters: Vec<_> = (0..reference.parameter_names.len())
                    .map(|position| GenericParam {
                        owner: owner.clone(),
                        position,
                    })
                    .collect();
                let ty = reference
                    .apply(parameters.iter().map(|parameter| Type(parameter.as_type())))?;
                (ty, parameters, reference.parameter_names)
            }
        };
        Ok(Self {
            module,
            receiver,
            receiver_codec: None,
            parameters,
            parameter_names,
        })
    }

    pub fn receiver(&self) -> Type {
        self.receiver.clone()
    }

    pub fn parameter(&self, name: &str) -> NativeResult<ParameterRef> {
        let index = self
            .parameter_names
            .iter()
            .position(|parameter| parameter == name)
            .ok_or_else(|| RuntimeError::metadata_conflict("unknown receiver parameter"))?;
        Ok(ParameterRef {
            ty: Type(self.parameters[index].as_type()),
        })
    }

    pub fn receiver_codec(&mut self, codec: Codec) -> NativeResult<()> {
        if !codec.accepts(self.receiver.abi(), &self.module.providers) {
            return Err(RuntimeError::metadata_conflict(
                "receiver codec differs from the Kagari type",
            ));
        }
        self.receiver_codec = Some(codec);
        Ok(())
    }

    pub fn trait_impl<T>(
        &mut self,
        applied: AppliedTrait,
        configure: impl FnOnce(&mut MethodsBuilder) -> NativeResult<T>,
    ) -> NativeResult<T> {
        if self.module.providers.get(&applied.contract.id)
            != Some(
                &self
                    .module
                    .providers
                    .scope(applied.contract.contract.as_ref())?,
            )
        {
            return Err(RuntimeError::metadata_conflict(
                "trait contract is not in this module's provider catalog",
            ));
        }
        let id = self
            .module
            .declaration
            .implementation_id(self.module.declaration.implementations.len());
        let parameters: Vec<_> = self
            .parameters
            .iter()
            .map(|parameter| GenericParam {
                owner: id.clone(),
                position: parameter.position,
            })
            .collect();
        let arguments: Vec<_> = parameters.iter().map(GenericParam::as_type).collect();
        let mut substitution = TypeSubstitution::default();
        for (source, target) in self.parameters.iter().zip(&arguments) {
            substitution.bind(&source.owner, source.position, target);
        }
        let cancel = CancellationToken::default();
        let receiver = substitution.apply(&self.receiver.0, &cancel).map_err(|_| {
            RuntimeError::metadata_conflict("invalid implementation receiver substitution")
        })?;
        let mut methods = MethodsBuilder {
            applied,
            id,
            documentation: BTreeMap::new(),
            receiver_codec: self.receiver_codec.clone(),
            bindings: BTreeMap::new(),
        };
        let result = configure(&mut methods)?;
        // The authoring closure uses its group binder. Each emitted impl owns a
        // fresh binder, including associated outputs configured in the closure.
        methods.applied.ty = substitution
            .apply_nominal(&methods.applied.ty, &cancel)
            .map_err(|_| RuntimeError::metadata_conflict("invalid applied trait substitution"))?;
        let identities: Vec<_> = methods
            .bindings
            .keys()
            .map(|name| (name.as_str(), ModuleDecl::method_id(&methods.id, name)))
            .collect();
        self.module
            .declaration
            .implement_trait(
                methods.applied.contract.contract.as_ref(),
                methods.applied.ty,
                receiver,
                parameters,
                &identities,
            )
            .map_err(|error| RuntimeError::metadata_conflict(error.to_string()))?;
        for method in &methods.applied.contract.contract.methods {
            let target = ModuleDecl::method_id(&methods.id, &method.name);
            let source = ModuleDecl::method_id(&methods.applied.contract.id, &method.name);
            if let Some(text) = methods
                .documentation
                .get(&method.name)
                .or_else(|| self.module.providers.documentation.get(&source))
            {
                self.module
                    .declaration
                    .documentation
                    .insert(target, text.clone());
            }
        }
        for (name, binding) in methods.bindings {
            self.module
                .bindings
                .insert(ModuleDecl::method_id(&methods.id, &name), binding);
        }
        Ok(result)
    }
}

pub struct MethodsBuilder {
    applied: AppliedTrait,
    id: DefinitionPath,
    receiver_codec: Option<Codec>,
    documentation: BTreeMap<String, String>,
    bindings: BTreeMap<String, NativeBinding>,
}

impl MethodsBuilder {
    /// Override the inherited trait member's Markdown on this implementation.
    pub fn documentation(&mut self, name: &str, text: impl Into<String>) -> NativeResult<()> {
        if !self
            .applied
            .contract
            .contract
            .methods
            .iter()
            .any(|method| method.name == name)
        {
            return Err(RuntimeError::metadata_conflict(
                "unknown implementation documentation target",
            ));
        }
        self.documentation.insert(name.into(), text.into());
        Ok(())
    }

    pub fn associated_type(&mut self, name: &str, ty: Type) -> NativeResult<()> {
        self.applied = self.applied.clone().associated(name, ty)?;
        Ok(())
    }

    pub fn bind<A, R>(&mut self, name: &str, entry: impl NativeFunction<A, R>) -> NativeResult<()> {
        self.bind_with(name, entry.binding())
    }

    pub fn bind_with(&mut self, name: &str, binding: NativeBinding) -> NativeResult<()> {
        let method = self
            .applied
            .contract
            .contract
            .methods
            .iter()
            .find(|method| method.name == name)
            .ok_or_else(|| RuntimeError::metadata_conflict("unknown implementation member"))?;
        let receiver = method
            .params
            .first()
            .is_some_and(|parameter| parameter.name == "self");
        if receiver
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
        if self.bindings.contains_key(name) {
            return Err(RuntimeError::metadata_conflict(
                "duplicate implementation member binding",
            ));
        }
        self.bindings.insert(name.into(), binding);
        Ok(())
    }
}
