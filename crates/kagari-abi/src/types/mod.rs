pub mod access;
pub mod applications;
pub mod inheritance;
pub mod matching;
pub mod native;
pub mod proofs;
pub mod substitution;
pub mod verify;
mod wire;

use crate::{
    callable::{CallableImplementation, MethodPolicy, interface::InterfaceCallContract},
    language::Protocol,
    native_import::callables::NativeCallableRequirement,
    representation::ValueType,
    scalar::BuiltinType,
    standard::surface::{StandardEnum as StandardEnumKind, StandardTypeConstraint},
    types::{
        native::{NativeStorageLayout, NativeTypeConstructor},
        substitution::TypeSubstitution,
    },
};
use bincode::{DefaultOptions, Options};
use kagari_common::{
    cancellation::CancellationToken,
    collection::CollectionAccess,
    host_interface::value_type::HostValueType,
    identity::{DefinitionId, DefinitionKind, ModuleIdentity},
    range::RangeKind,
};

use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, fmt::Write};

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModuleAbi {
    #[serde(deserialize_with = "crate::decode_limits::table")]
    pub native_declarations: Vec<NativeDeclaration>,
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
    pub method_policy: MethodPolicy,
    pub name: String,
    pub implementation: CallableImplementation,
    #[serde(deserialize_with = "crate::decode_limits::nested")]
    pub generic_params: Vec<GenericParameterAbi>,
    #[serde(deserialize_with = "crate::decode_limits::nested")]
    pub bounds: Vec<GenericBoundAbi>,
    #[serde(deserialize_with = "crate::decode_limits::nested")]
    pub params: Vec<ParameterAbi>,
    pub return_type: AbiType,
}

/// Declaration contract for native entrypoints, including inherent and private
/// methods absent from the public ABI. The function uses the same checked model.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NativeDeclaration {
    /// Concrete value produced by Rust before a checked interface-result adapter.
    pub concrete_result: Option<AbiType>,
    pub declaration: DefinitionId,
    pub function: FunctionAbi,
    #[serde(deserialize_with = "crate::decode_limits::nested")]
    pub callable_requirements: Vec<NativeCallableRequirement>,
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
    Native(NativeTypeConstructor),
    NativeStorage(NativeStorageLayout),
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

impl NominalAbiType {
    /// A required view can leave outputs unspecified; specified outputs remain
    /// invariant and must equal the concrete implementation's checked outputs.
    pub fn satisfies(&self, required: &Self) -> bool {
        self.declaration == required.declaration
            && self.arguments == required.arguments
            && required
                .associated_types
                .iter()
                .all(|(member, ty)| self.associated_types.get(member) == Some(ty))
    }
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
    /// A nominal script-heap object backed by a registered traced Rust payload.
    NativeObject(NominalAbiType),
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
                Self::Struct(ty) | Self::NativeObject(ty) | Self::Enum(ty) | Self::Trait(ty) => {
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
            Self::Parameter { .. } | Self::Projection { .. } | Self::SelfType(_) => {
                ValueType::Generic
            }
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
                Self::Struct(ty) | Self::NativeObject(ty) | Self::Enum(ty) | Self::Trait(ty) => {
                    pending.extend(&ty.arguments);
                    pending.extend(ty.associated_types.values());
                }
                Self::Host(_) | Self::Builtin(_) => {}
            }
        }
        true
    }

    pub fn instantiate(&self, owner: &DefinitionId, arguments: &[AbiType]) -> Option<Self> {
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
    /// Method-owned generics remain scoped templates until their application is
    /// selected. Associated output projections retain their substituted receivers
    /// until the linked proof catalog resolves them. Implementation bounds retain
    /// their concrete substitutions for downstream method applications. The verifier proves template
    /// validity, bounds and method slots.
    pub fn instantiate(&self, arguments: &[AbiType]) -> Option<Self> {
        self.instantiate_in(arguments, &[])
    }

    /// Apply a selected implementation inside a checked shared body or table.
    /// This validates binder scope; linked verification additionally proves
    /// implementation bounds and the selected executable entries.
    pub fn instantiate_in(
        &self,
        arguments: &[AbiType],
        parameters: &[GenericParameterAbi],
    ) -> Option<Self> {
        if arguments.len() != self.generic_params.len()
            || !verify::types_in_scope(arguments, parameters, &CancellationToken::default())
        {
            return None;
        }
        let cancel = CancellationToken::default();
        let mut substitution = TypeSubstitution::default();
        for (parameter, argument) in self.generic_params.iter().zip(arguments) {
            substitution.bind(&parameter.owner, parameter.position, argument);
        }
        let apply = |ty: &AbiType| substitution.apply(ty, &cancel).ok();
        let methods = self
            .methods
            .iter()
            .map(|method| {
                Some(FunctionAbi {
                    method_policy: method.method_policy,
                    name: method.name.clone(),
                    implementation: method.implementation.apply(&substitution, &cancel).ok()?,
                    generic_params: method
                        .generic_params
                        .iter()
                        .filter(|parameter| !self.generic_params.contains(parameter))
                        .cloned()
                        .collect(),
                    bounds: substitution.apply_bounds(&method.bounds, &cancel).ok()?,
                    params: method
                        .params
                        .iter()
                        .map(|param| {
                            Some(ParameterAbi {
                                name: param.name.clone(),
                                mutable: param.mutable,
                                ty: substitution.apply(&param.ty, &cancel).ok()?,
                            })
                        })
                        .collect::<Option<_>>()?,
                    return_type: substitution.apply(&method.return_type, &cancel).ok()?,
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
            declaration: self.declaration.clone(),
            name: self.name.clone(),
            generic_params: Vec::new(),
            bounds: substitution.apply_bounds(&self.bounds, &cancel).ok()?,
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
    if interface.declaration.module != *owner {
        return None;
    }
    let contract = trait_contract(owner, public_items, trait_contracts, &interface.declaration)?;
    let call = InterfaceCallContract {
        receiver: None,
        operations: vec![],
        interface: interface.clone(),
        method_slot: u32::try_from(slot).ok()?,
        arguments: vec![],
    };
    let signature = call
        .signature(contract, &CancellationToken::default())
        .ok()?;
    if !signature
        .params
        .iter()
        .chain([&signature.result])
        .all(AbiType::is_concrete)
    {
        return None;
    }
    Some((signature.params, signature.result))
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

/// Resolve a trait only within its defining executable module. Standard traits
/// follow the same carried-contract path; a well-known ID is not a declaration.
pub fn trait_contract<'a>(
    owner: &ModuleIdentity,
    public_items: &'a [PublicAbiItem],
    private: &'a [TraitContract],
    id: &DefinitionId,
) -> Option<&'a TraitAbi> {
    if id.module != *owner
        || id.path.len() != 1
        || id.path[0].kind != DefinitionKind::Trait
        || id.path[0].occurrence != 0
    {
        return None;
    }
    private
        .iter()
        .find(|record| record.declaration == *id)
        .map(|record| &record.abi)
        .or_else(|| {
            public_items.iter().find_map(|item| match item {
                PublicAbiItem::Trait(record) if record.name == id.path[0].name => Some(record),
                _ => None,
            })
        })
}

/// Whether a canonical standard interface uses collection identity semantics.
pub fn is_collection_interface(id: &DefinitionId) -> bool {
    Protocol::from_id(id).is_some_and(Protocol::collection)
}

/// Find a declared native storage type by its exact owning identity.
pub fn native_storage_contract<'a>(
    owner: &ModuleIdentity,
    items: &'a [PublicAbiItem],
    id: &DefinitionId,
) -> Option<&'a TypeAbi> {
    if id.module != *owner
        || id.path.len() != 1
        || id.path[0].kind != DefinitionKind::AssociatedType
        || id.path[0].occurrence != 0
    {
        return None;
    }
    items.iter().find_map(|item| match item {
        PublicAbiItem::Type(record)
            if record.name == id.path[0].name
                && matches!(record.kind, TypeAbiKind::NativeStorage(_)) =>
        {
            Some(record)
        }
        _ => None,
    })
}
