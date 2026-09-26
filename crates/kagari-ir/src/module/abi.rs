pub use kagari_hir::builtin::surface::StandardEnum as StandardEnumKind;
pub use kagari_hir::types::BuiltinType;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModuleAbi {
    #[serde(deserialize_with = "crate::decode_limits::table")]
    pub public_items: PublicAbiItemBuffer,
    #[serde(deserialize_with = "crate::decode_limits::nested")]
    pub trait_contracts: Vec<TraitContract>,
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
        if let Self::InterfaceTable(table) = self {
            use bincode::Options;
            use std::fmt::Write;
            let encoded = bincode::DefaultOptions::new()
                .with_fixint_encoding()
                .with_little_endian()
                .serialize(&table.declaration)
                .expect("validated interface declaration identity");
            let mut name = String::from("interface_table:");
            for byte in encoded {
                write!(name, "{byte:02x}").expect("writing to a String cannot fail");
            }
            name
        } else {
            format!("{}:{}", self.category(), self.name())
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FunctionAbi {
    pub name: String,
    #[serde(deserialize_with = "crate::decode_limits::nested")]
    pub generic_params: Vec<GenericParameterAbi>,
    #[serde(deserialize_with = "crate::decode_limits::nested")]
    pub bounds: Vec<GenericBoundAbi>,
    #[serde(deserialize_with = "crate::decode_limits::nested")]
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
    #[serde(deserialize_with = "crate::decode_limits::nested")]
    pub generic_params: Vec<GenericParameterAbi>,
    #[serde(deserialize_with = "crate::decode_limits::nested")]
    pub bounds: Vec<GenericBoundAbi>,
    #[serde(deserialize_with = "crate::decode_limits::nested")]
    pub fields: Vec<FieldAbi>,
    #[serde(deserialize_with = "crate::decode_limits::nested")]
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
    #[serde(deserialize_with = "crate::decode_limits::nested")]
    pub payload: Vec<AbiType>,
}

/// Semantic ABI types preserve nominal identity and container arguments, whereas
/// ValueType describes only the representation used by instruction operands.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct NominalAbiType {
    pub declaration: kagari_common::identity::DefinitionId,
    #[serde(deserialize_with = "crate::decode_limits::nested")]
    pub arguments: Vec<AbiType>,
    #[serde(deserialize_with = "crate::decode_limits::map")]
    pub associated_types:
        std::collections::BTreeMap<kagari_common::identity::DefinitionId, AbiType>,
}

impl NominalAbiType {
    pub(crate) fn to_checked_type(&self) -> kagari_hir::types::NominalType {
        kagari_hir::types::NominalType {
            associated_types: self
                .associated_types
                .iter()
                .map(|(id, ty)| (id.clone(), ty.to_checked_type()))
                .collect(),
            declaration: self.declaration.clone(),
            arguments: self
                .arguments
                .iter()
                .map(AbiType::to_checked_type)
                .collect(),
        }
    }

    pub(crate) fn from_checked_type(ty: &kagari_hir::types::NominalType) -> Self {
        Self {
            associated_types: ty
                .associated_types
                .iter()
                .map(|(id, ty)| (id.clone(), AbiType::from_checked_type(ty)))
                .collect(),
            declaration: ty.declaration.clone(),
            arguments: ty
                .arguments
                .iter()
                .map(AbiType::from_checked_type)
                .collect(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum AbiType {
    Projection {
        receiver: Box<AbiType>,
        interface: Box<NominalAbiType>,
        member: kagari_common::identity::DefinitionId,
    },
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
    Function {
        params: Vec<AbiType>,
        result: Box<AbiType>,
    },
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
    pub(crate) fn to_checked_type(&self) -> kagari_hir::types::TypeId {
        use kagari_hir::types::{GenericParameterType, TypeId};
        match self {
            Self::Projection {
                receiver,
                interface,
                member,
            } => TypeId::Projection {
                receiver: Box::new(receiver.to_checked_type()),
                interface: Box::new(interface.to_checked_type()),
                member: member.clone(),
            },
            Self::Host(id) => TypeId::Host(id.clone()),
            Self::SelfType(id) => TypeId::SelfType(id.clone()),
            Self::Parameter { owner, position } => TypeId::Generic(GenericParameterType {
                owner: owner.clone(),
                position: *position,
                name: String::new(),
            }),
            Self::Builtin(ty) => TypeId::Builtin(*ty),
            Self::Tuple(types) => TypeId::Tuple(types.iter().map(Self::to_checked_type).collect()),
            Self::Function { params, result } => TypeId::Function {
                params: params.iter().map(Self::to_checked_type).collect(),
                result: Box::new(result.to_checked_type()),
            },
            Self::Array(ty) => TypeId::Array(Box::new(ty.to_checked_type())),
            Self::Map { key, value } => TypeId::Map {
                key: Box::new(key.to_checked_type()),
                value: Box::new(value.to_checked_type()),
            },
            Self::Set(ty) => TypeId::Set(Box::new(ty.to_checked_type())),
            Self::Struct(ty) => TypeId::Struct(ty.to_checked_type()),
            Self::Enum(ty) => TypeId::Enum(ty.to_checked_type()),
            Self::Trait(ty) => TypeId::Trait(ty.to_checked_type()),
            Self::StandardEnum { kind, args } => TypeId::StandardEnum {
                kind: *kind,
                args: args.iter().map(Self::to_checked_type).collect(),
            },
        }
    }

    pub(crate) fn from_host_type(ty: &kagari_common::host_interface::HostValueType) -> Self {
        use kagari_common::host_interface::HostValueType as Host;
        match ty {
            Host::Unit => Self::Builtin(BuiltinType::Unit),
            Host::Bool => Self::Builtin(BuiltinType::Bool),
            Host::I32 => Self::Builtin(BuiltinType::I32),
            Host::I64 => Self::Builtin(BuiltinType::I64),
            Host::F32 => Self::Builtin(BuiltinType::F32),
            Host::F64 => Self::Builtin(BuiltinType::F64),
            Host::String => Self::Builtin(BuiltinType::String),
            Host::Opaque(id) => Self::Host(id.clone()),
            Host::Tuple(types) => Self::Tuple(types.iter().map(Self::from_host_type).collect()),
            Host::Array(ty) => Self::Array(Box::new(Self::from_host_type(ty))),
            Host::Map { key, value } => Self::Map {
                key: Box::new(Self::from_host_type(key)),
                value: Box::new(Self::from_host_type(value)),
            },
            Host::Set(ty) => Self::Set(Box::new(Self::from_host_type(ty))),
            Host::Option(ty) => Self::StandardEnum {
                kind: StandardEnumKind::Option,
                args: vec![Self::from_host_type(ty)],
            },
            Host::Result { ok, error } => Self::StandardEnum {
                kind: StandardEnumKind::Result,
                args: vec![Self::from_host_type(ok), Self::from_host_type(error)],
            },
        }
    }

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
            TypeId::Projection {
                receiver,
                interface,
                member,
            } => Self::Projection {
                receiver: Box::new(Self::from_checked_type(receiver)),
                interface: Box::new(NominalAbiType::from_checked_type(interface)),
                member: member.clone(),
            },
            TypeId::Host(id) => Self::Host(id.clone()),
            TypeId::Builtin(ty) => Self::Builtin(*ty),
            TypeId::Tuple(elements) => {
                Self::Tuple(elements.iter().map(Self::from_checked_type).collect())
            }
            TypeId::Function { params, result } => Self::Function {
                params: params.iter().map(Self::from_checked_type).collect(),
                result: Box::new(Self::from_checked_type(result)),
            },
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

    pub fn is_concrete(&self) -> bool {
        let mut pending = vec![self];
        while let Some(ty) = pending.pop() {
            match ty {
                Self::Projection { .. } | Self::Parameter { .. } | Self::SelfType(_) => {
                    return false;
                }
                Self::Tuple(types) | Self::StandardEnum { args: types, .. } => {
                    pending.extend(types)
                }
                Self::Function { params, result } => {
                    pending.extend(params);
                    pending.push(result);
                }
                Self::Array(ty) | Self::Set(ty) => pending.push(ty),
                Self::Map { key, value } => pending.extend([key.as_ref(), value.as_ref()]),
                Self::Struct(ty) | Self::Enum(ty) | Self::Trait(ty) => {
                    pending.extend(&ty.arguments);
                    pending.extend(ty.associated_types.values());
                }
                Self::Host(_) | Self::Builtin(_) => {}
            }
        }
        true
    }

    pub(crate) fn instantiate(
        &self,
        owner: &kagari_common::identity::DefinitionId,
        arguments: &[AbiType],
    ) -> Option<Self> {
        let nominal = |ty: &NominalAbiType| -> Option<NominalAbiType> {
            Some(NominalAbiType {
                associated_types: ty
                    .associated_types
                    .iter()
                    .map(|(id, ty)| Some((id.clone(), ty.instantiate(owner, arguments)?)))
                    .collect::<Option<_>>()?,
                declaration: ty.declaration.clone(),
                arguments: ty
                    .arguments
                    .iter()
                    .map(|ty| ty.instantiate(owner, arguments))
                    .collect::<Option<_>>()?,
            })
        };
        Some(match self {
            Self::Projection { .. } => return None,
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
            Self::Function { params, result } => Self::Function {
                params: params
                    .iter()
                    .map(|ty| ty.instantiate(owner, arguments))
                    .collect::<Option<_>>()?,
                result: Box::new(result.instantiate(owner, arguments)?),
            },
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
    #[serde(deserialize_with = "crate::decode_limits::nested")]
    pub generic_params: Vec<GenericParameterAbi>,
    #[serde(deserialize_with = "crate::decode_limits::nested")]
    pub bounds: Vec<GenericBoundAbi>,
    #[serde(deserialize_with = "crate::decode_limits::nested")]
    pub methods: Vec<FunctionAbi>,
    #[serde(deserialize_with = "crate::decode_limits::nested")]
    pub associated_types: Vec<AssociatedTypeAbi>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AssociatedTypeAbi {
    pub declaration: kagari_common::identity::DefinitionId,
    #[serde(deserialize_with = "crate::decode_limits::nested")]
    pub bounds: Vec<ConstraintAbi>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InterfaceTableAbi {
    pub declaration: kagari_common::identity::DefinitionId,
    pub name: String,
    #[serde(deserialize_with = "crate::decode_limits::nested")]
    pub generic_params: Vec<GenericParameterAbi>,
    #[serde(deserialize_with = "crate::decode_limits::nested")]
    pub bounds: Vec<GenericBoundAbi>,
    pub trait_type: AbiType,
    pub for_type: AbiType,
    #[serde(deserialize_with = "crate::decode_limits::nested")]
    pub methods: Vec<FunctionAbi>,
}

impl InterfaceTableAbi {
    /// Substitute a selected impl's concrete arguments into its call contract.
    /// The verifier separately proves template validity, bounds and method slots.
    pub fn instantiate(&self, arguments: &[AbiType]) -> Option<Self> {
        if arguments.len() != self.generic_params.len()
            || !arguments.iter().all(AbiType::is_concrete)
        {
            return None;
        }
        let apply = |ty: &AbiType| ty.instantiate(&self.declaration, arguments);
        let methods = self
            .methods
            .iter()
            .map(|method| {
                Some(FunctionAbi {
                    name: method.name.clone(),
                    generic_params: method.generic_params.clone(),
                    bounds: method.bounds.clone(),
                    params: method
                        .params
                        .iter()
                        .map(|param| {
                            Some(ParameterAbi {
                                name: param.name.clone(),
                                mutable: param.mutable,
                                ty: apply(&param.ty)?,
                            })
                        })
                        .collect::<Option<_>>()?,
                    return_type: apply(&method.return_type)?,
                })
            })
            .collect::<Option<Vec<_>>>()?;
        Some(Self {
            declaration: self.declaration.clone(),
            name: self.name.clone(),
            generic_params: Vec::new(),
            bounds: Vec::new(),
            trait_type: apply(&self.trait_type)?,
            for_type: apply(&self.for_type)?,
            methods,
        })
    }
}

/// The physical call contract of a method on a concrete applied interface.
/// The first argument is the boxed receiver; the runtime unwraps it only after
/// checking the interface identity and selected method slot.
pub(crate) fn interface_method_types(
    owner: &kagari_common::identity::ModuleIdentity,
    public_items: &[PublicAbiItem],
    trait_contracts: &[TraitContract],
    interface: &NominalAbiType,
    slot: usize,
) -> Option<(Vec<super::ValueType>, super::ValueType)> {
    use kagari_common::identity::DefinitionKind;
    let path = &interface.declaration.path;
    if interface.declaration.module != *owner
        || path.len() != 1
        || path[0].kind != DefinitionKind::Trait
        || path[0].occurrence != 0
        || !interface.arguments.iter().all(AbiType::is_concrete)
    {
        return None;
    }
    let trait_abi = trait_contracts
        .iter()
        .find(|contract| contract.declaration == interface.declaration)
        .map(|contract| &contract.abi)
        .or_else(|| {
            public_items.iter().find_map(|item| match item {
                PublicAbiItem::Trait(trait_abi) if trait_abi.name == path[0].name => {
                    Some(trait_abi)
                }
                _ => None,
            })
        })?;
    if interface.arguments.len() != trait_abi.generic_params.len() {
        return None;
    }
    if interface.associated_types.len() != trait_abi.associated_types.len()
        || trait_abi.associated_types.iter().any(|member| {
            !interface
                .associated_types
                .get(&member.declaration)
                .is_some_and(AbiType::is_concrete)
        })
    {
        return None;
    }
    let method = trait_abi.methods.get(slot)?;
    if !method.generic_params.is_empty()
        || method.params.first()?.ty != AbiType::SelfType(interface.declaration.clone())
    {
        return None;
    }
    let instantiated = |ty: &AbiType| {
        let interface = interface.to_checked_type();
        let substitution = trait_abi
            .generic_params
            .iter()
            .map(|parameter| kagari_hir::types::GenericParameterType {
                owner: parameter.owner.clone(),
                position: parameter.position,
                name: String::new(),
            })
            .zip(interface.arguments.iter().cloned())
            .collect();
        let ty = ty
            .to_checked_type()
            .instantiate(&substitution)
            .with_associated_types(&interface);
        ty.is_concrete()
            .then(|| super::ValueType::from_type_id(&ty))
    };
    let mut params = vec![super::ValueType::HeapObject];
    params.extend(
        method
            .params
            .iter()
            .skip(1)
            .map(|param| instantiated(&param.ty))
            .collect::<Option<Vec<_>>>()?,
    );
    Some((params, instantiated(&method.return_type)?))
}

/// Executable contract for a private trait absent from the public ABI.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TraitContract {
    pub declaration: kagari_common::identity::DefinitionId,
    pub abi: TraitAbi,
}

/// Concrete executable identity; diagnostic function names are not binding keys.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ConcreteFunctionIdentity {
    pub declaration: kagari_common::identity::DefinitionId,
    #[serde(deserialize_with = "crate::decode_limits::nested")]
    pub arguments: Vec<AbiType>,
}

impl ConcreteFunctionIdentity {
    pub(crate) fn from_ir(instance: &super::function::FunctionInstance) -> Self {
        Self {
            declaration: instance.declaration.clone(),
            arguments: instance
                .arguments
                .iter()
                .map(AbiType::from_checked_type)
                .collect(),
        }
    }
}

mod wire;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GenericParameterAbi {
    pub owner: kagari_common::identity::DefinitionId,
    pub position: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GenericBoundAbi {
    pub ty: AbiType,
    #[serde(deserialize_with = "crate::decode_limits::nested")]
    pub constraints: Vec<ConstraintAbi>,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum ConstraintAbi {
    Standard(kagari_hir::builtin::surface::StandardTypeConstraint),
    Trait(NominalAbiType),
}

pub type PublicAbiItemBuffer = Vec<PublicAbiItem>;

pub(crate) mod verify;
