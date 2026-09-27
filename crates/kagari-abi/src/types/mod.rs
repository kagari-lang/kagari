pub mod matching;
pub mod substitution;
pub mod verify;
mod wire;

use crate::representation::ValueType;
use crate::scalar::BuiltinType;
use crate::standard::contracts;
use crate::standard::surface::StandardEnum as StandardEnumKind;
use crate::types::substitution::{TypeSubstitution, resolve_associated_outputs};
use bincode::DefaultOptions;
use bincode::Options;
use kagari_common::cancellation::CancellationToken;
use kagari_common::collection::CollectionAccess;
use kagari_common::host_interface::HostValueType;
use kagari_common::identity::DefinitionId;
use kagari_common::identity::DefinitionKind;
use kagari_common::identity::ModuleIdentity;
use kagari_common::range::RangeKind;

use crate::standard::surface::StandardTypeConstraint;
use crate::standard::traits::StandardTrait;

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fmt::Write;
use std::sync::OnceLock;

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
    InterfaceTable(Box<InterfaceTableAbi>),
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
            let encoded = DefaultOptions::new()
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
    pub declaration: DefinitionId,
    #[serde(deserialize_with = "crate::decode_limits::nested")]
    pub arguments: Vec<AbiType>,
    #[serde(deserialize_with = "crate::decode_limits::map")]
    pub associated_types: BTreeMap<DefinitionId, AbiType>,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum AbiType {
    Projection {
        arguments: Vec<AbiType>,
        receiver: Box<AbiType>,
        interface: Box<NominalAbiType>,
        member: DefinitionId,
    },
    Host(DefinitionId),
    /// Receiver template in a trait signature, never an executable value layout.
    SelfType(DefinitionId),
    /// Valid only in a declaration template, never in an executable layout.
    Parameter {
        owner: DefinitionId,
        position: usize,
    },
    Builtin(BuiltinType),
    Tuple(Vec<AbiType>),
    Function {
        params: Vec<AbiType>,
        result: Box<AbiType>,
    },
    Iter(Box<AbiType>),
    Range(Box<AbiType>, RangeKind),
    Array(Box<AbiType>, CollectionAccess),
    Map {
        key: Box<AbiType>,
        value: Box<AbiType>,
        access: CollectionAccess,
    },
    Set(Box<AbiType>, CollectionAccess),
    Struct(NominalAbiType),
    Enum(NominalAbiType),
    Trait(NominalAbiType),
    StandardEnum {
        kind: StandardEnumKind,
        args: Vec<AbiType>,
    },
}

impl AbiType {
    pub fn contains_projection(&self) -> bool {
        let mut pending = vec![self];
        while let Some(ty) = pending.pop() {
            match ty {
                Self::Projection { .. } => return true,
                Self::Struct(ty) | Self::Enum(ty) | Self::Trait(ty) => {
                    pending.extend(&ty.arguments);
                    pending.extend(ty.associated_types.values());
                }
                Self::Tuple(items) | Self::StandardEnum { args: items, .. } => {
                    pending.extend(items)
                }
                Self::Array(item, _)
                | Self::Set(item, _)
                | Self::Iter(item)
                | Self::Range(item, _) => pending.push(item),
                Self::Map { key, value, .. } => pending.extend([key.as_ref(), value.as_ref()]),
                Self::Function { params, result } => {
                    pending.extend(params);
                    pending.push(result);
                }
                Self::Host(_) | Self::Builtin(_) | Self::SelfType(_) | Self::Parameter { .. } => {}
            }
        }
        false
    }

    pub fn from_host_type(ty: &HostValueType) -> Self {
        match ty {
            HostValueType::Unit => Self::Builtin(BuiltinType::Unit),
            HostValueType::Bool => Self::Builtin(BuiltinType::Bool),
            HostValueType::I32 => Self::Builtin(BuiltinType::I32),
            HostValueType::I64 => Self::Builtin(BuiltinType::I64),
            HostValueType::F32 => Self::Builtin(BuiltinType::F32),
            HostValueType::F64 => Self::Builtin(BuiltinType::F64),
            HostValueType::String => Self::Builtin(BuiltinType::String),
            HostValueType::Opaque(id) => Self::Host(id.clone()),
            HostValueType::Tuple(types) => {
                Self::Tuple(types.iter().map(Self::from_host_type).collect())
            }
            HostValueType::Array(ty, access) => {
                Self::Array(Box::new(Self::from_host_type(ty)), *access)
            }
            HostValueType::Map { key, value, access } => Self::Map {
                key: Box::new(Self::from_host_type(key)),
                value: Box::new(Self::from_host_type(value)),
                access: *access,
            },
            HostValueType::Set(ty, access) => {
                Self::Set(Box::new(Self::from_host_type(ty)), *access)
            }
            HostValueType::Option(ty) => Self::StandardEnum {
                kind: StandardEnumKind::Option,
                args: vec![Self::from_host_type(ty)],
            },
            HostValueType::Result { ok, error } => Self::StandardEnum {
                kind: StandardEnumKind::Result,
                args: vec![Self::from_host_type(ok), Self::from_host_type(error)],
            },
        }
    }

    pub fn representation(&self) -> ValueType {
        match self {
            Self::Host(_) => ValueType::HostHandle,
            Self::Builtin(ty) => ValueType::from_builtin_type(*ty),
            _ => ValueType::HeapObject,
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
                Self::Array(ty, _) | Self::Set(ty, _) | Self::Iter(ty) | Self::Range(ty, _) => {
                    pending.push(ty)
                }
                Self::Map { key, value, .. } => pending.extend([key.as_ref(), value.as_ref()]),
                Self::Struct(ty) | Self::Enum(ty) | Self::Trait(ty) => {
                    pending.extend(&ty.arguments);
                    pending.extend(ty.associated_types.values());
                }
                Self::Host(_) | Self::Builtin(_) => {}
            }
        }
        true
    }

    pub(crate) fn instantiate(&self, owner: &DefinitionId, arguments: &[AbiType]) -> Option<Self> {
        let result = TypeSubstitution::for_owner(owner, arguments)
            .apply(self, &CancellationToken::default())
            .ok()?;
        result.is_concrete().then_some(result)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TraitAbi {
    #[serde(deserialize_with = "crate::decode_limits::nested")]
    pub associated_consts: Vec<AssociatedConstAbi>,
    pub name: String,
    #[serde(deserialize_with = "crate::decode_limits::nested")]
    pub default_methods: Vec<usize>,
    #[serde(deserialize_with = "crate::decode_limits::nested")]
    pub supertraits: Vec<NominalAbiType>,
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
pub struct AssociatedConstAbi {
    pub declaration: DefinitionId,
    pub ty: AbiType,
    pub default_value: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AssociatedTypeAbi {
    #[serde(deserialize_with = "crate::decode_limits::nested")]
    pub generic_params: Vec<GenericParameterAbi>,
    #[serde(deserialize_with = "crate::decode_limits::nested")]
    pub parameter_bounds: Vec<GenericBoundAbi>,
    pub declaration: DefinitionId,
    #[serde(deserialize_with = "crate::decode_limits::nested")]
    pub bounds: Vec<ConstraintAbi>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AssociatedTypeFamilyAbi {
    pub declaration: DefinitionId,
    #[serde(deserialize_with = "crate::decode_limits::nested")]
    pub generic_params: Vec<GenericParameterAbi>,
    #[serde(deserialize_with = "crate::decode_limits::nested")]
    pub bounds: Vec<GenericBoundAbi>,
    pub value: AbiType,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InterfaceTableAbi {
    #[serde(deserialize_with = "crate::decode_limits::nested")]
    pub associated_type_families: Vec<AssociatedTypeFamilyAbi>,
    #[serde(deserialize_with = "crate::decode_limits::nested")]
    pub associated_consts: Vec<ConstAbi>,
    pub host_bridge: bool,
    pub native_bridge: bool,
    pub declaration: DefinitionId,
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
        let cancel = CancellationToken::default();
        let mut substitution = TypeSubstitution::default();
        for (parameter, argument) in self.generic_params.iter().zip(arguments) {
            substitution.bind(&parameter.owner, parameter.position, argument);
        }
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
            associated_type_families: self
                .associated_type_families
                .iter()
                .map(|family| {
                    Some(AssociatedTypeFamilyAbi {
                        declaration: family.declaration.clone(),
                        generic_params: family.generic_params.clone(),
                        bounds: substitution.apply_bounds(&family.bounds, &cancel).ok()?,
                        value: substitution.apply(&family.value, &cancel).ok()?,
                    })
                })
                .collect::<Option<_>>()?,
            associated_consts: self.associated_consts.clone(),
            host_bridge: self.host_bridge,
            native_bridge: self.native_bridge,
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
pub fn interface_method_types(
    owner: &ModuleIdentity,
    public_items: &[PublicAbiItem],
    trait_contracts: &[TraitContract],
    interface: &NominalAbiType,
    slot: usize,
) -> Option<(Vec<ValueType>, ValueType)> {
    let (params, result) =
        interface_method_semantics(owner, public_items, trait_contracts, interface, slot)?;
    Some((
        params.iter().map(AbiType::representation).collect(),
        result.representation(),
    ))
}

pub fn interface_method_semantics(
    owner: &ModuleIdentity,
    public_items: &[PublicAbiItem],
    trait_contracts: &[TraitContract],
    interface: &NominalAbiType,
    slot: usize,
) -> Option<(Vec<AbiType>, AbiType)> {
    let path = &interface.declaration.path;
    if (interface.declaration.module != *owner
        && standard_trait_contract(&interface.declaration).is_none())
        || path.len() != 1
        || path[0].kind != DefinitionKind::Trait
        || path[0].occurrence != 0
        || !interface.arguments.iter().all(AbiType::is_concrete)
    {
        return None;
    }
    let trait_abi = standard_trait_contract(&interface.declaration).or_else(|| {
        trait_contracts
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
            })
    })?;
    if interface.arguments.len() != trait_abi.generic_params.len() {
        return None;
    }
    if !trait_abi.associated_consts.is_empty()
        || trait_abi
            .associated_types
            .iter()
            .any(|member| !member.generic_params.is_empty())
        || interface.associated_types.len() != trait_abi.associated_types.len()
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
    let cancel = CancellationToken::default();
    let mut substitution = TypeSubstitution::default();
    for (parameter, argument) in trait_abi.generic_params.iter().zip(&interface.arguments) {
        substitution.bind(&parameter.owner, parameter.position, argument);
    }
    let instantiated = |ty: &AbiType| {
        let ty = substitution.apply(ty, &cancel).ok()?;
        let ty = resolve_associated_outputs(&ty, interface, &cancel).ok()?;
        ty.is_concrete().then_some(ty)
    };
    let mut params = vec![AbiType::Trait(interface.clone())];
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
    pub declaration: DefinitionId,
    pub abi: TraitAbi,
}

/// Concrete executable identity; diagnostic function names are not binding keys.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ConcreteFunctionIdentity {
    pub declaration: DefinitionId,
    #[serde(deserialize_with = "crate::decode_limits::nested")]
    pub arguments: Vec<AbiType>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GenericParameterAbi {
    pub owner: DefinitionId,
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
    Standard(StandardTypeConstraint),
    Trait(NominalAbiType),
}

pub type PublicAbiItemBuffer = Vec<PublicAbiItem>;

/// Canonical standard contracts are engine-owned, never supplied by an artifact.
pub fn standard_trait_contract(id: &DefinitionId) -> Option<&'static TraitAbi> {
    static CONTRACTS: OnceLock<Vec<TraitAbi>> = OnceLock::new();
    let kind = StandardTrait::from_id(id)?;
    Some(
        &CONTRACTS.get_or_init(|| {
            StandardTrait::ALL
                .into_iter()
                .map(|kind| {
                    contracts::trait_contract(kind).expect("valid generated standard contract")
                })
                .collect()
        })[kind as usize],
    )
}

/// Whether a canonical standard interface uses collection identity semantics.
pub fn is_collection_interface(id: &DefinitionId) -> bool {
    StandardTrait::from_id(id).is_some_and(StandardTrait::collection)
}
