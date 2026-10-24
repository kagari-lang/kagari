//! The admission walker either reports a missing product or batches it for the planner.
use crate::{
    callable::interface::InterfaceCallContract,
    types::ConcreteFunctionIdentity,
    updates::{
        BodyApplication, CallRequirement, DemandRequests, InterfaceRequest,
        limits::{Budget, UpdateError},
        wire::call_key,
    },
};
use kagari_common::identity::{
    mapping::DefinitionMappingError, scoped::DefinitionReferences, table::DefinitionTable,
};
use kagari_types::ty::Ty;
use std::collections::HashSet;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(super) enum Request {
    Function(ConcreteFunctionIdentity),
    Shared(BodyApplication),
    Interface(InterfaceRequest),
    Construction(ConcreteFunctionIdentity),
    Layout(Ty),
}

pub(super) struct Requests<'a> {
    definitions: Option<&'a DefinitionTable>,
    seen: HashSet<Request>,
    result: DemandRequests,
}

impl<'a> Requests<'a> {
    pub(super) fn new(definitions: Option<&'a DefinitionTable>) -> Self {
        Self {
            definitions,
            seen: HashSet::new(),
            result: DemandRequests::default(),
        }
    }

    pub(super) fn missing(
        &mut self,
        request: Request,
        budget: &mut Budget<'_>,
    ) -> Result<(), UpdateError> {
        budget.visit()?;
        let Some(definitions) = self.definitions else {
            return Err(match request {
                Request::Function(id) => UpdateError::MissingTarget(id),
                Request::Shared(application) => UpdateError::MissingBody(application.body),
                _ => UpdateError::MissingCoverage,
            });
        };
        if self.seen.contains(&request) {
            return Ok(());
        }

        let accounting = match &request {
            Request::Function(id) | Request::Construction(id) => {
                CallRequirement::Direct(id.clone())
            }
            Request::Shared(application) => CallRequirement::RetainedBody(application.clone()),
            Request::Layout(ty) => CallRequirement::Layout(ty.clone()),
            Request::Interface(call) => {
                CallRequirement::Interface(Box::new(InterfaceCallContract {
                    interface: call.interface.clone(),
                    receiver: call.receiver.clone(),
                    method_slot: call.method_slot,
                    arguments: call.arguments.clone(),
                    operations: vec![],
                    constructions: vec![],
                    normalizations: vec![],
                }))
            }
        };
        // Accounting uses the existing bounded identity/semantic codecs; these
        // temporary records are never passed off as checked call evidence.
        let mut references = HashSet::new();
        let cancel = budget.cancel;
        accounting.visit_definition_ids(
            &mut |id| {
                budget.visit().map_err(|error| match error {
                    UpdateError::Cancelled => DefinitionMappingError::Cancelled,
                    _ => DefinitionMappingError::LimitExceeded,
                })?;
                references.insert(*id);
                Ok(())
            },
            cancel,
        )?;
        let encoding = definitions
            .encode(references)
            .map_err(DefinitionMappingError::from)?;
        let _ = call_key(&accounting, &encoding, budget)?;
        match &request {
            Request::Function(id) => budget.application(&BodyApplication {
                body: id.clone(),
                arguments: vec![],
            })?,
            Request::Shared(application) => budget.application(application)?,
            _ => {}
        }
        self.seen.insert(request.clone());
        match request {
            Request::Function(value) => self.result.functions.push(value),
            Request::Shared(value) => self.result.shared.push(value),
            Request::Interface(value) => self.result.interfaces.push(value),
            Request::Construction(value) => self.result.constructions.push(value),
            Request::Layout(value) => self.result.layouts.push(value),
        }
        Ok(())
    }

    pub(super) fn finish(self) -> DemandRequests {
        self.result
    }
}
