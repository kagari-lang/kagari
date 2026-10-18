//! Module-owned recipes collect once, seal declarations, then prepare native entries.
//!
//! No runtime is allocated by this API. Declaration snapshots contain no adapters;
//! consuming an assembly prepares ordinary modules for the existing installer.
pub mod authoring;
pub mod draft;
pub mod function;
pub mod references;
mod seal;

#[cfg(test)]
mod default_tests;

#[cfg(test)]
mod proof_tests;

#[cfg(test)]
mod tests;

#[cfg(test)]
mod transfer_tests;

use crate::{
    error::RuntimeError,
    native::{
        assembly::{
            authoring::{PendingBinding, SealedPreparation, SendPrepare},
            draft::{DraftState, ModuleDraft},
            seal::seal,
        },
        binding::NativeResult,
        catalog::{DeclarationCatalog, import::CatalogImports},
        module::NativeModule,
        storage::NativeStorage,
    },
};
use kagari_common::{
    cancellation::CancellationToken,
    identity::{
        ModuleIdentity, map::DefinitionContext, metadata::DefinitionMetadata, table::DefinitionId,
    },
};
use kagari_types::declaration::module::ModuleDecl;
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
};

type Recipe = Box<dyn FnOnce(&mut ModuleDraft<'_>) -> NativeResult<()>>;

/// One canonical owner and one authoring recipe, consumed exactly once.
/// Recipes run sequentially and may own thread-local authoring state.
pub struct ModuleDefinition {
    identity: ModuleIdentity,
    define: Recipe,
}

impl ModuleDefinition {
    pub fn new(
        identity: ModuleIdentity,
        define: impl FnOnce(&mut ModuleDraft<'_>) -> NativeResult<()> + 'static,
    ) -> Self {
        Self {
            identity,
            define: Box::new(define),
        }
    }
}

/// An isolated registration batch. External providers are adopted once at creation.
pub struct ModuleSetBuilder {
    context: DefinitionContext,
    providers: DeclarationCatalog,
    owners: BTreeSet<ModuleIdentity>,
    definitions: Vec<ModuleDefinition>,
}

impl ModuleSetBuilder {
    pub fn new(providers: &DeclarationCatalog) -> NativeResult<Self> {
        let context = DefinitionContext::new().map_err(conflict)?;
        let providers = CatalogImports::new(context.clone()).import(providers)?;
        Ok(Self {
            context,
            providers,
            owners: BTreeSet::new(),
            definitions: vec![],
        })
    }

    /// Reject duplicate and external owners before executing any recipe.
    pub fn add(&mut self, definition: ModuleDefinition) -> NativeResult<()> {
        if self.providers.contains_module(&definition.identity)
            || self.owners.contains(&definition.identity)
        {
            return Err(conflict("duplicate or external module owner"));
        }
        self.context
            .intern_root(&definition.identity)
            .map_err(conflict)?;
        self.owners.insert(definition.identity.clone());
        self.definitions.push(definition);
        Ok(())
    }

    /// Run recipes, lower implementation intents against complete contracts, and
    /// validate all semantic records. Adapter factories are not called here.
    pub fn finish(self) -> NativeResult<ModuleAssembly> {
        let mut drafts = Vec::with_capacity(self.definitions.len());
        for definition in self.definitions {
            let mut state = DraftState::new(definition.identity);
            (definition.define)(&mut ModuleDraft::new(&self.context, &mut state))?;
            drafts.push(state);
        }
        let snapshot = seal(&self.context, &self.providers, &mut drafts)?;
        Ok(ModuleAssembly { snapshot, drafts })
    }
}

/// Independently retained, immutable semantic products, including presentation data.
/// Cloning shares records; it cannot run recipes or native factories.
#[derive(Clone, Debug)]
pub struct DeclarationSnapshot {
    modules: Arc<[DefinitionMetadata<ModuleDecl>]>,
    catalog: DeclarationCatalog,
}

impl DeclarationSnapshot {
    pub fn modules(&self) -> &[DefinitionMetadata<ModuleDecl>] {
        &self.modules
    }

    pub fn catalog(&self) -> &DeclarationCatalog {
        &self.catalog
    }
}

/// Sealed semantic records and privately owned, still unprepared native associations.
/// This type is intentionally not Clone: factories have one-shot ownership.
pub struct ModuleAssembly {
    snapshot: DeclarationSnapshot,
    drafts: Vec<DraftState>,
}

struct SealedModule {
    declaration: ModuleDecl,
    storage: BTreeMap<DefinitionId, NativeStorage>,
}

struct PendingModule {
    module: SealedModule,
    bindings: BTreeMap<DefinitionId, PendingBinding>,
}

impl From<DraftState> for PendingModule {
    fn from(draft: DraftState) -> Self {
        Self {
            module: SealedModule {
                declaration: draft.declaration,
                storage: draft.storage,
            },
            bindings: draft.bindings,
        }
    }
}

struct SendPendingModule {
    module: SealedModule,
    bindings: BTreeMap<DefinitionId, SendPrepare>,
}

/// A sealed, one-shot assembly whose pending preparations can cross threads.
/// It retains no recipes, configuration callbacks or draft state. Transfer does
/// not prepare adapters; consume it on the destination thread with into_modules.
/// Send is structural, not an unsafe assertion; Sync is not required of factories.
pub struct SendModuleAssembly {
    snapshot: DeclarationSnapshot,
    modules: Vec<SendPendingModule>,
}

impl SendModuleAssembly {
    pub fn declarations(&self) -> DeclarationSnapshot {
        self.snapshot.clone()
    }

    /// Use the ordinary sealed preparation pipeline without recollection or sealing.
    pub fn into_modules(self) -> NativeResult<Vec<NativeModule>> {
        let modules = self
            .modules
            .into_iter()
            .map(|pending| PendingModule {
                module: pending.module,
                bindings: pending
                    .bindings
                    .into_iter()
                    .map(|(id, prepare)| (id, PendingBinding::from_transferable(prepare)))
                    .collect(),
            })
            .collect();
        prepare_modules(&self.snapshot.catalog, modules)
    }
}

impl ModuleAssembly {
    pub fn declarations(&self) -> DeclarationSnapshot {
        self.snapshot.clone()
    }

    /// Extract transferable pending closures without running factories or sealing again.
    /// Ordinary implementation/implementation_with entries are explicitly local,
    /// even if their captures happen to be Send. Failure consumes this assembly and
    /// reports the canonical module and member of the first local binding.
    pub fn try_into_send(self) -> NativeResult<SendModuleAssembly> {
        let mut modules = Vec::with_capacity(self.drafts.len());
        for pending in self.drafts.into_iter().map(PendingModule::from) {
            let mut bindings = BTreeMap::new();
            for (id, binding) in pending.bindings {
                let prepare = binding.into_transferable().ok_or_else(|| {
                    let definitions = self.snapshot.catalog.definitions();
                    match definitions.resolve(id) {
                        Ok(path) => conflict(format!(
                            "local pending binding cannot transfer: {}::{} ({:?})",
                            path.module(),
                            path.segments()
                                .map(|part| part.name)
                                .collect::<Vec<_>>()
                                .join("::"),
                            path.to_path().path,
                        )),
                        Err(error) => conflict(error),
                    }
                })?;
                bindings.insert(id, prepare);
            }
            modules.push(SendPendingModule {
                module: pending.module,
                bindings,
            });
        }
        Ok(SendModuleAssembly {
            snapshot: self.snapshot,
            modules,
        })
    }

    /// Prepare every adapter against the sealed catalog. Failure returns no modules
    /// and never installs anything. Factory side effects themselves cannot be undone.
    pub fn into_modules(self) -> NativeResult<Vec<NativeModule>> {
        prepare_modules(
            &self.snapshot.catalog,
            self.drafts.into_iter().map(PendingModule::from).collect(),
        )
    }
}

fn prepare_modules(
    catalog: &DeclarationCatalog,
    pending_modules: Vec<PendingModule>,
) -> NativeResult<Vec<NativeModule>> {
    let mut modules = Vec::with_capacity(pending_modules.len());
    for state in pending_modules {
        let mut bindings = Vec::with_capacity(state.bindings.len());
        for (id, pending) in state.bindings {
            let declaration = catalog
                .declarations
                .get_id(id)
                .ok_or_else(|| conflict("pending binding has no sealed declaration"))?;
            let scoped = DefinitionMetadata::checked(
                catalog.definitions(),
                declaration.clone(),
                &CancellationToken::default(),
            )
            .map_err(conflict)?;
            let preparation = SealedPreparation::new(&scoped, catalog);
            let binding = pending.prepare(&declaration.function, &preparation)?;
            bindings.push((id, binding));
        }
        modules.push(NativeModule::checked(
            state.module.declaration,
            bindings,
            state.module.storage,
            vec![],
            catalog,
        )?);
    }
    Ok(modules)
}

fn conflict(error: impl ToString) -> RuntimeError {
    RuntimeError::metadata_conflict(error.to_string())
}
