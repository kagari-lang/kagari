//! Native catalog values and keys share an explicit checked identity context.
use crate::native::types::{TraitRef, TypeRef};
use crate::{
    error::RuntimeError,
    native::{catalog::DeclarationCatalog, module::NativeModule},
};
use kagari_common::{
    cancellation::CancellationToken,
    identity::{
        DefinitionKind, DefinitionPath,
        map::DefinitionMap,
        mapping::{DefinitionMapper, DefinitionRecord},
        table::{DefinitionId, DefinitionTable},
    },
};
use kagari_types::declaration::{
    NativeDeclaration, TraitDef, TypeDef,
    module::{ImplDecl, ModuleDecl},
    ownership::ReceiverOwners,
};
use std::sync::Arc;

fn conflict(cause: impl ToString) -> RuntimeError {
    RuntimeError::metadata_conflict(cause.to_string())
}

impl DeclarationCatalog {
    /// Validate a complete, explicitly supplied set of declaration providers.
    pub fn from_declarations<'a>(
        modules: impl IntoIterator<Item = &'a ModuleDecl>,
    ) -> Result<Self, RuntimeError> {
        let modules = modules.into_iter().collect::<Vec<_>>();
        let catalog = Self::declared(modules.iter().copied())?;
        let owners = catalog.receiver_owners(None)?;
        for module in &modules {
            module
                .validate(&|receiver| {
                    catalog
                        .scope(receiver)
                        .ok()
                        .and_then(|receiver| owners.owner(&receiver, &catalog.definitions()))
                })
                .map_err(conflict)?;
        }
        catalog.check_implementations(modules.iter().copied())?;
        catalog.validate_callable_contracts()?;
        Ok(catalog)
    }

    pub(crate) fn receiver_owners(
        &self,
        extra: Option<&ModuleDecl>,
    ) -> Result<ReceiverOwners, RuntimeError> {
        let extra = extra
            .map(|module| {
                self.scope(&module.types)
                    .map(|types| (&module.identity, types))
            })
            .transpose()?;
        let definitions = self.definitions();
        let types = self
            .types
            .entries()
            .map(|(id, ty)| definitions.resolve(id).map(|view| (view.module(), ty)))
            .collect::<Result<Vec<_>, _>>()
            .map_err(conflict)?;
        ReceiverOwners::from_types(
            types.into_iter().chain(
                extra
                    .iter()
                    .flat_map(|(module, types)| types.iter().map(|ty| (*module, ty))),
            ),
        )
        .map_err(conflict)
    }

    /// Resolve a checked trait by its portable declaration identity.
    pub fn trait_reference(&self, declaration: &DefinitionPath) -> Result<TraitRef, RuntimeError> {
        let contract = self
            .traits
            .get(declaration)
            .ok_or_else(|| conflict("unknown native trait declaration"))?;
        Ok(TraitRef {
            id: declaration.clone(),
            contract: Arc::new(self.paths(contract)?),
        })
    }

    /// Resolve a checked nominal type by its portable declaration identity.
    pub fn type_reference(&self, declaration: &DefinitionPath) -> Result<TypeRef, RuntimeError> {
        let contract = self
            .types
            .get(declaration)
            .ok_or_else(|| conflict("unknown native type declaration"))?;
        Ok(TypeRef {
            id: declaration.clone(),
            declaration: Arc::new(self.paths(contract)?),
            parameter_names: (0..contract.generic_params.len())
                .map(|index| format!("T{index}"))
                .collect(),
        })
    }

    pub fn from_modules(modules: &[&NativeModule]) -> Result<Self, RuntimeError> {
        let mut result = Self::default();
        for module in modules {
            result.merge(&module.catalog())?;
        }
        Ok(result)
    }

    pub(crate) fn definitions(&self) -> DefinitionTable {
        self.types.context().snapshot()
    }

    pub(crate) fn paths<T: DefinitionRecord<DefinitionId>>(
        &self,
        records: &T,
    ) -> Result<T::Rebind<DefinitionPath>, RuntimeError> {
        let definitions = self.definitions();
        records
            .map_identities(&mut DefinitionMapper::new(
                &mut |id| Ok(definitions.resolve(*id)?.to_path()),
                &Default::default(),
            ))
            .map_err(conflict)
    }

    pub(crate) fn scope<T: DefinitionRecord<DefinitionPath>>(
        &self,
        records: &T,
    ) -> Result<T::Rebind<DefinitionId>, RuntimeError> {
        records
            .map_identities(&mut DefinitionMapper::new(
                &mut |path| self.types.context().intern(path).map_err(Into::into),
                &Default::default(),
            ))
            .map_err(conflict)
    }

    pub(crate) fn declared<'a>(
        modules: impl IntoIterator<Item = &'a ModuleDecl>,
    ) -> Result<Self, RuntimeError> {
        let mut result = Self::default();
        let context = result.types.context().clone();
        for module in modules {
            let root = context.intern_root(&module.identity).map_err(conflict)?;
            Arc::make_mut(&mut result.documentation).extend(module.documentation.clone());
            for ty in &module.types {
                let id = context
                    .intern_child(root, ty.kind.definition_kind(), &ty.name, 0)
                    .map_err(conflict)?;
                let contract = result.scope(ty)?;
                insert_contract(
                    &mut result.types,
                    id,
                    contract,
                    "conflicting native storage type contracts",
                )?;
            }
            for contract in &module.traits {
                let id = context
                    .intern_child(root, DefinitionKind::Trait, &contract.name, 0)
                    .map_err(conflict)?;
                let contract = result.scope(contract)?;
                insert_contract(
                    &mut result.traits,
                    id,
                    contract,
                    "conflicting native trait contracts",
                )?;
            }
            for declaration in module.native_declarations() {
                let declaration = result.scope(&declaration)?;
                result.insert_declaration(declaration)?;
            }
            for (index, implementation) in module.implementations.iter().enumerate() {
                if implementation.trait_type.is_some() {
                    let occurrence = u32::try_from(index)
                        .map_err(|_| conflict("native implementation identity exhausted"))?;
                    let id = context
                        .intern_child(root, DefinitionKind::Impl, "", occurrence)
                        .map_err(conflict)?;
                    let implementation = result.scope(implementation)?;
                    result.insert_implementation(id, &implementation)?;
                }
            }
        }
        Ok(result)
    }

    pub(crate) fn get(&self, declaration: &DefinitionPath) -> Option<&TraitDef<DefinitionId>> {
        self.traits.get(declaration)
    }

    pub(crate) fn insert(
        &mut self,
        id: DefinitionPath,
        contract: TraitDef,
    ) -> Result<(), RuntimeError> {
        let id = self.types.context().intern(&id).map_err(conflict)?;
        let contract = self.scope(&contract)?;
        insert_contract(
            &mut self.traits,
            id,
            contract,
            "conflicting native trait contracts",
        )
    }

    pub(crate) fn insert_type(
        &mut self,
        id: DefinitionPath,
        contract: TypeDef,
    ) -> Result<(), RuntimeError> {
        let id = self.types.context().intern(&id).map_err(conflict)?;
        let contract = self.scope(&contract)?;
        insert_contract(
            &mut self.types,
            id,
            contract,
            "conflicting native storage type contracts",
        )
    }

    pub(crate) fn insert_declaration(
        &mut self,
        declaration: NativeDeclaration<DefinitionId>,
    ) -> Result<(), RuntimeError> {
        let definitions = self.definitions();
        declaration
            .visit_definitions(
                &mut |id| Ok(definitions.resolve(*id).map(|_| ())?),
                &Default::default(),
            )
            .map_err(conflict)?;
        insert_contract(
            &mut self.declarations,
            declaration.declaration,
            declaration,
            "conflicting native template declarations",
        )
    }

    pub(super) fn insert_implementation(
        &mut self,
        id: DefinitionId,
        implementation: &ImplDecl<DefinitionId>,
    ) -> Result<(), RuntimeError> {
        if self.implementations.get_id(id).is_none() && self.implementations.len() >= 4096 {
            return Err(conflict(
                "native implementation catalog exceeds proof limits",
            ));
        }
        let implementation = implementation.clone();
        insert_contract(
            &mut self.implementations,
            id,
            implementation,
            "conflicting native implementation contracts",
        )
    }

    pub(crate) fn merge(&mut self, other: &Self) -> Result<(), RuntimeError> {
        if self
            .implementations
            .union_len(&other.implementations)
            .map_err(conflict)?
            > 4096
        {
            return Err(conflict(
                "native implementation catalog exceeds proof limits",
            ));
        }
        merge_records(
            &mut self.types,
            &other.types,
            "conflicting native storage type contracts",
        )?;
        merge_records(
            &mut self.traits,
            &other.traits,
            "conflicting native trait contracts",
        )?;
        merge_records(
            &mut self.declarations,
            &other.declarations,
            "conflicting native template declarations",
        )?;
        merge_records(
            &mut self.implementations,
            &other.implementations,
            "conflicting native implementation contracts",
        )?;
        Arc::make_mut(&mut self.documentation).extend(
            other
                .documentation
                .iter()
                .map(|(id, text)| (id.clone(), text.clone())),
        );
        Ok(())
    }

    pub(crate) fn satisfied_by(&self, installed: &Self) -> Result<bool, RuntimeError> {
        Ok(subset(&self.types, &installed.types)?
            && subset(&self.traits, &installed.traits)?
            && subset(&self.declarations, &installed.declarations)?
            && subset(&self.implementations, &installed.implementations)?)
    }

    pub(crate) fn check_implementations<'a>(
        &self,
        modules: impl IntoIterator<Item = &'a ModuleDecl>,
    ) -> Result<(), RuntimeError> {
        let traits = self
            .traits
            .iter()
            .map(|(id, contract)| Ok((id, self.paths(contract)?)))
            .collect::<Result<_, RuntimeError>>()?;
        for module in modules {
            module
                .validate_trait_implementations(&traits)
                .map_err(conflict)?;
        }
        Ok(())
    }
}

pub(super) fn insert_contract<T: Clone + PartialEq>(
    map: &mut Arc<DefinitionMap<T>>,
    id: DefinitionId,
    contract: T,
    message: &str,
) -> Result<(), RuntimeError> {
    if let Some(previous) = map.get_id(id) {
        if previous != &contract {
            return Err(conflict(message));
        }
    } else {
        Arc::make_mut(map)
            .insert_id(id, contract)
            .map_err(conflict)?;
    }
    Ok(())
}

fn merge_records<
    T: DefinitionRecord<DefinitionId, Rebind<DefinitionId> = T> + Clone + PartialEq,
>(
    target: &mut Arc<DefinitionMap<T>>,
    source: &DefinitionMap<T>,
    message: &str,
) -> Result<(), RuntimeError> {
    let source = source
        .import_records(target.context(), &CancellationToken::default())
        .map_err(conflict)?;
    if source.is_subset_of(target).map_err(conflict)? {
        return Ok(());
    }
    if !Arc::make_mut(target).merge(&source).map_err(conflict)? {
        return Err(conflict(message));
    }
    Ok(())
}

fn subset<T: DefinitionRecord<DefinitionId, Rebind<DefinitionId> = T> + Clone + PartialEq>(
    source: &DefinitionMap<T>,
    target: &DefinitionMap<T>,
) -> Result<bool, RuntimeError> {
    source
        .import_records(target.context(), &CancellationToken::default())
        .map_err(conflict)?
        .is_subset_of(target)
        .map_err(conflict)
}
