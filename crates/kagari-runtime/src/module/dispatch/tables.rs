//! Installation-owned correspondence indexes over shared product facts.
use crate::{
    error::RuntimeError,
    module::{
        ModuleStore, VerifiedProgram,
        admission::DefinitionAdmission,
        dispatch::{
            PublicationDispatch, PublicationToken,
            facts::{DispatchFacts, Placement, RouteSelection, TargetRecord},
            identity::{DeclarationIndex, PublicationDefinitions},
            invalid,
        },
    },
};
use kagari_bytecode::{module::CallableTarget, program::ModuleRef};
use kagari_common::{cancellation::CancellationToken, identity::ModuleIdentity};
use kagari_contract::{native_import::NativeImport, types::ConcreteFunctionIdentity};
use std::{collections::HashMap, sync::Arc};

#[derive(Debug)]
struct NativeTarget {
    provider: ModuleIdentity,
    contract: NativeImport,
    placement: Placement,
}

#[derive(Debug)]
pub(super) struct DispatchTables {
    pub(super) facts: Arc<DispatchFacts>,
    named: HashMap<ConcreteFunctionIdentity, Placement>,
    natives: HashMap<ConcreteFunctionIdentity, Vec<NativeTarget>>,
}

impl PublicationDispatch {
    pub(in crate::module) fn prepare(
        store: &ModuleStore,
        token: PublicationToken,
        previous: Option<&Self>,
        program: &VerifiedProgram,
        admission: Arc<DefinitionAdmission>,
    ) -> Result<Self, RuntimeError> {
        let (index, scope) = DeclarationIndex::prepare(
            store,
            token,
            previous.map(|old| &old.index),
            program,
            admission,
        )?;
        let tables = DispatchTables::prepare(program, &scope)?;
        Ok(Self {
            index,
            scope,
            tables,
            routes: HashMap::new(),
        })
    }
}

impl DispatchTables {
    fn prepare(
        program: &VerifiedProgram,
        scope: &PublicationDefinitions,
    ) -> Result<Self, RuntimeError> {
        let facts = program.dispatch_facts()?.clone();
        let mut result = Self {
            facts,
            named: HashMap::new(),
            natives: HashMap::new(),
        };
        let cancel = CancellationToken::default();
        for (&placement, target) in result.facts.iter() {
            match target.selection.as_ref() {
                RouteSelection::Named { application, .. } => {
                    let application = scope.canonicalize(scope, application, &cancel)?;
                    if result.named.insert(application, placement).is_some() {
                        return Err(invalid("duplicate current callable application"));
                    }
                }
                RouteSelection::Native { provider, import } => {
                    let contract = scope.canonicalize(scope, import.as_ref(), &cancel)?;
                    result
                        .natives
                        .entry(contract.instance.clone())
                        .or_default()
                        .push(NativeTarget {
                            provider: provider.clone(),
                            contract,
                            placement,
                        });
                }
                RouteSelection::BodyOwned => {}
            }
        }
        Ok(result)
    }

    pub(super) fn target(
        &self,
        module: ModuleRef,
        target: CallableTarget,
    ) -> Option<&TargetRecord> {
        self.facts.target(module, target)
    }

    pub(super) fn select(
        &self,
        current: &PublicationDefinitions,
        source: &PublicationDefinitions,
        selection: &RouteSelection,
        cancel: &CancellationToken,
    ) -> Result<Placement, RuntimeError> {
        match selection {
            RouteSelection::Named {
                application,
                shared,
            } => {
                let application = current.canonicalize(source, application, cancel)?;
                let placement = self
                    .named
                    .get(&application)
                    .copied()
                    .ok_or_else(|| invalid("missing admitted current application"))?;
                if self
                    .facts
                    .target(placement.0, placement.1)
                    .is_none_or(|target| target.generic.is_some() != *shared)
                {
                    return Err(invalid("current shared entry contract changed"));
                }
                Ok(placement)
            }
            RouteSelection::Native { provider, import } => {
                let contract = current.canonicalize(source, import.as_ref(), cancel)?;
                self.natives
                    .get(&contract.instance)
                    .into_iter()
                    .flatten()
                    .find(|target| target.provider == *provider && target.contract == contract)
                    .map(|target| target.placement)
                    .ok_or_else(|| invalid("missing exact current native provider evidence"))
            }
            RouteSelection::BodyOwned => Err(invalid("body-owned target requires exact selection")),
        }
    }
}
