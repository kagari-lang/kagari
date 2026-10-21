//! Bounded linked proofs over validated portable implementation and layout records.
//! This catalog borrows executable contracts; it does not reconstruct source
//! signatures or infer types. Declaration validity remains the module verifier's
//! responsibility, while this layer checks dependency-dependent obligations.

mod callables;
mod composition;
mod construction;
pub mod defaults;
pub mod implementation;
mod normalize;
mod ownership;
mod protocols;
pub mod resources;
mod search;
mod structural;

use crate::{
    layout::EnumLayout,
    types::proofs::{
        implementation::Implementation,
        resources::{ProofModule, Record, ScopedMap, ScopedRecords},
    },
};
use kagari_common::{
    cancellation::CancellationToken,
    identity::{
        DefinitionKind, ModuleIdentity,
        mapping::DefinitionMappingError,
        scoped::DefinitionReferences,
        table::{DefinitionId, DefinitionTable},
    },
};
use kagari_types::{
    declaration::{
        FnDecl, NativeDeclaration, TraitDef,
        verify::{DeclarationValidationError, validate_native_declarations},
    },
    ty::{
        GenericBound, GenericParam, NominalTy, Ty, inheritance,
        substitution::{TypeSubstitution, TypeTransformError},
    },
};
use std::{
    cell::Cell,
    collections::{BTreeMap, HashSet},
    slice,
};

const MAX_IMPLEMENTATIONS: usize = 4096;
const MAX_NATIVE_DECLARATIONS: usize = 4096;
const MAX_CHECKS: usize = 100_000;
const MAX_DEPTH: usize = 64;

pub struct ProofCatalog<'a> {
    definitions: DefinitionTable,
    implementations: ScopedRecords<Implementation<'a>>,
    enumerations: ScopedMap<NominalTy, Vec<&'a Ty>>,
    contracts: ScopedMap<DefinitionId, &'a TraitDef>,
    native_declarations: ScopedMap<DefinitionId, &'a NativeDeclaration>,
}

struct Budget<'a> {
    left: Cell<usize>,
    cancel: &'a CancellationToken,
}

impl<'a> Budget<'a> {
    fn new(cancel: &'a CancellationToken) -> Self {
        Self {
            left: Cell::new(MAX_CHECKS),
            cancel,
        }
    }

    fn step(&self, depth: usize) -> Result<(), TypeTransformError> {
        self.cancel
            .check()
            .map_err(|_| TypeTransformError::Cancelled)?;
        if depth >= MAX_DEPTH || self.left.get() == 0 {
            return Err(TypeTransformError::LimitExceeded);
        }
        self.left.set(self.left.get() - 1);
        Ok(())
    }
}

impl<'a> ProofCatalog<'a> {
    /// Identity scope retained by this proof operation, separate from its proofs.
    pub fn definitions(&self) -> &DefinitionTable {
        &self.definitions
    }

    fn module(&self, id: DefinitionId) -> Result<&ModuleIdentity, TypeTransformError> {
        self.definitions
            .resolve(id)
            .map(|view| view.module())
            .map_err(|_| TypeTransformError::InvalidContract)
    }

    fn associated(
        &self,
        owner: DefinitionId,
        name: &str,
    ) -> Result<DefinitionId, TypeTransformError> {
        self.definitions
            .lookup_child(owner, DefinitionKind::AssociatedType, name, 0)
            .ok_or(TypeTransformError::InvalidContract)
    }

    pub fn trait_contract(&self, declaration: &DefinitionId) -> Option<&'a TraitDef> {
        self.contracts.get(declaration).copied()
    }

    /// Resolve checked native declaration facts for a shared entry application.
    pub fn native_declaration(&self, declaration: DefinitionId) -> Option<&'a NativeDeclaration> {
        self.native_declarations.get(&declaration).copied()
    }

    /// Check linked contracts in an immutable, already registered identity scope.
    ///
    /// Registration must collect implicit method, declared implementation and language
    /// names before taking this snapshot. Missing or foreign references reject;
    /// names alone never establish an implementation or a valid declaration.
    /// Cancellation, proof limits and invalid contracts return an error.
    pub fn new(
        implementations: Vec<Implementation<'a>>,
        enumerations: impl IntoIterator<Item = &'a EnumLayout>,
        contracts: impl IntoIterator<Item = (DefinitionId, &'a TraitDef)>,
        native_declarations: impl IntoIterator<Item = &'a NativeDeclaration>,
        definitions: &DefinitionTable,
        cancel: &CancellationToken,
    ) -> Result<Self, TypeTransformError> {
        let result = Self::prepare(
            vec![ProofModule {
                implementations,
                enumerations: enumerations.into_iter().collect(),
                contracts: contracts.into_iter().collect(),
                native_declarations: native_declarations.into_iter().collect(),
            }],
            definitions,
            cancel,
        )?;
        result.validate_linked(cancel)?;
        Ok(result)
    }

    fn prepare(
        modules: Vec<ProofModule<'a>>,
        definitions: &DefinitionTable,
        cancel: &CancellationToken,
    ) -> Result<Self, TypeTransformError> {
        let mut implementation_inputs = Vec::new();
        let mut enumerations = Vec::new();
        let mut contracts = Vec::new();
        let mut native_declarations = Vec::new();
        for (index, module) in modules.into_iter().enumerate() {
            cancel.check().map_err(|_| TypeTransformError::Cancelled)?;
            implementation_inputs.extend(
                module
                    .implementations
                    .into_iter()
                    .map(|record| (index, record)),
            );
            enumerations.extend(
                module
                    .enumerations
                    .into_iter()
                    .map(|record| (index, record)),
            );
            contracts.extend(module.contracts.into_iter().map(|record| (index, record)));
            native_declarations.extend(
                module
                    .native_declarations
                    .into_iter()
                    .map(|record| (index, record)),
            );
        }
        cancel.check().map_err(|_| TypeTransformError::Cancelled)?;
        let copier = TypeSubstitution::default();
        let mut ids = HashSet::new();
        let mut count = 0;
        let mut implementations = Vec::new();
        for (module, implementation) in implementation_inputs {
            #[cfg(test)]
            preparation_tests::record(cancel);
            cancel.check().map_err(|_| TypeTransformError::Cancelled)?;
            definitions
                .resolve(*implementation.declaration())
                .map_err(|_| TypeTransformError::InvalidContract)?;
            match &implementation {
                Implementation::Interface(record) => {
                    validate_references(*record, definitions, cancel)?
                }
                Implementation::Native { implementation, .. } => {
                    validate_references(*implementation, definitions, cancel)?
                }
            }
            for method in implementation.methods() {
                definitions
                    .lookup_child(
                        *implementation.declaration(),
                        DefinitionKind::Method,
                        &method.name,
                        0,
                    )
                    .ok_or(TypeTransformError::InvalidContract)?;
            }
            if !ids.insert(*implementation.declaration())
                || implementation.is_bridge()
                || implementation.interface().is_none()
            {
                return Err(TypeTransformError::InvalidContract);
            }
            count += 1;
            if count > MAX_IMPLEMENTATIONS {
                return Err(TypeTransformError::LimitExceeded);
            }
            copier.apply_nominal(
                implementation.interface().expect("checked trait source"),
                cancel,
            )?;
            copier.apply(implementation.receiver(), cancel)?;
            copier.apply_bounds(implementation.bounds(), definitions, cancel)?;
            implementations.push(Record::new(implementation, module));
        }
        let mut payloads: BTreeMap<NominalTy, Record<Vec<&'a Ty>>> = BTreeMap::new();
        for (module, layout) in enumerations {
            #[cfg(test)]
            preparation_tests::record(cancel);
            validate_references(layout, definitions, cancel)?;
            let key = NominalTy {
                declaration: layout.declaration,
                arguments: layout.arguments.clone(),
                associated_types: BTreeMap::new(),
            };
            let key = copier.apply_nominal(&key, cancel)?;
            let payload: Vec<_> = layout
                .variants
                .iter()
                .flat_map(|variant| &variant.payload)
                .collect();
            for ty in &payload {
                copier.apply(ty, cancel)?;
            }
            if let Some(old) = payloads.get_mut(&key) {
                if old.value != payload {
                    return Err(TypeTransformError::InvalidContract);
                }
                old.modules.push(module);
            } else {
                payloads.insert(key, Record::new(payload, module));
            }
        }
        let mut declarations: BTreeMap<DefinitionId, Record<&'a TraitDef>> = BTreeMap::new();
        for (module, (id, contract)) in contracts {
            cancel.check().map_err(|_| TypeTransformError::Cancelled)?;
            if let Some(old) = declarations.get_mut(&id) {
                if old.value != contract {
                    return Err(TypeTransformError::InvalidContract);
                }
                old.modules.push(module);
                continue;
            }
            #[cfg(test)]
            preparation_tests::record(cancel);
            definitions
                .resolve(id)
                .map_err(|_| TypeTransformError::InvalidContract)?;
            validate_references(contract, definitions, cancel)?;
            for method in &contract.methods {
                definitions
                    .lookup_child(id, DefinitionKind::Method, &method.name, 0)
                    .ok_or(TypeTransformError::InvalidContract)?;
            }
            declarations.insert(id, Record::new(contract, module));
        }
        let mut templates = BTreeMap::new();
        for (module, template) in native_declarations {
            #[cfg(test)]
            preparation_tests::record(cancel);
            cancel.check().map_err(|_| TypeTransformError::Cancelled)?;
            if templates.len() >= MAX_NATIVE_DECLARATIONS {
                return Err(TypeTransformError::LimitExceeded);
            }
            validate_references(template, definitions, cancel)?;
            let owner = definitions
                .resolve(template.declaration)
                .map_err(|_| TypeTransformError::InvalidContract)?;
            validate_native_declarations(
                slice::from_ref(template),
                owner.module(),
                definitions,
                cancel,
            )
            .map_err(|error| match error {
                DeclarationValidationError::Cancelled => TypeTransformError::Cancelled,
                _ => TypeTransformError::InvalidContract,
            })?;
            if templates
                .insert(template.declaration, Record::new(template, module))
                .is_some()
            {
                return Err(TypeTransformError::InvalidContract);
            }
        }
        Ok(Self {
            definitions: definitions.clone(),
            implementations: ScopedRecords::new(implementations),
            enumerations: ScopedMap::new(payloads),
            contracts: ScopedMap::new(declarations),
            native_declarations: ScopedMap::new(templates),
        })
    }

    fn validate_linked(&self, cancel: &CancellationToken) -> Result<(), TypeTransformError> {
        for template in self.native_declarations.values() {
            for required in &template.callable_requirements {
                if !self.callable_requirement_valid(required)
                    || !self.holds(
                        &required.interface,
                        &required.receiver,
                        &template.function.bounds,
                        cancel,
                    )?
                {
                    return Err(TypeTransformError::InvalidContract);
                }
            }
        }
        if !self.native_defaults_valid(cancel)? {
            return Err(TypeTransformError::InvalidContract);
        }
        Ok(())
    }

    pub fn ancestry(
        &self,
        interface: &NominalTy,
        receiver: &Ty,
        cancel: &CancellationToken,
    ) -> Result<Vec<NominalTy>, TypeTransformError> {
        validate_references(interface, &self.definitions, cancel)?;
        validate_references(receiver, &self.definitions, cancel)?;
        inheritance::trait_closure(interface, receiver, cancel, &|id| {
            self.contracts.get(id).copied()
        })
    }

    /// A protocol slot from a validated carried trait declaration.
    pub fn method(&self, interface: &DefinitionId, slot: usize) -> Option<&FnDecl> {
        self.contracts.get(interface)?.methods.get(slot)
    }

    /// Trait-owned generic parameters, separately from a method's local scope.
    pub fn parameters(&self, interface: &DefinitionId) -> Option<&[GenericParam]> {
        self.contracts
            .get(interface)
            .map(|contract| contract.generic_params.as_slice())
    }

    pub fn expand_bounds(
        &self,
        bounds: &[GenericBound],
        cancel: &CancellationToken,
    ) -> Result<Vec<GenericBound>, TypeTransformError> {
        let mut expanded =
            TypeSubstitution::default().apply_bounds(bounds, &self.definitions, cancel)?;
        for bound in &expanded {
            validate_references(bound, &self.definitions, cancel)?;
        }
        let budget = Budget::new(cancel);
        for bound in &mut expanded {
            let mut additional = Vec::new();
            for interface in &bound.constraints {
                budget.step(0)?;
                for inherited in self.ancestry(interface, &bound.ty, cancel)? {
                    budget.step(0)?;
                    if !bound.constraints.contains(&inherited) && !additional.contains(&inherited) {
                        additional.push(inherited);
                    }
                }
            }
            bound.constraints.extend(additional);
        }
        Ok(expanded)
    }
}

fn satisfies(available: &NominalTy, required: &NominalTy) -> bool {
    available.satisfies(required)
}

fn validate_references<T: DefinitionReferences>(
    record: &T,
    definitions: &DefinitionTable,
    cancel: &CancellationToken,
) -> Result<(), TypeTransformError> {
    record
        .visit_definition_ids(&mut |id| Ok(definitions.resolve(*id).map(|_| ())?), cancel)
        .map_err(|error| match error {
            DefinitionMappingError::Cancelled => TypeTransformError::Cancelled,
            _ => TypeTransformError::InvalidContract,
        })
}

#[cfg(test)]
mod tests;

#[cfg(test)]
mod preparation_tests;
