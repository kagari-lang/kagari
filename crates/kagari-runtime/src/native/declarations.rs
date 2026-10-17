//! Explicit scoped signatures precede Rust implementation binding.
use crate::{
    error::RuntimeError,
    native::{
        binding::NativeResult,
        types::{AppliedTrait, MethodRef, ParameterRef, Type, same_definition},
    },
};
use kagari_common::{
    cancellation::CancellationToken,
    identity::{
        map::DefinitionContext,
        metadata::DefinitionMetadata,
        table::{DefinitionId, DefinitionTable},
    },
};
use kagari_types::{
    callable::{CallableImplementation, MethodPolicy},
    declaration::{
        FnDecl, NativeDeclaration, Param, host::HostCallPolicy,
        requirement::NativeCallableRequirement,
    },
    reference::ReferenceAccess,
    ty::{GenericBound, GenericParam, Ty},
};
use std::collections::BTreeMap;

pub(crate) fn normalize_bounds(
    signature: &mut FnDecl,
    definitions: &DefinitionTable,
) -> NativeResult<()> {
    signature.bounds = GenericBound::normalize(
        signature.bounds.clone(),
        definitions,
        &CancellationToken::default(),
    )
    .map_err(|error| conflict(format!("native bounds: {error:?}")))?;
    Ok(())
}

/// Input parameter handles retain their own scopes until the enclosing builder adopts them.
#[derive(Debug, Clone)]
pub struct FunctionDecl {
    pub(crate) host_policy: Option<HostCallPolicy>,
    pub(crate) documentation: Option<String>,
    pub(crate) name: String,
    pub(crate) params: Vec<(String, Type)>,
    pub(crate) result: Type,
}

impl FunctionDecl {
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            host_policy: None,
            documentation: None,
            name: name.into(),
            params: vec![],
            result: Type::unit(),
        }
    }

    pub fn documentation(mut self, text: impl Into<String>) -> Self {
        self.documentation = Some(text.into());
        self
    }

    /// Declare external-ID passing styles and effects beside the ordinary signature.
    /// Only a checked HostCallContext adapter may bind this policy.
    pub fn host_policy(mut self, policy: HostCallPolicy) -> Self {
        self.host_policy = Some(policy);
        self
    }

    pub fn parameter(mut self, name: impl Into<String>, ty: Type) -> Self {
        self.params.push((name.into(), ty));
        self
    }

    pub fn returns(mut self, ty: Type) -> Self {
        self.result = ty;
        self
    }

    pub(crate) fn lower(
        self,
        implementation: CallableImplementation,
        context: &DefinitionContext,
    ) -> NativeResult<FnDecl> {
        Ok(FnDecl {
            host_policy: self.host_policy,
            name: self.name,
            params: self
                .params
                .into_iter()
                .map(|(name, ty)| {
                    Ok(Param {
                        name,
                        ty: ty.adopt(context)?,
                        mutable: false,
                    })
                })
                .collect::<NativeResult<_>>()?,
            return_type: self.result.adopt(context)?,
            implementation,
            generic_params: vec![],
            bounds: vec![],
            method_policy: MethodPolicy::default(),
        })
    }
}

#[derive(Debug, Clone)]
pub struct MethodDecl {
    pub(crate) signature: FunctionDecl,
    pub(crate) instance: bool,
    receiver_access: Option<ReferenceAccess>,
}

impl MethodDecl {
    /// Declare passing/effects for an explicit external-ID member callback.
    pub fn host_policy(mut self, policy: HostCallPolicy) -> Self {
        self.signature.host_policy = Some(policy);
        self
    }

    /// Require a short shared or exclusive host loan for this instance member.
    /// The common binding adapter must agree with this declared capability.
    pub fn receiver_access(mut self, access: ReferenceAccess) -> Self {
        self.receiver_access = Some(access);
        self
    }

    pub fn documentation(mut self, text: impl Into<String>) -> Self {
        self.signature.documentation = Some(text.into());
        self
    }

    pub fn instance(name: impl Into<String>) -> Self {
        Self {
            signature: FunctionDecl::new(name),
            instance: true,
            receiver_access: None,
        }
    }

    pub fn static_method(name: impl Into<String>) -> Self {
        Self {
            signature: FunctionDecl::new(name),
            instance: false,
            receiver_access: None,
        }
    }

    pub fn parameter(mut self, name: impl Into<String>, ty: Type) -> Self {
        self.signature = self.signature.parameter(name, ty);
        self
    }

    pub fn returns(mut self, ty: Type) -> Self {
        self.signature = self.signature.returns(ty);
        self
    }

    pub(crate) fn lower(
        mut self,
        receiver: Type,
        context: &DefinitionContext,
    ) -> NativeResult<FnDecl> {
        if self.instance {
            self.signature.params.insert(0, ("self".into(), receiver));
        }
        if self.receiver_access.is_some() && !self.instance {
            return Err(conflict("receiver access requires an instance method"));
        }
        let mut signature = self
            .signature
            .lower(CallableImplementation::Required, context)?;
        signature.method_policy.receiver_access = self.receiver_access;
        Ok(signature)
    }
}

#[derive(Debug, Clone)]
pub struct CallableRequirement {
    pub(crate) requirement: DefinitionMetadata<NativeCallableRequirement>,
}

impl CallableRequirement {
    pub fn arguments(mut self, arguments: impl IntoIterator<Item = Type>) -> NativeResult<Self> {
        let context = DefinitionContext::new().map_err(conflict)?;
        let mut requirement = self
            .requirement
            .import_into(&context, &CancellationToken::default())
            .map_err(conflict)?
            .into_records();
        requirement.arguments = arguments
            .into_iter()
            .map(|ty| ty.adopt(&context))
            .collect::<NativeResult<_>>()?;
        self.requirement = DefinitionMetadata::checked(
            context.snapshot(),
            requirement,
            &CancellationToken::default(),
        )
        .map_err(conflict)?;
        Ok(self)
    }

    pub fn method(receiver: Type, method: MethodRef) -> NativeResult<Self> {
        let interface = method.owner.apply([])?;
        Self::applied_method(receiver, interface, method)
    }

    pub fn applied_method(
        receiver: Type,
        interface: AppliedTrait,
        method: MethodRef,
    ) -> NativeResult<Self> {
        if !same_definition(
            interface.contract.id,
            &interface.contract.definitions,
            method.owner.id,
            &method.owner.definitions,
        ) {
            return Err(conflict("callable member belongs to another trait"));
        }
        let context = DefinitionContext::new().map_err(conflict)?;
        let receiver = receiver.adopt(&context)?;
        let Ty::Trait(interface) = interface.ty().adopt(&context)? else {
            unreachable!("trait input")
        };
        let member = context
            .import(method.definitions(), [method.id])
            .map_err(conflict)?
            .map(method.id)
            .map_err(conflict)?;
        let requirement = NativeCallableRequirement {
            receiver,
            interface,
            member,
            arguments: vec![],
        };
        Ok(Self {
            requirement: DefinitionMetadata::checked(
                context.snapshot(),
                requirement,
                &CancellationToken::default(),
            )
            .map_err(conflict)?,
        })
    }
}

#[derive(Debug, Clone)]
pub struct SelectedCall {
    pub(crate) declaration: DefinitionId,
    pub(crate) definitions: DefinitionTable,
    pub(crate) slot: usize,
}

impl SelectedCall {
    /// Recover a token from an exact scoped requirement. Invocation still checks
    /// its declaration owner and consumes the installed compiler-selected operation.
    pub fn from_declaration(
        declaration: &DefinitionMetadata<NativeDeclaration>,
        requirement: &NativeCallableRequirement,
    ) -> NativeResult<Self> {
        let mut matches = declaration
            .records()
            .callable_requirements
            .iter()
            .enumerate()
            .filter(|(_, candidate)| *candidate == requirement);
        let (slot, _) = matches
            .next()
            .ok_or_else(|| conflict("unknown native callable requirement"))?;
        if matches.next().is_some() {
            return Err(conflict("ambiguous native callable requirement"));
        }
        Ok(Self {
            declaration: declaration.records().declaration,
            definitions: declaration.definitions().clone(),
            slot,
        })
    }
}

pub struct FunctionBuilder<'a> {
    pub(crate) id: DefinitionId,
    pub(crate) context: &'a DefinitionContext,
    pub(crate) concrete_results: &'a mut BTreeMap<DefinitionId, Ty>,
    pub(crate) signature: &'a mut FnDecl,
    pub(crate) requirements: &'a mut Vec<NativeCallableRequirement>,
    pub(crate) names: &'a mut Vec<String>,
}

impl FunctionBuilder<'_> {
    pub fn type_parameter(&mut self, name: impl Into<String>) -> NativeResult<ParameterRef> {
        let name = name.into();
        if self.names.contains(&name) {
            return Err(conflict("duplicate type parameter"));
        }
        let parameter = GenericParam {
            owner: self.id,
            position: self.names.len(),
        };
        let ty = Type::from_semantic(parameter.as_type(), &self.context.snapshot())?;
        self.names.push(name);
        self.signature.generic_params.push(parameter);
        Ok(ParameterRef { ty })
    }

    pub fn parameter(&mut self, name: impl Into<String>, ty: Type) -> NativeResult<()> {
        let ty = ty.adopt(self.context)?;
        self.signature.params.push(Param {
            name: name.into(),
            ty,
            mutable: false,
        });
        Ok(())
    }

    pub fn returns(&mut self, ty: Type) -> NativeResult<()> {
        self.signature.return_type = ty.adopt(self.context)?;
        Ok(())
    }

    /// The exported result stays an interface; compilation verifies its construction.
    pub fn produces(&mut self, ty: Type) -> NativeResult<()> {
        self.concrete_results
            .insert(self.id, ty.adopt(self.context)?);
        Ok(())
    }

    pub fn bound(&mut self, ty: Type, contract: AppliedTrait) -> NativeResult<()> {
        let ty = ty.adopt(self.context)?;
        let Ty::Trait(contract) = contract.ty().adopt(self.context)? else {
            unreachable!("trait input")
        };
        self.signature.bounds.push(GenericBound {
            ty,
            constraints: vec![contract],
        });
        Ok(())
    }

    pub fn requires(&mut self, requirement: CallableRequirement) -> NativeResult<SelectedCall> {
        let requirement = requirement
            .requirement
            .import_into(self.context, &CancellationToken::default())
            .map_err(conflict)?
            .into_records();
        let slot = self.requirements.len();
        self.requirements.push(requirement);
        Ok(SelectedCall {
            declaration: self.id,
            definitions: self.context.snapshot(),
            slot,
        })
    }
}

fn conflict(error: impl ToString) -> RuntimeError {
    RuntimeError::metadata_conflict(error.to_string())
}
