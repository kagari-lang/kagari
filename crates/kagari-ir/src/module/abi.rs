pub use kagari_hir::builtin::surface::StandardEnum as StandardEnumKind;
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
    pub generic_params: Vec<String>,
    pub bounds: Vec<String>,
    pub params: Vec<ParameterAbi>,
    pub return_type: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ParameterAbi {
    pub name: String,
    pub ty: String,
    pub mutable: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConstAbi {
    pub name: String,
    pub ty: String,
    pub value: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TypeAbi {
    pub name: String,
    pub kind: TypeAbiKind,
    pub generic_params: Vec<String>,
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
    pub ty: String,
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

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum AbiType {
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
            Self::Builtin(ty) => {
                super::ValueType::from_type_id(&kagari_hir::types::TypeId::Builtin(*ty))
            }
            _ => super::ValueType::HeapObject,
        }
    }

    pub(crate) fn from_checked_type(ty: &kagari_hir::types::TypeId) -> Self {
        use kagari_hir::types::TypeId;
        match ty {
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
            TypeId::Unknown | TypeId::Error | TypeId::SelfType(_) => {
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
            Self::Parameter { .. } => return None,
            Self::Builtin(_) => self.clone(),
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
    pub generic_params: Vec<String>,
    pub methods: Vec<FunctionAbi>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InterfaceTableAbi {
    pub name: String,
    pub trait_name: String,
    pub for_type: String,
    pub methods: Vec<FunctionAbi>,
}

pub type PublicAbiItemBuffer = Vec<PublicAbiItem>;
