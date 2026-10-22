use crate::{
    callable::interface::InterfaceCallContract, representation::semantic_representation,
    statics::StaticContract, types::inherent::InherentTable,
};
use bincode::{DefaultOptions, Options};
use kagari_abi::representation::ValueType;
use kagari_common::{
    cancellation::CancellationToken,
    identity::{
        DefinitionKind, ModuleIdentity,
        table::{DefinitionId, DefinitionTable, DefinitionTableError},
    },
};
use kagari_types::{
    declaration::{
        AssociatedTypeFamily, ConstDef, FnDecl, NativeDeclaration, Param, TraitDef, TypeDef,
        verify::types_in_scope,
    },
    ty::{GenericBound, GenericParam, NominalTy, Ty, substitution::TypeSubstitution},
};
use serde::{Deserialize, Serialize};
use std::fmt::Write;

pub mod applications;
pub mod identity;
pub mod inherent;
pub mod matching;
pub mod proofs;
pub mod verify;

#[doc(hidden)]
pub mod wire;

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ModuleContract {
    pub native_declarations: Vec<NativeDeclaration>,
    pub public_items: Vec<PublicItem>,
    /// Exact foreign source requirements; these do not define or export a trait.
    pub trait_references: Vec<TraitContract>,
    pub trait_contracts: Vec<TraitContract>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PublicItem {
    Function(FnDecl),
    Const(ConstDef),
    Static(StaticContract),
    Type(TypeDef),
    Trait(TraitDef),
    InterfaceTable(Box<InterfaceTable>),
    InherentTable(Box<InherentTable>),
}

impl PublicItem {
    pub fn name(&self) -> &str {
        match self {
            Self::Function(item) => &item.name,
            Self::Const(item) => &item.name,
            Self::Static(item) => &item.name,
            Self::Type(item) => &item.name,
            Self::Trait(item) => &item.name,
            Self::InterfaceTable(item) => &item.name,
            Self::InherentTable(item) => &item.name,
        }
    }

    pub fn category(&self) -> &'static str {
        match self {
            Self::Function(_) => "function",
            Self::Const(_) => "const",
            Self::Static(_) => "static",
            Self::Type(_) => "type",
            Self::Trait(_) => "trait",
            Self::InterfaceTable(_) => "interface_table",
            Self::InherentTable(_) => "inherent_table",
        }
    }
}

impl PublicItem {
    pub fn fingerprint_name(
        &self,
        definitions: &DefinitionTable,
    ) -> Result<String, DefinitionTableError> {
        let declaration = match self {
            Self::InterfaceTable(table) => Some(&table.declaration),
            Self::InherentTable(table) => Some(&table.declaration),
            _ => None,
        };
        if let Some(declaration) = declaration {
            let encoding = definitions.encode([*declaration])?;
            let encoded = DefaultOptions::new()
                .with_fixint_encoding()
                .with_little_endian()
                .serialize(&(encoding.reference(*declaration)?, encoding.table))
                .expect("validated interface declaration identity");
            let mut name = format!("{}:", self.category());
            for byte in encoded {
                write!(name, "{byte:02x}").expect("writing to a String cannot fail");
            }
            Ok(name)
        } else {
            Ok(format!("{}:{}", self.category(), self.name()))
        }
    }
}

/// A dispatch bridge retains existing evidence rather than declaring a new impl.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum InterfaceBridge {
    Intrinsic,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InterfaceTable {
    pub associated_type_families: Vec<AssociatedTypeFamily>,
    pub associated_consts: Vec<ConstDef>,
    pub bridge: Option<InterfaceBridge>,
    pub declaration: DefinitionId,
    pub name: String,
    pub generic_params: Vec<GenericParam>,
    pub bounds: Vec<GenericBound>,
    pub trait_type: Ty,
    pub for_type: Ty,
    pub methods: Vec<FnDecl>,
}

impl InterfaceTable {
    /// Substitute a selected impl's concrete arguments into its call contract.
    /// Method-owned generics remain scoped templates until their application is
    /// selected. Associated output projections retain their substituted receivers
    /// until the linked proof catalog resolves them. Implementation bounds retain
    /// their concrete substitutions for downstream method applications. The verifier proves template
    /// validity, bounds and method slots.
    pub fn instantiate(
        &self,
        arguments: &[Ty],
        parameters: &[GenericParam],
        table: &DefinitionTable,
    ) -> Option<Self> {
        if arguments.len() != self.generic_params.len()
            || !types_in_scope(arguments, parameters, &CancellationToken::default(), table)
        {
            return None;
        }
        let cancel = CancellationToken::default();
        let mut substitution = TypeSubstitution::default();
        for (parameter, argument) in self.generic_params.iter().zip(arguments) {
            substitution.bind(&parameter.owner, parameter.position, argument);
        }
        let apply = |ty: &Ty| substitution.apply(ty, &cancel).ok();
        let methods = self
            .methods
            .iter()
            .map(|method| {
                Some(FnDecl {
                    host_policy: method.host_policy.clone(),
                    method_policy: method.method_policy,
                    name: method.name.clone(),
                    implementation: method.implementation.apply(&substitution, &cancel).ok()?,
                    generic_params: method
                        .generic_params
                        .iter()
                        .filter(|parameter| !self.generic_params.contains(parameter))
                        .cloned()
                        .collect(),
                    bounds: substitution
                        .apply_bounds(&method.bounds, table, &cancel)
                        .ok()?,
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
                        declaration: family.declaration,
                        generic_params: family.generic_params.clone(),
                        bounds: substitution
                            .apply_bounds(&family.bounds, table, &cancel)
                            .ok()?,
                        value: substitution.apply(&family.value, &cancel).ok()?,
                    })
                })
                .collect::<Option<_>>()?,
            associated_consts: self.associated_consts.clone(),
            bridge: self.bridge,
            declaration: self.declaration,
            name: self.name.clone(),
            generic_params: Vec::new(),
            bounds: substitution
                .apply_bounds(&self.bounds, table, &cancel)
                .ok()?,
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
    definitions: &DefinitionTable,
) -> Option<(Vec<ValueType>, ValueType)> {
    let (params, result) = interface_method_semantics(
        owner,
        public_items,
        trait_contracts,
        interface,
        slot,
        definitions,
    )?;
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
    definitions: &DefinitionTable,
) -> Option<(Vec<Ty>, Ty)> {
    if definitions
        .resolve(interface.declaration)
        .map(|view| view.module())
        != Ok(owner)
    {
        return None;
    }
    let contract = trait_contract(
        owner,
        public_items,
        trait_contracts,
        &interface.declaration,
        definitions,
    )?;
    let call = InterfaceCallContract {
        constructions: vec![],
        normalizations: vec![],
        receiver: None,
        operations: vec![],
        interface: interface.clone(),
        method_slot: u32::try_from(slot).ok()?,
        arguments: vec![],
    };
    let signature = call
        .signature(contract, &CancellationToken::default(), definitions)
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

/// Scoped common trait facts. The containing definition/reference list determines
/// ownership; a reference never supplies the defining trait or script defaults.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TraitContract {
    pub declaration: DefinitionId,
    pub abi: TraitDef,
}

/// Concrete executable identity; diagnostic function names are not binding keys.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ConcreteFunctionIdentity {
    pub declaration: DefinitionId,
    pub arguments: Vec<Ty>,
}

/// Resolve a trait only within its defining executable module. Standard traits
/// follow the same carried-contract path; a well-known ID is not a declaration.
pub fn trait_contract<'a>(
    owner: &ModuleIdentity,
    public_items: &'a [PublicItem],
    private: &'a [TraitContract],
    id: &DefinitionId,
    definitions: &DefinitionTable,
) -> Option<&'a TraitDef> {
    let view = definitions.resolve(*id).ok()?;
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

/// Find a declared nominal type by its exact owning identity.
pub fn type_contract<'a>(
    owner: &ModuleIdentity,
    items: &'a [PublicItem],
    id: &DefinitionId,
    definitions: &DefinitionTable,
) -> Option<&'a TypeDef> {
    let view = definitions.resolve(*id).ok()?;
    let part = view.last()?;
    if view.module() != owner || view.segments().count() != 1 || part.occurrence != 0 {
        return None;
    }
    items.iter().find_map(|item| match item {
        PublicItem::Type(record)
            if record.name == part.name && record.kind.definition_kind() == part.kind =>
        {
            Some(record)
        }
        _ => None,
    })
}

mod mapping;

#[cfg(test)]
mod native_tests;

#[cfg(test)]
mod inheritance_tests;

#[cfg(test)]
mod tests;
