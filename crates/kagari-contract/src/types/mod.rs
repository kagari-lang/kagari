use crate::representation::builtin_representation;
pub mod access;
pub mod applications;
pub mod identity;
pub mod inheritance;
pub mod matching;
pub mod native;
pub mod proofs;
pub mod substitution;
pub mod verify;
mod wire;

use bincode::{DefaultOptions, Options};
use kagari_common::{
    cancellation::CancellationToken,
    collection::CollectionAccess,
    host_interface::value_type::HostValueType,
    identity::{
        DefinitionKind, DefinitionPath, ModuleIdentity, reference::DefinitionReference,
        table::DefinitionTable,
    },
    range::RangeKind,
};
use {
    crate::{
        callable::{CallableImplementation, MethodPolicy, interface::InterfaceCallContract},
        language::Protocol,
        native_import::callables::NativeCallableRequirement,
        scalar::BuiltinType,
        standard::surface::{StandardEnum as StandardEnumKind, StandardTypeConstraint},
        types::{
            native::{NativeStorageLayout, NativeTypeConstructor},
            substitution::TypeSubstitution,
        },
    },
    kagari_abi::representation::ValueType,
};

use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, fmt::Write};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(bound(
    serialize = "I: DefinitionReference + serde::Serialize",
    deserialize = "I: DefinitionReference + serde::Deserialize<'de>"
))]
pub struct ModuleContract<I = DefinitionPath> {
    #[serde(deserialize_with = "crate::decode_limits::table")]
    pub native_declarations: Vec<NativeDeclaration<I>>,
    #[serde(deserialize_with = "crate::decode_limits::table")]
    pub public_items: Vec<PublicItem<I>>,
    #[serde(deserialize_with = "crate::decode_limits::nested")]
    pub trait_contracts: Vec<TraitContract<I>>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(bound(
    serialize = "I: DefinitionReference + serde::Serialize",
    deserialize = "I: DefinitionReference + serde::Deserialize<'de>"
))]
pub enum PublicItem<I = DefinitionPath> {
    Function(FnDecl<I>),
    Const(ConstDef<I>),
    Type(TypeDef<I>),
    Trait(TraitDef<I>),
    InterfaceTable(Box<InterfaceTable<I>>),
}

impl PublicItem {
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
#[serde(bound(
    serialize = "I: DefinitionReference + serde::Serialize",
    deserialize = "I: DefinitionReference + serde::Deserialize<'de>"
))]
pub struct FnDecl<I = DefinitionPath> {
    pub method_policy: MethodPolicy,
    pub name: String,
    pub implementation: CallableImplementation<I>,
    #[serde(deserialize_with = "crate::decode_limits::nested")]
    pub generic_params: Vec<GenericParam<I>>,
    #[serde(deserialize_with = "crate::decode_limits::nested")]
    pub bounds: Vec<GenericBound<I>>,
    #[serde(deserialize_with = "crate::decode_limits::nested")]
    pub params: Vec<Param<I>>,
    pub return_type: Ty<I>,
}

/// Declaration contract for native entrypoints, including inherent and private
/// methods absent from the public ABI. The function uses the same checked model.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(bound(
    serialize = "I: DefinitionReference + serde::Serialize",
    deserialize = "I: DefinitionReference + serde::Deserialize<'de>"
))]
pub struct NativeDeclaration<I = DefinitionPath> {
    /// Concrete value produced by Rust before a checked interface-result adapter.
    pub concrete_result: Option<Ty<I>>,
    pub declaration: I,
    pub function: FnDecl<I>,
    #[serde(deserialize_with = "crate::decode_limits::nested")]
    pub callable_requirements: Vec<NativeCallableRequirement<I>>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(bound(
    serialize = "I: DefinitionReference + serde::Serialize",
    deserialize = "I: DefinitionReference + serde::Deserialize<'de>"
))]
pub struct Param<I = DefinitionPath> {
    pub name: String,
    pub ty: Ty<I>,
    pub mutable: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(bound(
    serialize = "I: DefinitionReference + serde::Serialize",
    deserialize = "I: DefinitionReference + serde::Deserialize<'de>"
))]
pub struct ConstDef<I = DefinitionPath> {
    pub name: String,
    pub ty: Ty<I>,
    pub value: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(bound(
    serialize = "I: DefinitionReference + serde::Serialize",
    deserialize = "I: DefinitionReference + serde::Deserialize<'de>"
))]
pub struct TypeDef<I = DefinitionPath> {
    pub name: String,
    pub kind: TypeDefKind,
    #[serde(deserialize_with = "crate::decode_limits::nested")]
    pub generic_params: Vec<GenericParam<I>>,
    #[serde(deserialize_with = "crate::decode_limits::nested")]
    pub bounds: Vec<GenericBound<I>>,
    #[serde(deserialize_with = "crate::decode_limits::nested")]
    pub fields: Vec<FieldDef<I>>,
    #[serde(deserialize_with = "crate::decode_limits::nested")]
    pub variants: Vec<VariantDef<I>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TypeDefKind {
    Struct,
    Enum,
    Native(NativeTypeConstructor),
    NativeStorage(NativeStorageLayout),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(bound(
    serialize = "I: DefinitionReference + serde::Serialize",
    deserialize = "I: DefinitionReference + serde::Deserialize<'de>"
))]
pub struct FieldDef<I = DefinitionPath> {
    pub name: String,
    pub ty: Ty<I>,
    pub mutable: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(bound(
    serialize = "I: DefinitionReference + serde::Serialize",
    deserialize = "I: DefinitionReference + serde::Deserialize<'de>"
))]
pub struct VariantDef<I = DefinitionPath> {
    pub name: String,
    #[serde(deserialize_with = "crate::decode_limits::nested")]
    pub payload: Vec<Ty<I>>,
}

/// Semantic ABI types preserve nominal identity and container arguments, whereas
/// ValueType describes only the representation used by instruction operands.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(bound(
    serialize = "I: DefinitionReference + Serialize",
    deserialize = "I: DefinitionReference + Deserialize<'de>"
))]
pub struct NominalTy<I = DefinitionPath> {
    pub declaration: I,
    #[serde(deserialize_with = "crate::decode_limits::nested")]
    pub arguments: Vec<Ty<I>>,
    #[serde(deserialize_with = "crate::decode_limits::map")]
    pub associated_types: BTreeMap<I, Ty<I>>,
}

impl<I: DefinitionReference> NominalTy<I> {
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
pub enum Ty<I = DefinitionPath> {
    Projection {
        arguments: Vec<Ty<I>>,
        receiver: Box<Ty<I>>,
        interface: Box<NominalTy<I>>,
        member: I,
    },
    Host(I),
    /// Receiver template in a trait signature, never an executable value layout.
    SelfType(I),
    /// Valid only in a declaration template, never in an executable layout.
    Parameter {
        owner: I,
        position: usize,
    },
    Builtin(BuiltinType),
    Tuple(Vec<Ty<I>>),
    Function {
        params: Vec<Ty<I>>,
        result: Box<Ty<I>>,
    },
    Iter(Box<Ty<I>>),
    Range(Box<Ty<I>>, RangeKind),
    Array(Box<Ty<I>>, CollectionAccess),
    Map {
        key: Box<Ty<I>>,
        value: Box<Ty<I>>,
        access: CollectionAccess,
    },
    Set(Box<Ty<I>>, CollectionAccess),
    Struct(NominalTy<I>),
    /// A nominal script-heap object backed by a registered traced Rust payload.
    NativeObject(NominalTy<I>),
    Enum(NominalTy<I>),
    Trait(NominalTy<I>),
    StandardEnum {
        kind: StandardEnumKind,
        args: Vec<Ty<I>>,
    },
}

impl<I: DefinitionReference> Ty<I> {
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

    pub fn representation(&self) -> ValueType {
        match self {
            Self::Host(_) => ValueType::HostHandle,
            Self::Builtin(ty) => builtin_representation(*ty),
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
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(bound(
    serialize = "I: DefinitionReference + serde::Serialize",
    deserialize = "I: DefinitionReference + serde::Deserialize<'de>"
))]
pub struct TraitDef<I = DefinitionPath> {
    #[serde(deserialize_with = "crate::decode_limits::nested")]
    pub associated_consts: Vec<AssociatedConstDef<I>>,
    pub name: String,
    #[serde(deserialize_with = "crate::decode_limits::nested")]
    pub supertraits: Vec<NominalTy<I>>,
    #[serde(deserialize_with = "crate::decode_limits::nested")]
    pub generic_params: Vec<GenericParam<I>>,
    #[serde(deserialize_with = "crate::decode_limits::nested")]
    pub bounds: Vec<GenericBound<I>>,
    #[serde(deserialize_with = "crate::decode_limits::nested")]
    pub methods: Vec<FnDecl<I>>,
    #[serde(deserialize_with = "crate::decode_limits::nested")]
    pub associated_types: Vec<AssociatedTypeDef<I>>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(bound(
    serialize = "I: DefinitionReference + serde::Serialize",
    deserialize = "I: DefinitionReference + serde::Deserialize<'de>"
))]
pub struct AssociatedConstDef<I = DefinitionPath> {
    pub declaration: I,
    pub ty: Ty<I>,
    pub default_value: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(bound(
    serialize = "I: DefinitionReference + serde::Serialize",
    deserialize = "I: DefinitionReference + serde::Deserialize<'de>"
))]
pub struct AssociatedTypeDef<I = DefinitionPath> {
    #[serde(deserialize_with = "crate::decode_limits::nested")]
    pub generic_params: Vec<GenericParam<I>>,
    #[serde(deserialize_with = "crate::decode_limits::nested")]
    pub parameter_bounds: Vec<GenericBound<I>>,
    pub declaration: I,
    #[serde(deserialize_with = "crate::decode_limits::nested")]
    pub bounds: Vec<Constraint<I>>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(bound(
    serialize = "I: DefinitionReference + serde::Serialize",
    deserialize = "I: DefinitionReference + serde::Deserialize<'de>"
))]
pub struct AssociatedTypeFamily<I = DefinitionPath> {
    pub declaration: I,
    #[serde(deserialize_with = "crate::decode_limits::nested")]
    pub generic_params: Vec<GenericParam<I>>,
    #[serde(deserialize_with = "crate::decode_limits::nested")]
    pub bounds: Vec<GenericBound<I>>,
    pub value: Ty<I>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(bound(
    serialize = "I: DefinitionReference + serde::Serialize",
    deserialize = "I: DefinitionReference + serde::Deserialize<'de>"
))]
pub struct InterfaceTable<I = DefinitionPath> {
    #[serde(deserialize_with = "crate::decode_limits::nested")]
    pub associated_type_families: Vec<AssociatedTypeFamily<I>>,
    #[serde(deserialize_with = "crate::decode_limits::nested")]
    pub associated_consts: Vec<ConstDef<I>>,
    pub host_bridge: bool,
    pub declaration: I,
    pub name: String,
    #[serde(deserialize_with = "crate::decode_limits::nested")]
    pub generic_params: Vec<GenericParam<I>>,
    #[serde(deserialize_with = "crate::decode_limits::nested")]
    pub bounds: Vec<GenericBound<I>>,
    pub trait_type: Ty<I>,
    pub for_type: Ty<I>,
    #[serde(deserialize_with = "crate::decode_limits::nested")]
    pub methods: Vec<FnDecl<I>>,
}

impl<I: DefinitionReference> InterfaceTable<I> {
    /// Substitute a selected impl's concrete arguments into its call contract.
    /// Method-owned generics remain scoped templates until their application is
    /// selected. Associated output projections retain their substituted receivers
    /// until the linked proof catalog resolves them. Implementation bounds retain
    /// their concrete substitutions for downstream method applications. The verifier proves template
    /// validity, bounds and method slots.
    pub fn instantiate(&self, arguments: &[Ty<I>]) -> Option<Self> {
        self.instantiate_in(arguments, &[])
    }

    /// Apply a selected implementation inside a checked shared body or table.
    /// This validates binder scope; linked verification additionally proves
    /// implementation bounds and the selected executable entries.
    pub fn instantiate_in(
        &self,
        arguments: &[Ty<I>],
        parameters: &[GenericParam<I>],
    ) -> Option<Self> {
        self.instantiate_scoped(arguments, parameters, None)
    }

    pub fn instantiate_scoped(
        &self,
        arguments: &[Ty<I>],
        parameters: &[GenericParam<I>],
        table: Option<&DefinitionTable>,
    ) -> Option<Self> {
        if arguments.len() != self.generic_params.len()
            || !verify::types_in_scope_in(
                arguments,
                parameters,
                &CancellationToken::default(),
                table,
            )
        {
            return None;
        }
        let cancel = CancellationToken::default();
        let mut substitution = TypeSubstitution::default();
        for (parameter, argument) in self.generic_params.iter().zip(arguments) {
            substitution.bind(&parameter.owner, parameter.position, argument);
        }
        let apply = |ty: &Ty<I>| substitution.apply(ty, &cancel).ok();
        let methods = self
            .methods
            .iter()
            .map(|method| {
                Some(FnDecl {
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
                            Some(Param {
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
                    Some(AssociatedTypeFamily {
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
    public_items: &[PublicItem],
    trait_contracts: &[TraitContract],
    interface: &NominalTy,
    slot: usize,
) -> Option<(Vec<ValueType>, ValueType)> {
    let (params, result) =
        interface_method_semantics(owner, public_items, trait_contracts, interface, slot)?;
    Some((
        params.iter().map(Ty::representation).collect(),
        result.representation(),
    ))
}

pub fn interface_method_semantics(
    owner: &ModuleIdentity,
    public_items: &[PublicItem],
    trait_contracts: &[TraitContract],
    interface: &NominalTy,
    slot: usize,
) -> Option<(Vec<Ty>, Ty)> {
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
        .all(Ty::is_concrete)
    {
        return None;
    }
    Some((signature.params, signature.result))
}

/// Executable contract for a private trait absent from the public ABI.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(bound(
    serialize = "I: DefinitionReference + serde::Serialize",
    deserialize = "I: DefinitionReference + serde::Deserialize<'de>"
))]
pub struct TraitContract<I = DefinitionPath> {
    pub declaration: I,
    pub abi: TraitDef<I>,
}

/// Concrete executable identity; diagnostic function names are not binding keys.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(bound(
    serialize = "I: DefinitionReference + serde::Serialize",
    deserialize = "I: DefinitionReference + serde::Deserialize<'de>"
))]
pub struct ConcreteFunctionIdentity<I = DefinitionPath> {
    pub declaration: I,
    #[serde(deserialize_with = "crate::decode_limits::nested")]
    pub arguments: Vec<Ty<I>>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GenericParam<I = DefinitionPath> {
    pub owner: I,
    pub position: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(bound(
    serialize = "I: DefinitionReference + Serialize",
    deserialize = "I: DefinitionReference + Deserialize<'de>"
))]
pub struct GenericBound<I = DefinitionPath> {
    pub ty: Ty<I>,
    #[serde(deserialize_with = "crate::decode_limits::nested")]
    pub constraints: Vec<Constraint<I>>,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(bound(
    serialize = "I: DefinitionReference + Serialize",
    deserialize = "I: DefinitionReference + Deserialize<'de>"
))]
pub enum Constraint<I = DefinitionPath> {
    Standard(StandardTypeConstraint),
    Trait(NominalTy<I>),
}

/// Resolve a trait only within its defining executable module. Standard traits
/// follow the same carried-contract path; a well-known ID is not a declaration.
pub fn trait_contract<'a>(
    owner: &ModuleIdentity,
    items: &'a [PublicItem],
    private: &'a [TraitContract],
    id: &DefinitionPath,
) -> Option<&'a TraitDef> {
    trait_contract_in(None, owner, items, private, id)
}

pub fn native_storage_contract<'a>(
    owner: &ModuleIdentity,
    items: &'a [PublicItem],
    id: &DefinitionPath,
) -> Option<&'a TypeDef> {
    native_storage_contract_in(None, owner, items, id)
}

pub fn is_collection_interface(id: &DefinitionPath) -> bool {
    is_collection_interface_in(id, None)
}

pub fn trait_contract_in<'a, I: DefinitionReference>(
    table: Option<&DefinitionTable>,
    owner: &ModuleIdentity,
    public_items: &'a [PublicItem<I>],
    private: &'a [TraitContract<I>],
    id: &I,
) -> Option<&'a TraitDef<I>> {
    let view = id.describe(table).ok()?;
    let part = view.last()?;
    if view.module() != owner
        || view.segments().count() != 1
        || part.kind != DefinitionKind::Trait
        || part.occurrence != 0
    {
        return None;
    }
    private
        .iter()
        .find(|record| record.declaration == *id)
        .map(|record| &record.abi)
        .or_else(|| {
            public_items.iter().find_map(|item| match item {
                PublicItem::Trait(record) if record.name == part.name => Some(record),
                _ => None,
            })
        })
}

/// Whether a canonical standard interface uses collection identity semantics.
pub fn is_collection_interface_in<I: DefinitionReference>(
    id: &I,
    table: Option<&DefinitionTable>,
) -> bool {
    Protocol::from_reference(id, table).is_some_and(Protocol::collection)
}

/// Find a declared native storage type by its exact owning identity.
pub fn native_storage_contract_in<'a, I: DefinitionReference>(
    table: Option<&DefinitionTable>,
    owner: &ModuleIdentity,
    items: &'a [PublicItem<I>],
    id: &I,
) -> Option<&'a TypeDef<I>> {
    let view = id.describe(table).ok()?;
    let part = view.last()?;
    if view.module() != owner
        || view.segments().count() != 1
        || part.kind != DefinitionKind::AssociatedType
        || part.occurrence != 0
    {
        return None;
    }
    items.iter().find_map(|item| match item {
        PublicItem::Type(record)
            if record.name == part.name && matches!(record.kind, TypeDefKind::NativeStorage(_)) =>
        {
            Some(record)
        }
        _ => None,
    })
}

impl<I: DefinitionReference> Ty<I> {
    pub fn from_host_type(ty: &HostValueType<I>) -> Self {
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

    pub fn instantiate(&self, owner: &I, arguments: &[Ty<I>]) -> Option<Self> {
        let result = TypeSubstitution::for_owner(owner, arguments)
            .apply(self, &CancellationToken::default())
            .ok()?;
        result.is_concrete().then_some(result)
    }
}

impl<I> Default for ModuleContract<I> {
    fn default() -> Self {
        Self {
            native_declarations: Default::default(),
            public_items: Default::default(),
            trait_contracts: Default::default(),
        }
    }
}

mod mapping;
