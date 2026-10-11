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
        map::{DefinitionContext, DefinitionMap},
        mapping::{DefinitionMapper, DefinitionRecord},
        metadata::DefinitionMetadata,
        table::{DefinitionId, DefinitionTable},
    },
};
use kagari_types::declaration::{
    NativeDeclaration, TraitDef, TypeDef,
    module::{ImplDecl, ModuleDecl},
    ownership::ReceiverOwners,
};
use std::{collections::BTreeMap, sync::Arc};

fn conflict(cause: impl ToString) -> RuntimeError {
    RuntimeError::metadata_conflict(cause.to_string())
}

impl DeclarationCatalog {
    /// Validate scoped declaration providers after explicit adoption into one catalog.
    pub fn from_declarations<'a>(
        modules: impl IntoIterator<Item = &'a DefinitionMetadata<ModuleDecl>>,
    ) -> Result<Self, RuntimeError> {
        let context = DefinitionContext::new().map_err(conflict)?;
        let modules = modules
            .into_iter()
            .map(|module| {
                module
                    .import_into(&context, &CancellationToken::default())
                    .map(|metadata| metadata.into_records())
                    .map_err(conflict)
            })
            .collect::<Result<Vec<_>, _>>()?;
        let catalog = Self::declared(&modules, &context)?;
        let definitions = catalog.definitions();
        let owners = catalog.receiver_owners(None)?;
        for module in &modules {
            module
                .validate(
                    &|receiver| owners.owner(receiver, &definitions),
                    &definitions,
                )
                .map_err(conflict)?;
        }
        catalog.check_implementations(&modules)?;
        catalog.validate_callable_contracts()?;
        Ok(catalog)
    }

    pub(crate) fn receiver_owners(
        &self,
        extra: Option<&ModuleDecl>,
    ) -> Result<ReceiverOwners, RuntimeError> {
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
                    .into_iter()
                    .flat_map(|module| module.types.iter().map(|ty| (&module.identity, ty))),
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
        context: &DefinitionContext,
    ) -> Result<Self, RuntimeError> {
        let mut result = Self::in_context(context.clone());
        for module in modules {
            let definitions = module.collect_definitions(context).map_err(conflict)?;
            let root = definitions
                .lookup_root(&module.identity)
                .expect("collected module identity");
            Arc::make_mut(&mut result.documentation).extend(module.documentation.clone());
            for ty in &module.types {
                let id = context
                    .intern_child(root, ty.kind.definition_kind(), &ty.name, 0)
                    .map_err(conflict)?;
                let contract = ty.clone();
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
                let contract = contract.clone();
                insert_contract(
                    &mut result.traits,
                    id,
                    contract,
                    "conflicting native trait contracts",
                )?;
            }
            for declaration in module.native_declarations(&definitions).map_err(conflict)? {
                result.insert_declaration(declaration)?;
            }
            for (index, implementation) in module.implementations.iter().enumerate() {
                if implementation.trait_type.is_some() {
                    let occurrence = u32::try_from(index)
                        .map_err(|_| conflict("native implementation identity exhausted"))?;
                    let id = context
                        .intern_child(root, DefinitionKind::Impl, "", occurrence)
                        .map_err(conflict)?;
                    result.insert_implementation(id, implementation)?;
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
        id: DefinitionId,
        contract: TraitDef<DefinitionId>,
    ) -> Result<(), RuntimeError> {
        check_scope(&contract, &self.definitions())?;
        insert_contract(
            &mut self.traits,
            id,
            contract,
            "conflicting native trait contracts",
        )
    }

    pub(crate) fn insert_type(
        &mut self,
        id: DefinitionId,
        contract: TypeDef<DefinitionId>,
    ) -> Result<(), RuntimeError> {
        check_scope(&contract, &self.definitions())?;
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
        check_scope(&declaration, &self.definitions())?;
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
        check_scope(implementation, &self.definitions())?;
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
        let documentation = self.import_documentation(other)?;
        Arc::make_mut(&mut self.documentation)
            .extend(documentation.iter().map(|(id, text)| (*id, text.clone())));
        self.collect_implicit_definitions()?;
        Ok(())
    }

    pub(super) fn import_documentation(
        &self,
        other: &Self,
    ) -> Result<Arc<BTreeMap<DefinitionId, String>>, RuntimeError> {
        if self.definitions().id() == other.definitions().id() {
            return Ok(other.documentation.clone());
        }
        let remap = self
            .types
            .context()
            .import(&other.definitions(), other.documentation.keys().copied())
            .map_err(conflict)?;
        let entries = other
            .documentation
            .iter()
            .map(|(id, text)| Ok((remap.map(*id).map_err(conflict)?, text.clone())))
            .collect::<Result<_, RuntimeError>>()?;
        Ok(Arc::new(entries))
    }

    /// Importing explicit references does not collect implicit member names.
    /// Register those names separately; this operation grants no ABI authority.
    pub(super) fn collect_implicit_definitions(&self) -> Result<(), RuntimeError> {
        let context = self.types.context();
        for (owner, ty) in self.types.entries() {
            for variant in &ty.variants {
                context
                    .intern_child(owner, DefinitionKind::Variant, &variant.name, 0)
                    .map_err(conflict)?;
            }
        }
        for (owner, contract) in self.traits.entries() {
            for method in &contract.methods {
                context
                    .intern_child(owner, DefinitionKind::Method, &method.name, 0)
                    .map_err(conflict)?;
            }
        }
        for (owner, implementation) in self.implementations.entries() {
            for method in &implementation.methods {
                context
                    .intern_child(owner, DefinitionKind::Method, &method.name, 0)
                    .map_err(conflict)?;
            }
        }
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
        let traits = self.traits.entries().collect();
        let definitions = self.definitions();
        for module in modules {
            module
                .validate_trait_implementations(&traits, &definitions)
                .map_err(conflict)?;
        }
        Ok(())
    }
}

fn check_scope(
    records: &impl DefinitionRecord<DefinitionId>,
    definitions: &DefinitionTable,
) -> Result<(), RuntimeError> {
    records
        .visit_definitions(
            &mut |id| Ok(definitions.resolve(*id).map(|_| ())?),
            &CancellationToken::default(),
        )
        .map_err(conflict)
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
