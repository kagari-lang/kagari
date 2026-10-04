//! Scoped authoring references retain the owner of each Kagari declaration.
use crate::{
    error::RuntimeError,
    native::binding::{Codec, NativeResult},
};
use kagari_common::identity::{DefinitionPath, associated_type_id};
use kagari_types::{
    declaration::{TraitDef, TypeDef, module::ModuleDecl},
    scalar::BuiltinType,
    ty::{NominalTy, Ty},
};
use std::{collections::BTreeMap, sync::Arc};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Type(pub(crate) Ty);

impl Type {
    pub fn scalar(kind: BuiltinType) -> Self {
        Self(Ty::Builtin(kind))
    }

    pub fn unit() -> Self {
        Self::scalar(BuiltinType::Unit)
    }

    pub fn bool() -> Self {
        Self::scalar(BuiltinType::Bool)
    }

    pub fn i32() -> Self {
        Self::scalar(BuiltinType::I32)
    }

    pub fn i64() -> Self {
        Self::scalar(BuiltinType::I64)
    }

    pub fn usize() -> Self {
        Self::scalar(BuiltinType::USize)
    }

    pub fn u64() -> Self {
        Self::scalar(BuiltinType::U64)
    }

    pub fn f64() -> Self {
        Self::scalar(BuiltinType::F64)
    }

    pub fn tuple(items: impl IntoIterator<Item = Type>) -> Self {
        Self(Ty::Tuple(items.into_iter().map(|ty| ty.0).collect()))
    }

    pub fn function(params: impl IntoIterator<Item = Type>, result: Type) -> Self {
        Self(Ty::Function {
            params: params.into_iter().map(|ty| ty.0).collect(),
            result: Box::new(result.0),
        })
    }

    pub fn abi(&self) -> &Ty {
        &self.0
    }
}

#[derive(Debug, Clone)]
pub struct ParameterRef {
    pub(crate) ty: Type,
}

#[derive(Debug, Clone)]
pub struct TypeRef {
    pub(crate) id: DefinitionPath,
    pub(crate) declaration: Arc<TypeDef>,
    pub(crate) parameter_names: Vec<String>,
}

impl TypeRef {
    pub fn id(&self) -> &DefinitionPath {
        &self.id
    }

    pub fn codec(&self) -> Codec {
        Codec::Object(self.id.clone())
    }

    pub fn apply(&self, arguments: impl IntoIterator<Item = Type>) -> NativeResult<Type> {
        let arguments: Vec<_> = arguments.into_iter().map(|ty| ty.0).collect();
        if arguments.len() != self.declaration.generic_params.len() {
            return Err(RuntimeError::metadata_conflict(
                "native type argument count",
            ));
        }
        Ok(Type(Ty::NativeObject(NominalTy {
            declaration: self.id.clone(),
            arguments,
            associated_types: BTreeMap::new(),
        })))
    }
}

/// A concrete specialization or the complete generic declaration being implemented.
pub enum Receiver {
    Concrete(Type),
    Declaration(TypeRef),
}

impl From<Type> for Receiver {
    fn from(ty: Type) -> Self {
        Self::Concrete(ty)
    }
}

impl From<TypeRef> for Receiver {
    fn from(ty: TypeRef) -> Self {
        Self::Declaration(ty)
    }
}

impl ParameterRef {
    pub fn ty(&self) -> Type {
        self.ty.clone()
    }
}

#[derive(Debug, Clone)]
pub struct TraitRef {
    pub(crate) id: DefinitionPath,
    pub(crate) contract: Arc<TraitDef>,
}

impl TraitRef {
    pub fn apply(&self, arguments: impl IntoIterator<Item = Type>) -> AppliedTrait {
        AppliedTrait {
            contract: self.clone(),
            ty: NominalTy {
                declaration: self.id.clone(),
                arguments: arguments.into_iter().map(|ty| ty.0).collect(),
                associated_types: BTreeMap::new(),
            },
        }
    }

    pub fn method(&self, name: &str) -> NativeResult<MethodRef> {
        if !self
            .contract
            .methods
            .iter()
            .any(|method| method.name == name)
        {
            return Err(RuntimeError::metadata_conflict(
                "unknown Kagari trait method",
            ));
        }
        Ok(MethodRef {
            owner: self.clone(),
            id: kagari_method_id(&self.id, name),
        })
    }

    pub fn receiver(&self) -> Type {
        Type(Ty::SelfType(self.id.clone()))
    }

    pub fn id(&self) -> &DefinitionPath {
        &self.id
    }
}

fn kagari_method_id(owner: &DefinitionPath, name: &str) -> DefinitionPath {
    ModuleDecl::method_id(owner, name)
}

#[derive(Debug, Clone)]
pub struct AppliedTrait {
    pub(crate) contract: TraitRef,
    pub(crate) ty: NominalTy,
}

impl AppliedTrait {
    pub fn associated(mut self, name: &str, ty: Type) -> NativeResult<Self> {
        let id = associated_type_id(&self.contract.id, name);
        if !self
            .contract
            .contract
            .associated_types
            .iter()
            .any(|member| member.declaration == id)
            || self.ty.associated_types.insert(id, ty.0).is_some()
        {
            return Err(RuntimeError::metadata_conflict(
                "unknown or duplicate associated type",
            ));
        }
        Ok(self)
    }

    pub fn ty(&self) -> Type {
        Type(Ty::Trait(self.ty.clone()))
    }
}

#[derive(Debug, Clone)]
pub struct MethodRef {
    pub(crate) owner: TraitRef,
    pub(crate) id: DefinitionPath,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FunctionRef {
    pub(crate) id: DefinitionPath,
}

impl FunctionRef {
    pub fn id(&self) -> &DefinitionPath {
        &self.id
    }
}
