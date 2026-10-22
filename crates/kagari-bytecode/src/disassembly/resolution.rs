//! Borrowed executable identity indexes for one immutable disassembly operation.

use crate::program::BytecodeProgram;
use kagari_common::identity::{
    DefinitionKind, ModuleIdentity,
    table::{DefinitionId, DefinitionTable},
};
use kagari_contract::{
    native_import::callables::NativeCallableApplication,
    types::{ConcreteFunctionIdentity, InterfaceTable, PublicItem},
};
use kagari_types::{
    callable::{CallableImplementation, Signature},
    declaration::{NativeDeclaration, TraitDef},
    ty::{NominalTy, Ty},
};
use std::{collections::HashMap, fmt};

pub(super) mod contracts;

#[cfg(test)]
mod tests;

type Slot = (usize, usize);
type InstanceKey<'a> = (DefinitionId, &'a [Ty]);
type NativeKey<'a> = (&'a ConcreteFunctionIdentity, DefinitionId, &'a Signature);

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(super) enum TraitSource {
    PublicItem,
    Contract,
}

/// The slot belongs to the indicated retained declaration table, not an impl table.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(super) struct TraitReference {
    pub owner: usize,
    pub source: TraitSource,
    pub slot: usize,
}

impl fmt::Display for TraitReference {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let table = match self.source {
            TraitSource::PublicItem => "public_item",
            TraitSource::Contract => "trait_contract",
        };
        write!(formatter, "@{}:{table}#{}", self.owner, self.slot)
    }
}

#[derive(Default)]
struct LayoutIndex<'a> {
    local: HashMap<(usize, DefinitionId, &'a [Ty]), usize>,
    first: HashMap<InstanceKey<'a>, Slot>,
}

impl<'a> LayoutIndex<'a> {
    fn insert(
        &mut self,
        owner: usize,
        slot: usize,
        declaration: DefinitionId,
        arguments: &'a [Ty],
    ) {
        self.local
            .entry((owner, declaration, arguments))
            .or_insert(slot);
        self.first
            .entry((declaration, arguments))
            .or_insert((owner, slot));
    }

    fn get(&self, owner: usize, ty: &NominalTy) -> Option<Slot> {
        self.local
            .get(&(owner, ty.declaration, ty.arguments.as_slice()))
            .map(|slot| (owner, *slot))
            .or_else(|| {
                self.first
                    .get(&(ty.declaration, ty.arguments.as_slice()))
                    .copied()
            })
    }
}

/// Indexes borrow stored keys and declarations; they never infer or substitute types.
///
/// Construct once with the program's retained verified definition scope, then share
/// between reference collection and both renderers. Hash maps are lookup-only:
/// emitted order always comes from stored slots or the collector's ordered set.
pub(crate) struct Resolution<'a> {
    pub(super) program: &'a BytecodeProgram,
    definitions: &'a DefinitionTable,
    modules: HashMap<&'a ModuleIdentity, usize>,
    scripts: HashMap<&'a ConcreteFunctionIdentity, Slot>,
    natives: HashMap<NativeKey<'a>, Slot>,
    native_declarations: HashMap<DefinitionId, (usize, &'a NativeDeclaration)>,
    interfaces: HashMap<InstanceKey<'a>, Slot>,
    symbolic_interfaces: HashMap<DefinitionId, Slot>,
    interface_declarations: HashMap<DefinitionId, &'a InterfaceTable>,
    traits: HashMap<(usize, DefinitionId), TraitReference>,
    structures: LayoutIndex<'a>,
    enumerations: LayoutIndex<'a>,
}

impl<'a> Resolution<'a> {
    pub(crate) fn new(program: &'a BytecodeProgram, definitions: &'a DefinitionTable) -> Self {
        let mut result = Self {
            program,
            definitions,
            modules: HashMap::new(),
            scripts: HashMap::new(),
            natives: HashMap::new(),
            native_declarations: HashMap::new(),
            interfaces: HashMap::new(),
            symbolic_interfaces: HashMap::new(),
            interface_declarations: HashMap::new(),
            traits: HashMap::new(),
            structures: LayoutIndex::default(),
            enumerations: LayoutIndex::default(),
        };
        for (owner, module) in program.modules.iter().enumerate() {
            result.modules.insert(&module.identity, owner);
            for declaration in &module.native_declarations {
                result
                    .native_declarations
                    .insert(declaration.declaration, (owner, declaration));
            }
            // The verifier's trait_contract lookup prefers a retained private contract.
            for (slot, declaration) in module.trait_contracts.iter().enumerate() {
                result.traits.insert(
                    (owner, declaration.declaration),
                    TraitReference {
                        owner,
                        source: TraitSource::Contract,
                        slot,
                    },
                );
            }
            for (slot, item) in module.public_items.iter().enumerate() {
                match item {
                    PublicItem::InterfaceTable(table) => {
                        result
                            .interface_declarations
                            .insert(table.declaration, table);
                    }
                    PublicItem::Trait(record) => {
                        let root = definitions
                            .lookup_root(&module.identity)
                            .expect("verified module identity");
                        let declaration = definitions
                            .lookup_child(root, DefinitionKind::Trait, &record.name, 0)
                            .expect("verified public trait identity");
                        result
                            .traits
                            .entry((owner, declaration))
                            .or_insert(TraitReference {
                                owner,
                                source: TraitSource::PublicItem,
                                slot,
                            });
                    }
                    _ => {}
                }
            }
        }
        for (owner, module) in program.modules.iter().enumerate() {
            for (slot, function) in module.functions.iter().enumerate() {
                if let Some(identity) = &function.identity {
                    result.scripts.entry(identity).or_insert((owner, slot));
                }
            }
            for (slot, import) in module.native_imports.iter().enumerate() {
                if result
                    .native_declarations
                    .get(&import.instance.declaration)
                    .is_some_and(|(declaration_owner, _)| *declaration_owner == owner)
                {
                    result
                        .natives
                        .entry((&import.instance, import.binding, &import.signature))
                        .or_insert((owner, slot));
                }
            }
            for (slot, table) in module.interface_tables.iter().enumerate() {
                result
                    .interfaces
                    .entry((table.declaration, table.arguments.as_slice()))
                    .or_insert((owner, slot));
                let template = result.interface_declaration(table.declaration);
                if template.generic_params.len() == table.arguments.len()
                    && template
                        .generic_params
                        .iter()
                        .zip(&table.arguments)
                        .all(|(parameter, argument)| parameter.as_type() == *argument)
                {
                    result
                        .symbolic_interfaces
                        .entry(table.declaration)
                        .or_insert((owner, slot));
                }
            }
            for (slot, layout) in module.structures.iter().enumerate() {
                result
                    .structures
                    .insert(owner, slot, layout.declaration, &layout.arguments);
            }
            for (slot, layout) in module.enumerations.iter().enumerate() {
                result
                    .enumerations
                    .insert(owner, slot, layout.declaration, &layout.arguments);
            }
        }
        result
    }

    pub(super) fn module(&self, identity: &ModuleIdentity) -> usize {
        *self
            .modules
            .get(identity)
            .expect("verified module supplier")
    }

    pub(super) fn script(&self, identity: &ConcreteFunctionIdentity) -> Slot {
        self.scripts
            .get(identity)
            .copied()
            .expect("verified selected script entry")
    }

    pub(super) fn native(&self, call: &NativeCallableApplication) -> Option<Slot> {
        match &call.implementation {
            CallableImplementation::Native(binding) => Some(
                *self
                    .natives
                    .get(&(&call.instance, *binding, &call.signature))
                    .expect("verified selected owner-native entry"),
            ),
            CallableImplementation::Script
            | CallableImplementation::Required
            | CallableImplementation::NativeDefault(_) => None,
        }
    }

    pub(super) fn native_declaration(&self, declaration: DefinitionId) -> &'a NativeDeclaration {
        self.native_declarations[&declaration].1
    }

    pub(super) fn interface(&self, identity: &ConcreteFunctionIdentity) -> Slot {
        if identity.arguments.iter().all(Ty::is_concrete) {
            self.interfaces
                .get(&(identity.declaration, identity.arguments.as_slice()))
        } else {
            self.symbolic_interfaces.get(&identity.declaration)
        }
        .copied()
        .expect("verified preselected interface table")
    }

    pub(super) fn interface_declaration(&self, declaration: DefinitionId) -> &'a InterfaceTable {
        self.interface_declarations[&declaration]
    }

    pub(super) fn trait_reference(
        &self,
        owner: usize,
        declaration: DefinitionId,
    ) -> TraitReference {
        self.traits[&(owner, declaration)]
    }

    pub(super) fn trait_declaration(
        &self,
        reference: TraitReference,
    ) -> (DefinitionId, &'a TraitDef) {
        let module = &self.program.modules[reference.owner];
        match reference.source {
            TraitSource::Contract => {
                let contract = &module.trait_contracts[reference.slot];
                (contract.declaration, &contract.abi)
            }
            TraitSource::PublicItem => {
                let PublicItem::Trait(record) = &module.public_items[reference.slot] else {
                    unreachable!("indexed public trait declaration")
                };
                let root = self
                    .definitions
                    .lookup_root(&module.identity)
                    .expect("verified module identity");
                let declaration = self
                    .definitions
                    .lookup_child(root, DefinitionKind::Trait, &record.name, 0)
                    .expect("verified public trait identity");
                (declaration, record)
            }
        }
    }

    pub(super) fn trait_member(&self, reference: TraitReference, slot: usize) -> DefinitionId {
        let (declaration, record) = self.trait_declaration(reference);
        self.definitions
            .lookup_child(
                declaration,
                DefinitionKind::Method,
                &record.methods[slot].name,
                0,
            )
            .expect("verified interface member")
    }

    pub(super) fn structure(&self, owner: usize, ty: &NominalTy) -> Option<Slot> {
        self.structures.get(owner, ty)
    }

    pub(super) fn enumeration(&self, owner: usize, ty: &NominalTy) -> Option<Slot> {
        self.enumerations.get(owner, ty)
    }
}
