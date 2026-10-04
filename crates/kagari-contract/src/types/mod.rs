use crate::{callable::interface::InterfaceCallContract, representation::semantic_representation};
use bincode::{DefaultOptions, Options};
use kagari_abi::representation::ValueType;
use kagari_common::{
    cancellation::CancellationToken,
    identity::{
        DefinitionKind, DefinitionPath, ModuleIdentity, reference::DefinitionReference,
        table::DefinitionTable,
    },
};
use kagari_types::{
    declaration::{
        AssociatedTypeFamily, ConstDef, FnDecl, NativeDeclaration, Param, TraitDef, TypeDef,
        TypeDefKind, verify::types_in_scope_in,
    },
    ty::{GenericBound, GenericParam, NominalTy, Ty, substitution::TypeSubstitution},
};
use serde::{Deserialize, Serialize};
use std::fmt::Write;

pub mod applications;
pub mod matching;
pub mod proofs;
pub mod verify;

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
            || !types_in_scope_in(arguments, parameters, &CancellationToken::default(), table)
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
        params.iter().map(semantic_representation).collect(),
        semantic_representation(&result),
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

#[cfg(test)]
mod native_tests;

#[cfg(test)]
mod inheritance_tests;
