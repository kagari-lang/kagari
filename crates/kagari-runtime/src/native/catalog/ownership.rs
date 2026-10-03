//! Native catalog values and keys share an explicit checked identity context.
use crate::{
    error::RuntimeError,
    native::{
        catalog::{DeclarationCatalog, dependencies::DependencyClosure},
        module::NativeModule,
    },
};
use kagari_abi::{
    declaration::ModuleDecl,
    types::{NativeDeclaration, TraitAbi, TypeAbi},
};
use kagari_common::{
    cancellation::CancellationToken,
    identity::{
        DefinitionPath,
        map::{DefinitionContext, DefinitionMap},
        mapping::{DefinitionMapper, DefinitionRecord},
        table::{DefinitionId, DefinitionTable},
    },
};
use std::sync::Arc;

fn conflict(cause: impl ToString) -> RuntimeError {
    RuntimeError::metadata_conflict(cause.to_string())
}

impl DeclarationCatalog {
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

    pub(crate) fn import_into(&self, context: &DefinitionContext) -> Result<Self, RuntimeError> {
        let cancel = CancellationToken::default();
        Ok(Self {
            types: Arc::new(
                self.types
                    .import_records(context, &cancel)
                    .map_err(conflict)?,
            ),
            traits: Arc::new(
                self.traits
                    .import_records(context, &cancel)
                    .map_err(conflict)?,
            ),
            declarations: Arc::new(
                self.declarations
                    .import_records(context, &cancel)
                    .map_err(conflict)?,
            ),
            implementations: Arc::new(
                self.implementations
                    .import_records(context, &cancel)
                    .map_err(conflict)?,
            ),
        })
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

    /// Complete authoring proof checks borrow this temporary representation.
    /// Installed catalogs retain only compact references and their own context.
    pub(crate) fn to_paths(&self) -> Result<DeclarationCatalog<DefinitionPath>, RuntimeError> {
        Ok(DeclarationCatalog {
            types: Arc::new(self.types.map_values(|record| self.paths(record))?),
            traits: Arc::new(self.traits.map_values(|record| self.paths(record))?),
            declarations: Arc::new(self.declarations.map_values(|record| self.paths(record))?),
            implementations: Arc::new(
                self.implementations
                    .map_values(|record| self.paths(record))?,
            ),
        })
    }

    pub(crate) fn declared<'a>(
        modules: impl IntoIterator<Item = &'a ModuleDecl>,
    ) -> Result<Self, RuntimeError> {
        DeclarationCatalog::<DefinitionPath>::collect_authoring(modules)?.scoped()
    }

    pub(crate) fn get(&self, declaration: &DefinitionPath) -> Option<&TraitAbi<DefinitionId>> {
        self.traits.get(declaration)
    }

    pub(crate) fn insert(
        &mut self,
        id: DefinitionPath,
        contract: TraitAbi,
    ) -> Result<(), RuntimeError> {
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
        contract: TypeAbi,
    ) -> Result<(), RuntimeError> {
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
        declaration: NativeDeclaration,
    ) -> Result<(), RuntimeError> {
        let id = declaration.declaration.clone();
        let declaration = self.scope(&declaration)?;
        if let Some(previous) = self.declarations.get(&id) {
            if previous != &declaration {
                return Err(conflict("conflicting native template declarations"));
            }
        } else {
            Arc::make_mut(&mut self.declarations)
                .insert(id, declaration)
                .map_err(conflict)?;
        }
        Ok(())
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
        )
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
        self.to_paths()?.check_implementations(modules)
    }

    pub(crate) fn validate_callable_contracts(&self) -> Result<(), RuntimeError> {
        self.to_paths()?.validate_callable_contracts()
    }

    pub(crate) fn dependency_closure<'a>(
        &self,
        traits: impl IntoIterator<Item = DefinitionPath>,
        declarations: impl IntoIterator<Item = &'a NativeDeclaration>,
        modules: impl IntoIterator<Item = &'a ModuleDecl>,
    ) -> Result<DependencyClosure, RuntimeError> {
        self.to_paths()?
            .dependency_closure(traits, declarations, modules)
    }

    pub(crate) fn binding_dependencies<'a>(
        &self,
        base: &DependencyClosure,
        declarations: impl IntoIterator<Item = &'a NativeDeclaration>,
    ) -> Result<Self, RuntimeError> {
        self.to_paths()?
            .binding_dependencies(base, declarations)?
            .scoped()
    }
}

fn insert_contract<T: Clone + PartialEq>(
    map: &mut Arc<DefinitionMap<T>>,
    id: DefinitionPath,
    contract: T,
    message: &str,
) -> Result<(), RuntimeError> {
    if let Some(previous) = map.get(&id) {
        if previous != &contract {
            return Err(conflict(message));
        }
    } else {
        Arc::make_mut(map).insert(id, contract).map_err(conflict)?;
    }
    Ok(())
}

impl DeclarationCatalog<DefinitionPath> {
    fn scoped(self) -> Result<DeclarationCatalog, RuntimeError> {
        let context = self.types.context();
        Ok(DeclarationCatalog {
            types: Arc::new(scope_values(&self.types, context)?),
            traits: Arc::new(scope_values(&self.traits, context)?),
            declarations: Arc::new(scope_values(&self.declarations, context)?),
            implementations: Arc::new(scope_values(&self.implementations, context)?),
        })
    }
}

fn scope_values<T: DefinitionRecord<DefinitionPath>>(
    records: &DefinitionMap<T>,
    context: &DefinitionContext,
) -> Result<DefinitionMap<T::Rebind<DefinitionId>>, RuntimeError> {
    records.map_values(|record| {
        record
            .map_identities(&mut DefinitionMapper::new(
                &mut |path| context.intern(path).map_err(Into::into),
                &Default::default(),
            ))
            .map_err(conflict)
    })
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
