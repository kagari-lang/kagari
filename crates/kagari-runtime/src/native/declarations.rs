//! Explicit function and method signatures precede Rust implementation binding.
use crate::{
    error::RuntimeError,
    native::{
        binding::NativeResult,
        types::{AppliedTrait, MethodRef, ParameterRef, Type},
    },
};
use kagari_abi::{
    callable::{CallableImplementation, MethodPolicy},
    native_import::callables::NativeCallableRequirement,
    types::{ConstraintAbi, FunctionAbi, GenericBoundAbi, GenericParameterAbi, ParameterAbi},
};
use kagari_common::identity::DefinitionId;

#[derive(Debug, Clone)]
pub struct FunctionDecl {
    pub(crate) name: String,
    pub(crate) params: Vec<ParameterAbi>,
    pub(crate) result: Type,
}
impl FunctionDecl {
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            params: vec![],
            result: Type::unit(),
        }
    }
    pub fn parameter(mut self, name: impl Into<String>, ty: Type) -> Self {
        self.params.push(ParameterAbi {
            name: name.into(),
            ty: ty.0,
            mutable: false,
        });
        self
    }
    pub fn returns(mut self, ty: Type) -> Self {
        self.result = ty;
        self
    }
    pub(crate) fn lower(self, implementation: CallableImplementation) -> FunctionAbi {
        FunctionAbi {
            name: self.name,
            params: self.params,
            return_type: self.result.0,
            implementation,
            generic_params: vec![],
            bounds: vec![],
            method_policy: MethodPolicy::default(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct MethodDecl {
    pub(crate) signature: FunctionDecl,
    pub(crate) instance: bool,
}
impl MethodDecl {
    pub fn instance(name: impl Into<String>) -> Self {
        Self {
            signature: FunctionDecl::new(name),
            instance: true,
        }
    }
    pub fn static_method(name: impl Into<String>) -> Self {
        Self {
            signature: FunctionDecl::new(name),
            instance: false,
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
    pub(crate) fn lower(mut self, receiver: Type) -> FunctionAbi {
        if self.instance {
            self.signature.params.insert(
                0,
                ParameterAbi {
                    name: "self".into(),
                    ty: receiver.0,
                    mutable: false,
                },
            );
        }
        self.signature.lower(CallableImplementation::Required)
    }
}

#[derive(Debug, Clone)]
pub struct CallableRequirement {
    pub(crate) requirement: NativeCallableRequirement,
}
impl CallableRequirement {
    pub fn method(receiver: Type, method: MethodRef) -> Self {
        Self {
            requirement: NativeCallableRequirement {
                receiver: receiver.0,
                interface: method.owner.apply([]).ty,
                member: method.id,
                arguments: vec![],
            },
        }
    }
    pub fn applied_method(
        receiver: Type,
        interface: AppliedTrait,
        method: MethodRef,
    ) -> NativeResult<Self> {
        if interface.contract.id != method.owner.id {
            return Err(RuntimeError::metadata_conflict(
                "callable member belongs to another trait",
            ));
        }
        Ok(Self {
            requirement: NativeCallableRequirement {
                receiver: receiver.0,
                interface: interface.ty,
                member: method.id,
                arguments: vec![],
            },
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SelectedCall {
    pub(crate) slot: usize,
}

pub struct FunctionBuilder<'a> {
    pub(crate) id: DefinitionId,
    pub(crate) signature: &'a mut FunctionAbi,
    pub(crate) requirements: &'a mut Vec<NativeCallableRequirement>,
    pub(crate) names: &'a mut Vec<String>,
}
impl FunctionBuilder<'_> {
    pub fn type_parameter(&mut self, name: impl Into<String>) -> NativeResult<ParameterRef> {
        let name = name.into();
        if self.names.contains(&name) {
            return Err(RuntimeError::metadata_conflict("duplicate type parameter"));
        }
        let parameter = GenericParameterAbi {
            owner: self.id.clone(),
            position: self.names.len(),
        };
        self.names.push(name);
        self.signature.generic_params.push(parameter.clone());
        Ok(ParameterRef {
            ty: Type(parameter.as_type()),
        })
    }
    pub fn parameter(&mut self, name: impl Into<String>, ty: Type) {
        self.signature.params.push(ParameterAbi {
            name: name.into(),
            ty: ty.0,
            mutable: false,
        });
    }
    pub fn returns(&mut self, ty: Type) {
        self.signature.return_type = ty.0;
    }
    pub fn bound(&mut self, ty: Type, contract: AppliedTrait) {
        self.signature.bounds.push(GenericBoundAbi {
            ty: ty.0,
            constraints: vec![ConstraintAbi::Trait(contract.ty)],
        });
    }
    pub fn requires(&mut self, requirement: CallableRequirement) -> SelectedCall {
        let slot = self.requirements.len();
        self.requirements.push(requirement.requirement);
        SelectedCall { slot }
    }
}
