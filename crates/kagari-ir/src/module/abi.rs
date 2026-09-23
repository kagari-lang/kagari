pub use kagari_hir::builtin::surface::StandardEnum as StandardEnumKind;
pub use kagari_hir::types::BuiltinType;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModuleAbi {
    pub public_items: PublicAbiItemBuffer,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum PublicAbiItem {
    Function(FunctionAbi),
    Const(ConstAbi),
    Type(TypeAbi),
    Trait(TraitAbi),
    InterfaceTable(InterfaceTableAbi),
}

impl PublicAbiItem {
    pub fn name(&self) -> &str {
        match self {
            Self::Function(item) => &item.name,
            Self::Const(item) => &item.name,
            Self::Type(item) => &item.name,
            Self::Trait(item) => &item.name,
            Self::InterfaceTable(item) => &item.name,
        }
    }

    pub fn category(&self) -> &'static str {
        match self {
            Self::Function(_) => "function",
            Self::Const(_) => "const",
            Self::Type(_) => "type",
            Self::Trait(_) => "trait",
            Self::InterfaceTable(_) => "interface_table",
        }
    }

    pub fn fingerprint_name(&self) -> String {
        format!("{}:{}", self.category(), self.name())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FunctionAbi {
    pub name: String,
    pub generic_params: Vec<GenericParameterAbi>,
    pub bounds: Vec<GenericBoundAbi>,
    pub params: Vec<ParameterAbi>,
    pub return_type: AbiType,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ParameterAbi {
    pub name: String,
    pub ty: AbiType,
    pub mutable: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConstAbi {
    pub name: String,
    pub ty: AbiType,
    pub value: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TypeAbi {
    pub name: String,
    pub kind: TypeAbiKind,
    pub generic_params: Vec<GenericParameterAbi>,
    pub bounds: Vec<GenericBoundAbi>,
    pub fields: Vec<FieldAbi>,
    pub variants: Vec<VariantAbi>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TypeAbiKind {
    Struct,
    Enum,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FieldAbi {
    pub name: String,
    pub ty: AbiType,
    pub mutable: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct VariantAbi {
    pub name: String,
    pub payload: Vec<AbiType>,
}

/// Semantic ABI types preserve nominal identity and container arguments, whereas
/// ValueType describes only the representation used by instruction operands.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct NominalAbiType {
    pub declaration: kagari_common::identity::DefinitionId,
    pub arguments: Vec<AbiType>,
}

impl NominalAbiType {
    pub(crate) fn from_checked_type(ty: &kagari_hir::types::NominalType) -> Self {
        Self {
            declaration: ty.declaration.clone(),
            arguments: ty
                .arguments
                .iter()
                .map(AbiType::from_checked_type)
                .collect(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum AbiType {
    Host(kagari_common::identity::DefinitionId),
    /// Receiver template in a trait signature, never an executable value layout.
    SelfType(kagari_common::identity::DefinitionId),
    /// Valid only in a declaration template, never in an executable layout.
    Parameter {
        owner: kagari_common::identity::DefinitionId,
        position: usize,
    },
    Builtin(kagari_hir::types::BuiltinType),
    Tuple(Vec<AbiType>),
    Array(Box<AbiType>),
    Map {
        key: Box<AbiType>,
        value: Box<AbiType>,
    },
    Set(Box<AbiType>),
    Struct(NominalAbiType),
    Enum(NominalAbiType),
    Trait(NominalAbiType),
    StandardEnum {
        kind: kagari_hir::builtin::surface::StandardEnum,
        args: Vec<AbiType>,
    },
}

impl AbiType {
    pub fn representation(&self) -> super::ValueType {
        match self {
            Self::Host(_) => super::ValueType::HostHandle,
            Self::Builtin(ty) => {
                super::ValueType::from_type_id(&kagari_hir::types::TypeId::Builtin(*ty))
            }
            _ => super::ValueType::HeapObject,
        }
    }

    pub(crate) fn from_checked_type(ty: &kagari_hir::types::TypeId) -> Self {
        use kagari_hir::types::TypeId;
        match ty {
            TypeId::Host(id) => Self::Host(id.clone()),
            TypeId::Builtin(ty) => Self::Builtin(*ty),
            TypeId::Tuple(elements) => {
                Self::Tuple(elements.iter().map(Self::from_checked_type).collect())
            }
            TypeId::Array(element) => Self::Array(Box::new(Self::from_checked_type(element))),
            TypeId::Map { key, value } => Self::Map {
                key: Box::new(Self::from_checked_type(key)),
                value: Box::new(Self::from_checked_type(value)),
            },
            TypeId::Set(element) => Self::Set(Box::new(Self::from_checked_type(element))),
            TypeId::Struct(ty) => Self::Struct(NominalAbiType::from_checked_type(ty)),
            TypeId::Enum(ty) => Self::Enum(NominalAbiType::from_checked_type(ty)),
            TypeId::Trait(ty) => Self::Trait(NominalAbiType::from_checked_type(ty)),
            TypeId::StandardEnum { kind, args } => Self::StandardEnum {
                kind: *kind,
                args: args.iter().map(Self::from_checked_type).collect(),
            },
            TypeId::Generic(parameter) => Self::Parameter {
                owner: parameter.owner.clone(),
                position: parameter.position,
            },
            TypeId::SelfType(owner) => Self::SelfType(owner.clone()),
            TypeId::Unknown | TypeId::Error => {
                unreachable!("non-concrete type reached concrete ABI encoding")
            }
        }
    }

    pub(crate) fn instantiate(
        &self,
        owner: &kagari_common::identity::DefinitionId,
        arguments: &[AbiType],
    ) -> Option<Self> {
        let nominal = |ty: &NominalAbiType| -> Option<NominalAbiType> {
            Some(NominalAbiType {
                declaration: ty.declaration.clone(),
                arguments: ty
                    .arguments
                    .iter()
                    .map(|ty| ty.instantiate(owner, arguments))
                    .collect::<Option<_>>()?,
            })
        };
        Some(match self {
            Self::Parameter {
                owner: parameter_owner,
                position,
            } if owner == parameter_owner => arguments.get(*position)?.clone(),
            Self::Parameter { .. } | Self::SelfType(_) => return None,
            Self::Builtin(_) | Self::Host(_) => self.clone(),
            Self::Tuple(types) => Self::Tuple(
                types
                    .iter()
                    .map(|ty| ty.instantiate(owner, arguments))
                    .collect::<Option<_>>()?,
            ),
            Self::Array(ty) => Self::Array(Box::new(ty.instantiate(owner, arguments)?)),
            Self::Set(ty) => Self::Set(Box::new(ty.instantiate(owner, arguments)?)),
            Self::Map { key, value } => Self::Map {
                key: Box::new(key.instantiate(owner, arguments)?),
                value: Box::new(value.instantiate(owner, arguments)?),
            },
            Self::StandardEnum { kind, args } => Self::StandardEnum {
                kind: *kind,
                args: args
                    .iter()
                    .map(|ty| ty.instantiate(owner, arguments))
                    .collect::<Option<_>>()?,
            },
            Self::Struct(ty) => Self::Struct(nominal(ty)?),
            Self::Enum(ty) => Self::Enum(nominal(ty)?),
            Self::Trait(ty) => Self::Trait(nominal(ty)?),
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TraitAbi {
    pub name: String,
    pub generic_params: Vec<GenericParameterAbi>,
    pub bounds: Vec<GenericBoundAbi>,
    pub methods: Vec<FunctionAbi>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InterfaceTableAbi {
    pub name: String,
    pub generic_params: Vec<GenericParameterAbi>,
    pub bounds: Vec<GenericBoundAbi>,
    pub trait_type: AbiType,
    pub for_type: AbiType,
    pub methods: Vec<FunctionAbi>,
}

mod wire;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GenericParameterAbi {
    pub owner: kagari_common::identity::DefinitionId,
    pub position: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GenericBoundAbi {
    pub owner: kagari_common::identity::DefinitionId,
    pub position: usize,
    pub constraints: Vec<ConstraintAbi>,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum ConstraintAbi {
    Standard(kagari_hir::builtin::surface::StandardTypeConstraint),
    Trait(kagari_common::identity::DefinitionId),
}

pub type PublicAbiItemBuffer = Vec<PublicAbiItem>;

pub(crate) mod verify;
