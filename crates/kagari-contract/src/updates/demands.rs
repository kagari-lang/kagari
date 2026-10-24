//! Monotonic historical requirements and one bounded walker for planning/admission.
mod native;
mod requests;
mod witnesses;

use crate::{
    callable::{
        construction::ConstructionMethod, generic::GenericBody, interface::InterfaceCallContract,
        shared::SharedCall,
    },
    native_import::NativeImport,
    types::{ConcreteFunctionIdentity, proofs::ProofCatalog},
    updates::{
        BodyApplication, BodyClassification, BodyRecord, CallRequirement, DemandEnvelope,
        DemandRequests, InterfaceRequest, UpdateEvidence,
        admission::nested,
        demands::requests::{Request, Requests},
        limits::{Budget, UpdateError},
        wire::{application_key, body_key, call_key},
    },
};
use kagari_common::identity::{
    mapping::DefinitionMappingError, scoped::DefinitionReferences, table::DefinitionTable,
};
use kagari_types::{
    callable::CallableImplementation,
    ty::{Ty, substitution::TypeSubstitution},
};
use std::collections::{HashMap, HashSet, VecDeque};

/// A checked installed native application and optional trusted complete closure.
/// None remains unknown, not a sealed primitive. Coverage still retains all known
/// published applications and visits the import's checked callable/result adapters.
/// Protected comparison independently freezes the full inventory for unknown reentry.
pub struct NativeClosure<'a> {
    pub import: &'a NativeImport,
    pub callbacks: Option<&'a [CallRequirement]>,
}

/// Candidate-owned checked catalogs, after executable/linked witness verification.
/// Methods enumerate all admissible checked implementations, including new receivers.
/// Prior contracts and retained symbolic routes must have passed declaration
/// preservation; no method, construction or native record here authenticates itself.
pub struct Coverage<'a> {
    pub evidence: &'a UpdateEvidence,
    pub methods: &'a [ConstructionMethod],
    pub constructions: &'a [ConcreteFunctionIdentity],
    pub layouts: &'a [Ty],
    pub natives: &'a [NativeClosure<'a>],
}

/// Retain every current application, including named functions never directly called,
/// plus all body summaries and checked shared templates. Unknown values/reentry may
/// reach those applications; replacing source reachability cannot erase that demand.
pub fn publication_demands(
    evidence: &UpdateEvidence,
    budget: &mut Budget<'_>,
) -> Result<DemandEnvelope, UpdateError> {
    let mut result = DemandEnvelope::default();
    for body in &evidence.bodies {
        budget.visit()?;
        budget.records(1)?;
        let application = BodyApplication {
            body: body.requirement.origin.clone(),
            arguments: parameters(body.requirement.generic.as_ref()),
        };
        budget.application(&application)?;
        result.applications.push(application);
        result.bodies.push(body.requirement.clone());
    }
    for target in &evidence.targets {
        budget.visit()?;
        budget.records(1)?;
        result
            .requirements
            .push(CallRequirement::Direct(target.application.clone()));
    }
    for selection in &evidence.selections {
        budget.visit()?;
        budget.records(1)?;
        result.requirements.push(selection.clone());
    }
    Ok(result)
}

/// Deduplicate exact canonical records without replacing the installation's history.
/// Different versions of the same body origin retain both historical edge sets.
pub fn union_demands(
    previous: &DemandEnvelope,
    candidate: &DemandEnvelope,
    definitions: &DefinitionTable,
    budget: &mut Budget<'_>,
) -> Result<DemandEnvelope, UpdateError> {
    let mut references = HashSet::new();
    let cancel = budget.cancel;
    for envelope in [previous, candidate] {
        envelope.visit_definition_ids(
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
    }
    let encoding = definitions
        .encode(references)
        .map_err(DefinitionMappingError::from)?;
    let mut result = DemandEnvelope::default();
    let mut applications = HashSet::new();
    for application in previous.applications.iter().chain(&candidate.applications) {
        budget.visit()?;
        nested(application.arguments.len())?;
        let key = application_key(application, &encoding, budget)?;
        if !applications.contains(&key) {
            budget.application(application)?;
            applications.insert(key);
            result.applications.push(application.clone());
        }
    }
    let mut bodies = HashSet::new();
    for body in previous.bodies.iter().chain(&candidate.bodies) {
        budget.visit()?;
        let key = body_key(body, &encoding, budget)?;
        if bodies.insert(key) {
            result.bodies.push(body.clone());
        }
    }
    let mut calls = HashSet::new();
    for call in previous.requirements.iter().chain(&candidate.requirements) {
        budget.visit()?;
        let key = call_key(call, &encoding, budget)?;
        if calls.insert(key) {
            result.requirements.push(call.clone());
        }
    }
    Ok(result)
}

/// Check candidate coverage without compiling or selecting replacement impls.
/// Unknown does not imply empty closure: publication demands retain all known current
/// applications, and verified symbolic routes stay shared. Every known concrete
/// edge still requires a candidate target. Missing registration is an error.
/// `catalog` must contain the candidate's linked proof contracts in the same
/// correspondence scope as coverage and history. Retained shared calls are checked
/// against candidate binders/signatures using their historical caller assumptions.
/// Its identity table must include historical binder owners, even for removed bodies;
/// its proof facts must come from the candidate, not the historical publication.
pub fn check_coverage(
    envelope: &DemandEnvelope,
    coverage: &Coverage<'_>,
    catalog: &ProofCatalog<'_>,
    budget: &mut Budget<'_>,
) -> Result<(), UpdateError> {
    walk(envelope, coverage, catalog, None, budget).map(|_| ())
}

/// Expand a batch of missing candidate products using exactly the admission walker.
/// Inputs share one DeclarationCorrespondence scope. The compiler applies these
/// requests through its existing planner, unions the resulting candidate summaries,
/// and repeats with the same budget until empty; it then runs strict admission.
/// Missing retained certificates and unsupported symbolic routes are errors, not
/// requests to reconstruct historical code. Runtime must never invoke a compiler.
/// Uses the same candidate catalog and historical caller-scope checks as coverage;
/// an existing but incompatible body is an error, not a compilation request.
pub fn expand_requests(
    envelope: &DemandEnvelope,
    coverage: &Coverage<'_>,
    catalog: &ProofCatalog<'_>,
    budget: &mut Budget<'_>,
) -> Result<DemandRequests, UpdateError> {
    walk(
        envelope,
        coverage,
        catalog,
        Some(catalog.definitions()),
        budget,
    )
}

fn walk(
    envelope: &DemandEnvelope,
    coverage: &Coverage<'_>,
    catalog: &ProofCatalog<'_>,
    definitions: Option<&DefinitionTable>,
    budget: &mut Budget<'_>,
) -> Result<DemandRequests, UpdateError> {
    let mut walker = Walker {
        coverage,
        catalog,
        requests: Requests::new(definitions),
        depth: 0,
        current_bodies: HashMap::new(),
        pending: VecDeque::new(),
        seen: HashSet::new(),
        bodies: HashMap::new(),
        targets: HashMap::new(),
        native_seen: HashSet::new(),
        native_validated: HashSet::new(),
    };
    for (index, body) in envelope.bodies.iter().enumerate() {
        budget.visit()?;
        walker.bodies.entry(&body.origin).or_default().push(index);
    }
    for body in &coverage.evidence.bodies {
        budget.visit()?;
        if walker
            .current_bodies
            .insert(&body.requirement.origin, body)
            .is_some()
        {
            return Err(UpdateError::InvalidEvidence);
        }
        let indices = walker
            .bodies
            .get(&body.requirement.origin)
            .ok_or_else(|| UpdateError::MissingBody(body.requirement.origin.clone()))?;
        let mut found = false;
        for index in indices {
            budget.visit()?;
            if envelope.bodies[*index] == body.requirement {
                found = true;
                break;
            }
        }
        if !found {
            return Err(UpdateError::InvalidEvidence);
        }
    }
    for target in &coverage.evidence.targets {
        budget.visit()?;
        if walker
            .targets
            .insert(&target.application, &target.body)
            .is_some()
        {
            return Err(UpdateError::InvalidEvidence);
        }
    }
    for target in &coverage.evidence.targets {
        walker.direct(target.application.clone(), envelope, budget)?;
    }
    for application in &envelope.applications {
        walker.enqueue(application.clone(), envelope, budget)?;
    }
    for call in &envelope.requirements {
        walker.call(call, &TypeSubstitution::default(), None, envelope, budget)?;
    }
    // Complete checked dynamic catalogs must not hide an absent executable behind
    // a symbolic caller whose concrete receiver is not known until execution.
    for method in coverage.methods {
        budget.visit()?;
        walker.current(
            BodyApplication {
                body: method.body.clone(),
                arguments: method.arguments.clone(),
            },
            envelope,
            budget,
        )?;
    }
    while let Some((index, arguments)) = walker.pending.pop_front() {
        budget.visit()?;
        let body = &envelope.bodies[index];
        let generic = body.generic.as_ref();
        let params = generic.map_or(&[][..], |generic| generic.parameters.as_slice());
        if params.len() != arguments.len() {
            return Err(UpdateError::InvalidEvidence);
        }
        let mut bindings = TypeSubstitution::default();
        for (parameter, argument) in params.iter().zip(&arguments) {
            bindings.bind(&parameter.owner, parameter.position, argument);
        }
        for call in &body.calls {
            walker.call(call, &bindings, generic, envelope, budget)?;
        }
    }
    Ok(walker.requests.finish())
}

struct Walker<'a, 'd> {
    coverage: &'a Coverage<'a>,
    catalog: &'a ProofCatalog<'a>,
    requests: Requests<'d>,
    depth: usize,
    current_bodies: HashMap<&'a ConcreteFunctionIdentity, &'a BodyRecord>,
    bodies: HashMap<&'a ConcreteFunctionIdentity, Vec<usize>>,
    targets: HashMap<&'a ConcreteFunctionIdentity, &'a ConcreteFunctionIdentity>,
    pending: VecDeque<(usize, Vec<Ty>)>,
    seen: HashSet<(usize, Vec<Ty>)>,
    native_seen: HashSet<(usize, BodyApplication)>,
    native_validated: HashSet<usize>,
}

impl Walker<'_, '_> {
    fn enqueue(
        &mut self,
        application: BodyApplication,
        envelope: &DemandEnvelope,
        budget: &mut Budget<'_>,
    ) -> Result<(), UpdateError> {
        nested(application.arguments.len())?;
        let indices = self
            .bodies
            .get(&application.body)
            .ok_or_else(|| UpdateError::MissingBody(application.body.clone()))?;
        for index in indices {
            budget.visit()?;
            let generic = envelope.bodies[*index].generic.as_ref();
            let arity = generic.map_or(0, |generic| generic.parameters.len());
            if arity != application.arguments.len() {
                return Err(UpdateError::InvalidEvidence);
            }
            // A checked symbolic route is visited once in its owning binder scope.
            // Expanding f<T> -> f<Vec<T>> symbolically would invent an unbounded
            // specialization demand. Concrete historical applications still expand.
            let arguments = if application.arguments.iter().all(Ty::is_concrete) {
                application.arguments.clone()
            } else if generic.is_some() {
                parameters(generic)
            } else {
                return Err(UpdateError::UnresolvedSymbolicRoute(
                    application.body.clone(),
                ));
            };
            let key = (*index, arguments);
            if !self.seen.contains(&key) {
                budget.application(&BodyApplication {
                    body: application.body.clone(),
                    arguments: key.1.clone(),
                })?;
                self.seen.insert(key.clone());
                self.pending.push_back(key);
            }
        }
        Ok(())
    }

    fn current(
        &mut self,
        application: BodyApplication,
        envelope: &DemandEnvelope,
        budget: &mut Budget<'_>,
    ) -> Result<(), UpdateError> {
        if self.current_bodies.contains_key(&application.body) {
            return self.enqueue(application, envelope, budget);
        }
        let natives = self.find_natives(&application.body, None, budget)?;
        if !natives.is_empty() {
            for index in natives {
                self.native_arguments(index, &application.arguments, envelope, budget)?;
            }
            return Ok(());
        }
        self.requests.missing(Request::Shared(application), budget)
    }

    fn direct(
        &mut self,
        instance: ConcreteFunctionIdentity,
        envelope: &DemandEnvelope,
        budget: &mut Budget<'_>,
    ) -> Result<(), UpdateError> {
        budget.visit()?;
        if let Some(target) = self.targets.get(&instance) {
            if let Some(body) = self.current_bodies.get(target) {
                // Concrete environments come from SharedCall, not identity
                // arguments, which may belong to a specialized receiver instead.
                let arguments = parameters(body.requirement.generic.as_ref());
                return self.enqueue(
                    BodyApplication {
                        body: (*target).clone(),
                        arguments,
                    },
                    envelope,
                    budget,
                );
            }
        } else {
            // Checked generated adapters are exact body-owned routes, not named
            // current targets. Requiring a named target would invent recompilation
            // requests for an adapter already present in the candidate inventory.
            if let Some(body) = self.current_bodies.get(&instance)
                && matches!(&body.classification, BodyClassification::Adapter(origin) if origin == &instance)
            {
                return self.enqueue(
                    BodyApplication {
                        body: instance,
                        arguments: parameters(body.requirement.generic.as_ref()),
                    },
                    envelope,
                    budget,
                );
            }
            let natives = self.find_natives(&instance, None, budget)?;
            if !natives.is_empty() {
                for index in natives {
                    self.native(index, &TypeSubstitution::default(), envelope, budget)?;
                }
                return Ok(());
            }
        }
        if !instance.arguments.iter().all(Ty::is_concrete) {
            if let Some(indices) = self.bodies.get(&instance) {
                for index in indices {
                    budget.visit()?;
                    if let Some(generic) = &envelope.bodies[*index].generic {
                        let arguments = parameters(Some(generic));
                        return self.requests.missing(
                            Request::Shared(BodyApplication {
                                body: instance,
                                arguments,
                            }),
                            budget,
                        );
                    }
                }
            }
            return Err(UpdateError::UnresolvedSymbolicRoute(instance));
        }
        self.requests.missing(Request::Function(instance), budget)
    }

    fn call(
        &mut self,
        call: &CallRequirement,
        bindings: &TypeSubstitution<'_>,
        symbolic_scope: Option<&GenericBody>,
        envelope: &DemandEnvelope,
        budget: &mut Budget<'_>,
    ) -> Result<(), UpdateError> {
        budget.visit()?;
        if self.depth >= 64 {
            return Err(UpdateError::LimitExceeded);
        }
        self.depth += 1;
        let result = self.call_inner(call, bindings, symbolic_scope, envelope, budget);
        self.depth -= 1;
        result
    }

    fn call_inner(
        &mut self,
        call: &CallRequirement,
        bindings: &TypeSubstitution<'_>,
        symbolic_scope: Option<&GenericBody>,
        envelope: &DemandEnvelope,
        budget: &mut Budget<'_>,
    ) -> Result<(), UpdateError> {
        match call {
            CallRequirement::Direct(instance) => {
                let instance = apply_instance(instance, bindings, budget)?;
                // A checked template edge is instantiated by its concrete entry
                // applications, not by inventing specializations for binder symbols.
                if symbolic_scope.is_some() && !instance.arguments.iter().all(Ty::is_concrete) {
                    return Ok(());
                }
                self.direct(instance, envelope, budget)
            }
            CallRequirement::RetainedBody(application) => self.enqueue(
                BodyApplication {
                    body: application.body.clone(),
                    arguments: apply(&application.arguments, bindings, budget)?,
                },
                envelope,
                budget,
            ),
            CallRequirement::Shared(call) => {
                self.shared(call, bindings, symbolic_scope, envelope, budget)
            }
            CallRequirement::Interface(call) => {
                self.interface(call, bindings, symbolic_scope, envelope, budget)
            }
            CallRequirement::Native(import) => {
                let natives = self.find_natives(&import.instance, Some(import.binding), budget)?;
                if natives.is_empty() {
                    return Err(UpdateError::MissingNative(import.instance.clone()));
                }
                let mut matched = false;
                for index in natives {
                    if self.coverage.natives[index].import == import.as_ref() {
                        self.native(index, bindings, envelope, budget)?;
                        matched = true;
                    }
                }
                if matched {
                    Ok(())
                } else {
                    Err(UpdateError::InvalidEvidence)
                }
            }
            CallRequirement::Operation(operation) => {
                self.operation(operation, bindings, symbolic_scope, envelope, budget)
            }
            CallRequirement::Construction(construction) => {
                self.witness(construction, bindings, symbolic_scope, envelope, budget)
            }
            CallRequirement::Layout(ty) => {
                let ty = bindings.apply(ty, budget.cancel)?;
                if !ty.is_concrete() {
                    return if symbolic_scope.is_some() {
                        Ok(())
                    } else {
                        Err(UpdateError::MissingCoverage)
                    };
                }
                for layout in self.coverage.layouts {
                    budget.visit()?;
                    if *layout == ty {
                        return Ok(());
                    }
                }
                self.requests.missing(Request::Layout(ty), budget)
            }
            // This is not a complete target-set proof. The envelope still requires
            // every previously published target and checked route, independently of
            // this marker. Protection handles unknown reentry by freezing inventory.
            CallRequirement::Unknown(_) => Ok(()),
        }
    }

    fn shared(
        &mut self,
        call: &SharedCall,
        bindings: &TypeSubstitution<'_>,
        symbolic_scope: Option<&GenericBody>,
        envelope: &DemandEnvelope,
        budget: &mut Budget<'_>,
    ) -> Result<(), UpdateError> {
        // Check in the historical caller's binder scope: substituting only type
        // arguments would detach forwarded witnesses from their declared authority.
        let natives = if let CallableImplementation::Native(binding) = call.implementation {
            let natives = self.find_natives(&call.instance, Some(binding), budget)?;
            if natives.is_empty() {
                return Err(UpdateError::MissingNative(call.instance.clone()));
            }
            natives
        } else {
            vec![]
        };
        let contract = match call.implementation {
            CallableImplementation::Script => self
                .current_bodies
                .get(&call.instance)
                .map(|body| (body.requirement.generic.as_ref(), &body.signature)),
            CallableImplementation::Native(_) => None,
            _ => return Err(UpdateError::InvalidEvidence),
        };
        if let Some((generic, signature)) = contract {
            let generic = generic.ok_or(UpdateError::InvalidEvidence)?;
            let parameters = symbolic_scope.map_or(&[][..], |body| body.parameters.as_slice());
            let assumptions = symbolic_scope.map_or(&[][..], |body| body.bounds.as_slice());
            if !call.check(
                generic,
                signature,
                self.catalog,
                parameters,
                assumptions,
                budget.cancel,
            )? {
                budget.check()?;
                return Err(UpdateError::InvalidEvidence);
            }
        }
        for index in &natives {
            let import = self.coverage.natives[*index].import;
            let generic = import
                .generic
                .as_ref()
                .ok_or(UpdateError::InvalidEvidence)?;
            if !call.check(
                generic,
                &import.signature,
                self.catalog,
                symbolic_scope.map_or(&[], |body| body.parameters.as_slice()),
                symbolic_scope.map_or(&[], |body| body.bounds.as_slice()),
                budget.cancel,
            )? {
                return Err(UpdateError::InvalidEvidence);
            }
        }
        let arguments = apply(&call.arguments, bindings, budget)?;
        match call.implementation {
            CallableImplementation::Script => {
                let application = BodyApplication {
                    body: call.instance.clone(),
                    arguments,
                };
                if contract.is_some() {
                    self.current(application, envelope, budget)?;
                } else {
                    self.requests
                        .missing(Request::Shared(application), budget)?;
                }
            }
            CallableImplementation::Native(_) => {
                for index in natives {
                    self.native_arguments(index, &arguments, envelope, budget)?;
                }
            }
            _ => return Err(UpdateError::InvalidEvidence),
        }
        for operation in &call.operations {
            self.operation(operation, bindings, symbolic_scope, envelope, budget)?;
        }
        for construction in &call.constructions {
            self.witness(construction, bindings, symbolic_scope, envelope, budget)?;
        }
        Ok(())
    }

    fn interface(
        &mut self,
        call: &InterfaceCallContract,
        bindings: &TypeSubstitution<'_>,
        symbolic_scope: Option<&GenericBody>,
        envelope: &DemandEnvelope,
        budget: &mut Budget<'_>,
    ) -> Result<(), UpdateError> {
        let interface = bindings.apply_nominal(&call.interface, budget.cancel)?;
        let receiver = call
            .receiver
            .as_ref()
            .map(|ty| bindings.apply(ty, budget.cancel))
            .transpose()?;
        let arguments = apply(&call.arguments, bindings, budget)?;
        let concrete = Ty::Trait(interface.clone()).is_concrete()
            && receiver.as_ref().is_none_or(Ty::is_concrete)
            && arguments.iter().all(Ty::is_concrete);
        if !concrete && symbolic_scope.is_none() {
            return Err(UpdateError::MissingCoverage);
        }
        let mut found = false;
        if concrete {
            for method in self.coverage.methods {
                budget.visit()?;
                if method.slot != call.method_slot
                    || method.interface.declaration != interface.declaration
                    || method.interface.arguments != interface.arguments
                    || !interface
                        .associated_types
                        .iter()
                        .all(|(id, ty)| method.interface.associated_types.get(id) == Some(ty))
                    || receiver
                        .as_ref()
                        .is_some_and(|receiver| *receiver != method.receiver)
                {
                    continue;
                }
                if method.parameters.len() != arguments.len() {
                    return Err(UpdateError::InvalidEvidence);
                }
                let mut method_bindings = TypeSubstitution::default();
                for (parameter, argument) in method.parameters.iter().zip(&arguments) {
                    method_bindings.bind(&parameter.owner, parameter.position, argument);
                }
                self.current(
                    BodyApplication {
                        body: method.body.clone(),
                        arguments: apply(&method.arguments, &method_bindings, budget)?,
                    },
                    envelope,
                    budget,
                )?;
                found = true;
            }
            if !found {
                self.requests.missing(
                    Request::Interface(InterfaceRequest {
                        interface,
                        receiver,
                        method_slot: call.method_slot,
                        arguments,
                    }),
                    budget,
                )?;
            }
        }
        for operation in &call.operations {
            self.operation(operation, bindings, symbolic_scope, envelope, budget)?;
        }
        for construction in &call.constructions {
            self.witness(construction, bindings, symbolic_scope, envelope, budget)?;
        }
        Ok(())
    }

    fn construction(
        &mut self,
        instance: &ConcreteFunctionIdentity,
        bindings: &TypeSubstitution<'_>,
        symbolic_scope: Option<&GenericBody>,
        budget: &mut Budget<'_>,
    ) -> Result<(), UpdateError> {
        let applied = apply_instance(instance, bindings, budget)?;
        if !applied.arguments.iter().all(Ty::is_concrete) {
            return if symbolic_scope.is_some() {
                Ok(())
            } else {
                Err(UpdateError::UnresolvedSymbolicRoute(applied))
            };
        }
        for available in self.coverage.constructions {
            budget.visit()?;
            if *available == applied {
                return Ok(());
            }
        }
        self.requests
            .missing(Request::Construction(applied), budget)
    }
}

fn parameters(generic: Option<&GenericBody>) -> Vec<Ty> {
    generic.map_or_else(Vec::new, |generic| {
        generic
            .parameters
            .iter()
            .map(|parameter| parameter.as_type())
            .collect()
    })
}

fn apply(
    arguments: &[Ty],
    bindings: &TypeSubstitution<'_>,
    budget: &mut Budget<'_>,
) -> Result<Vec<Ty>, UpdateError> {
    nested(arguments.len())?;
    budget.visit()?;
    arguments
        .iter()
        .map(|ty| bindings.apply(ty, budget.cancel).map_err(Into::into))
        .collect()
}

fn apply_instance(
    instance: &ConcreteFunctionIdentity,
    bindings: &TypeSubstitution<'_>,
    budget: &mut Budget<'_>,
) -> Result<ConcreteFunctionIdentity, UpdateError> {
    Ok(ConcreteFunctionIdentity {
        declaration: instance.declaration,
        arguments: apply(&instance.arguments, bindings, budget)?,
    })
}
