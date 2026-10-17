//! Bounded trait matching, inherited constraints, defaults and associated-type normalization.

use crate::{
    aggregates::{
        AggregateCatalog,
        traits::{MethodDefault, MethodSignature},
    },
    declarations::Declarations,
    language::{semantics as traits, semantics::ProtocolSemantics},
    lower::LoweredModule,
    native::NativeBinding,
    resolver::resolved::ResolvedName,
    typeck::{
        GenericBounds, ModuleSignatures, associated, check::possibly_overlapping_impls,
        table::match_implementation,
    },
    types::{AssociatedTypeFamily, GenericParameterType, NominalType, TypeId, TypeSubstitution},
};
use kagari_common::{
    cancellation::{CancellationToken, Cancelled},
    identity::{
        DefinitionKind,
        table::{DefinitionId, DefinitionTable},
    },
};
use kagari_types::{
    declaration::conversion::ConversionAdapter,
    language::{Protocol, binding},
};
use std::{
    collections::{BTreeMap, HashSet},
    iter,
    sync::Arc,
};

/// Cancellation or resource exhaustion during bounded trait implementation search.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImplementationSearchError {
    /// Cooperative cancellation interrupted search.
    Cancelled,
    /// Candidate/depth or inheritance expansion budget was exhausted.
    LimitExceeded,
}

/// Explicit candidate/depth limits for one implementation lookup.
#[derive(Debug, Clone, Copy)]
pub struct ImplementationSearchLimits {
    /// Maximum candidate checks, including recursive requirements.
    pub max_checks: usize,
    /// Maximum nested implementation requirements.
    pub max_depth: usize,
}

/// Caller-owned assumptions, resource limits and cancellation for one implementation search.
#[derive(Clone, Copy)]
pub struct ImplementationSearchContext<'a> {
    /// Bounds available at the query site, including open generic receivers.
    pub assumptions: &'a GenericBounds,
    /// Candidate and recursive requirement limits for this query.
    pub limits: ImplementationSearchLimits,
    /// Cancellation shared with the calling analysis or lowering operation.
    pub cancel: &'a CancellationToken,
}

/// Per-search candidate/depth accounting, caller assumptions and guarded intrinsic-default exploration.
struct SearchBudget<'a> {
    definitions: &'a DefinitionTable,
    checks_left: usize,
    depth: usize,
    max_depth: usize,
    cancel: &'a CancellationToken,
    assumptions: &'a GenericBounds,
    defaults: HashSet<(Protocol, TypeId)>,
}

impl<'a> SearchBudget<'a> {
    fn new(
        definitions: &'a DefinitionTable,
        context: ImplementationSearchContext<'a>,
    ) -> Result<Self, ImplementationSearchError> {
        context
            .cancel
            .check()
            .map_err(|_| ImplementationSearchError::Cancelled)?;
        Ok(Self {
            definitions,
            checks_left: context.limits.max_checks,
            depth: 0,
            max_depth: context.limits.max_depth,
            cancel: context.cancel,
            assumptions: context.assumptions,
            defaults: HashSet::new(),
        })
    }

    fn check_candidate(&mut self) -> Result<(), ImplementationSearchError> {
        self.cancel
            .check()
            .map_err(|_| ImplementationSearchError::Cancelled)?;
        if self.checks_left == 0 {
            return Err(ImplementationSearchError::LimitExceeded);
        }
        self.checks_left -= 1;
        Ok(())
    }
}

/// A generic implementation pattern and the members selected when it applies.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImplementationSignature {
    /// Derived from the retained installation object, never a source identity.
    pub(crate) engine_owned: bool,
    /// Associated member definition to parameterized output implementation.
    pub associated_type_families: BTreeMap<DefinitionId, AssociatedTypeFamily>,
    /// Canonical semantic definition identity used as the catalog key.
    pub id: DefinitionId,
    /// Implemented trait application, possibly containing implementation binders.
    pub trait_type: NominalType,
    /// Receiver type pattern to match against the requested concrete/generic type.
    pub for_type: TypeId,
    /// Generic binders in declaration order, identified by owner and position.
    pub generic_params: Vec<GenericParameterType>,
    /// Checked generic constraints used during application/implementation selection.
    pub bounds: GenericBounds,
    /// Trait method identity to implementation method identity.
    pub methods: BTreeMap<DefinitionId, DefinitionId>,
}

impl AggregateCatalog {
    /// Standard protocol implementations are type-owned, so dependencies and callers
    /// cannot disagree because a downstream module adds a different override.
    pub fn standard_override_error(
        &self,
        definitions: &DefinitionTable,
        implementation: &ImplementationSignature,
    ) -> Option<&'static str> {
        let adapter = self
            .trait_(&implementation.trait_type.declaration)
            .and_then(|contract| contract.conversion_adapter.as_ref());
        let protocol =
            Protocol::from_definition(&implementation.trait_type.declaration, definitions);
        if matches!(adapter, Some(ConversionAdapter::Reverse { .. })) {
            return Some("Into and TryInto are derived from From and TryFrom");
        }
        if protocol == Some(Protocol::From)
            && implementation
                .trait_type
                .arguments
                .first()
                .is_some_and(|input| {
                    if input == &implementation.for_type {
                        return true;
                    }
                    match (input, &implementation.for_type) {
                        (TypeId::Generic(_), target) => !occurs_in_constructor(input, target),
                        (source, TypeId::Generic(_)) => {
                            !occurs_in_constructor(&implementation.for_type, source)
                        }
                        _ => possibly_overlapping_impls(input, &implementation.for_type),
                    }
                })
        {
            return Some("identity From<T> for T is supplied by the language");
        }
        if protocol == Some(Protocol::From)
            || matches!(adapter, Some(ConversionAdapter::Forward { .. }))
        {
            if matches!(implementation.for_type, TypeId::Host(_))
                || implementation
                    .trait_type
                    .arguments
                    .iter()
                    .any(|ty| matches!(ty, TypeId::Host(_)))
            {
                return Some("host conversion implementations are not supported");
            }
            let owned=iter::once(&implementation.for_type).chain(&implementation.trait_type.arguments).any(|ty| {
                matches!(ty,TypeId::NativeObject(n)|TypeId::Struct(n)|TypeId::Enum(n) if definitions.resolve(n.declaration).expect("scoped nominal type").module()==definitions.resolve(implementation.id).expect("scoped implementation").module())
            });
            return (!owned && !implementation.engine_owned).then_some("a conversion must belong to the defining module of its nominal source or destination");
        }
        let protocol = protocol?;
        if protocol.iteration() {
            let other = if protocol == Protocol::Iterator {
                Protocol::Iterable
            } else {
                Protocol::Iterator
            };
            if self.implementations.values().any(|candidate| {
                Some(candidate.trait_type.declaration) == other.definition(definitions)
                    && possibly_overlapping_impls(&candidate.for_type, &implementation.for_type)
            }) {
                return Some("Iterator already supplies identity Iterable");
            }
        }
        if protocol.host_implementable() {
            return None;
        }
        match &implementation.for_type {
            TypeId::NativeObject(nominal)
            | TypeId::Struct(nominal)
            | TypeId::Enum(nominal)
            | TypeId::Trait(nominal) => {
                if definitions
                    .resolve(nominal.declaration)
                    .expect("scoped nominal type")
                    .module()
                    != definitions
                        .resolve(implementation.id)
                        .expect("scoped implementation")
                        .module()
                {
                    return Some(
                        "standard protocol implementations must belong to the type's defining module",
                    );
                }
            }
            _ if !implementation.engine_owned => {
                return Some("language protocol implementations require a declared nominal type");
            }
            _ => {}
        }
        if !protocol.equality_protocol() {
            return None;
        }
        for required in [Protocol::PartialEq, Protocol::Eq] {
            if protocol == Protocol::PartialEq
                || protocol == Protocol::Eq && required == Protocol::Eq
            {
                continue;
            }
            if !matches!(
                required.nominal(definitions).map(|interface| self
                    .concrete_interface_implementation(
                        definitions,
                        &interface,
                        &implementation.for_type,
                        &implementation.bounds,
                        ImplementationSearchLimits {
                            max_checks: 4096,
                            max_depth: 64
                        },
                        &CancellationToken::default()
                    )),
                Some(Ok(Some(_)))
            ) {
                return Some(
                    "custom Eq requires explicit PartialEq; custom Hash requires explicit PartialEq and Eq under the same bounds",
                );
            }
        }
        None
    }

    /// Checks a standard protocol with 4,096 candidate checks and depth 64; an exhausted search returns false.
    pub fn standard_protocol_holds(
        &self,
        definitions: &DefinitionTable,
        protocol: Protocol,
        ty: &TypeId,
        assumptions: &GenericBounds,
    ) -> bool {
        let cancel = CancellationToken::default();
        let mut budget = SearchBudget {
            definitions,
            checks_left: 4096,
            depth: 0,
            max_depth: 64,
            cancel: &cancel,
            assumptions,
            defaults: HashSet::new(),
        };
        self.standard_holds(protocol, ty, &mut HashSet::new(), &mut budget)
            .unwrap_or(false)
    }

    fn standard_holds(
        &self,
        protocol: Protocol,
        ty: &TypeId,
        visiting: &mut HashSet<(NominalType, TypeId)>,
        budget: &mut SearchBudget<'_>,
    ) -> Result<bool, ImplementationSearchError> {
        budget.check_candidate()?;
        let interface = protocol.nominal(budget.definitions);
        if interface
            .as_ref()
            .is_some_and(|interface| visiting.contains(&(interface.clone(), ty.clone())))
        {
            return Ok(false);
        }
        if let TypeId::Trait(receiver_interface) = ty
            && self
                .trait_closure(receiver_interface, ty, budget.cancel)?
                .iter()
                .any(|parent| {
                    interface
                        .as_ref()
                        .is_some_and(|required| parent.satisfies(required))
                })
        {
            return Ok(true);
        }
        if budget.defaults.contains(&(protocol, ty.clone())) {
            return Ok(true);
        }
        if budget.depth >= budget.max_depth {
            return Err(ImplementationSearchError::LimitExceeded);
        }
        if let Some(constraints) = budget.assumptions.get(ty) {
            for constraint in constraints {
                let nominal = constraint;
                if Some(nominal) == interface.as_ref()
                    || protocol == Protocol::PartialEq
                        && Protocol::Eq.nominal(budget.definitions).as_ref() == Some(nominal)
                {
                    return Ok(true);
                }
            }
        }
        budget.depth += 1;
        let result = (|| {
            if matches!(
                ty,
                TypeId::Array(_)
                    | TypeId::Trait(_)
                    | TypeId::NativeObject(_)
                    | TypeId::Struct(_)
                    | TypeId::Enum(_)
                    | TypeId::Map { .. }
                    | TypeId::Set(_, _)
            ) {
                for implementation in self.implementations.values() {
                    if let Some(interface) = &interface
                        && self
                            .implementation_matches(
                                implementation,
                                interface,
                                ty,
                                visiting,
                                budget,
                            )?
                            .is_some()
                    {
                        return Ok(true);
                    }
                }
                if matches!(protocol, Protocol::Eq | Protocol::Hash)
                    && let Some(equality) = Protocol::PartialEq.nominal(budget.definitions)
                {
                    for implementation in self.implementations.values() {
                        if self
                            .implementation_matches(
                                implementation,
                                &equality,
                                ty,
                                visiting,
                                budget,
                            )?
                            .is_some()
                        {
                            return Ok(false);
                        }
                    }
                }
            }
            if protocol == Protocol::Iterable
                && self.standard_holds(Protocol::Iterator, ty, visiting, budget)?
            {
                return Ok(true);
            }
            if !protocol.equality_protocol() {
                return Ok(traits::intrinsic_holds(
                    budget.definitions,
                    protocol,
                    ty,
                    None,
                    budget.assumptions,
                ));
            }
            let members = match ty {
                TypeId::Tuple(members) => Some(members.clone()),
                TypeId::Enum(n)
                    if binding::member_comparison(&n.declaration, budget.definitions) =>
                {
                    if let Some(declaration) = self.enumeration(&n.declaration) {
                        let substitution = declaration
                            .generic_params
                            .iter()
                            .cloned()
                            .zip(n.arguments.iter().cloned())
                            .collect();
                        Some(
                            declaration
                                .variants
                                .iter()
                                .flat_map(|v| {
                                    v.payload.iter().map(|ty| ty.instantiate(&substitution))
                                })
                                .collect(),
                        )
                    } else {
                        return Ok(false);
                    }
                }
                _ => None,
            };
            if let Some(members) = members {
                budget.defaults.insert((protocol, ty.clone()));
                let result = (|| {
                    for member in members {
                        if !self.standard_holds(protocol, &member, visiting, budget)? {
                            return Ok(false);
                        }
                    }
                    Ok(true)
                })();
                budget.defaults.remove(&(protocol, ty.clone()));
                result
            } else {
                Ok(traits::intrinsic_holds(
                    budget.definitions,
                    protocol,
                    ty,
                    None,
                    budget.assumptions,
                ))
            }
        })();
        budget.depth -= 1;
        result
    }

    /// Normalizes associated projections using available checked implementation/family facts.
    pub fn normalize_type(&self, definitions: &DefinitionTable, ty: &TypeId) -> TypeId {
        associated::normalize(ty, &|interface, receiver, member, arguments| {
            if arguments.is_empty()
                && !matches!(receiver, TypeId::SelfType(_))
                && Some(*member)
                    == definitions.lookup_child(
                        interface.declaration,
                        DefinitionKind::AssociatedType,
                        "Error",
                        0,
                    )
                && let Some((required, target)) =
                    traits::conversion_requirement(interface, receiver, Some(self))
            {
                return Some(TypeId::Projection {
                    receiver: Box::new(target),
                    member: definitions.lookup_child(
                        required.declaration,
                        DefinitionKind::AssociatedType,
                        "Error",
                        0,
                    )?,
                    interface: Box::new(required),
                    arguments: vec![],
                });
            }

            if arguments.is_empty()
                && let Some(kind) = Protocol::from_definition(&interface.declaration, definitions)
                && kind.iteration()
                && let Some(outputs) = traits::iteration_outputs(
                    definitions,
                    kind,
                    receiver,
                    Some(self),
                    &Default::default(),
                )
                && let Some(output) = outputs.get(member)
            {
                return Some(output.clone());
            }
            if arguments.is_empty()
                && Some(*member)
                    == definitions.lookup_child(
                        interface.declaration,
                        DefinitionKind::AssociatedType,
                        "Output",
                        0,
                    )
                && let Some(output) = traits::intrinsic_output(definitions, interface, receiver)
            {
                return Some(output);
            }
            if let TypeId::Trait(actual) = receiver {
                return self
                    .interface_closure(definitions, actual, receiver, &Default::default())
                    .ok()?
                    .into_iter()
                    .find(|parent| parent.satisfies(interface))?
                    .associated_types
                    .get(member)
                    .cloned();
            }
            let mut matches = self.implementations.values().filter_map(|implementation| {
                let substitution = match_implementation(
                    &implementation.trait_type,
                    interface,
                    &implementation.for_type,
                    receiver,
                    &implementation.generic_params,
                )?;
                if !arguments.is_empty() {
                    return implementation
                        .associated_type_families
                        .get(member)?
                        .apply(&substitution, arguments);
                }
                Some(
                    implementation
                        .trait_type
                        .associated_types
                        .get(member)?
                        .instantiate(&substitution),
                )
            });
            let result = matches.next()?;
            matches.next().is_none().then_some(result)
        })
    }

    /// Visits recorded implementation contracts in definition-key order.
    pub fn implementations(&self) -> impl Iterator<Item = &ImplementationSignature> {
        self.implementations.values().map(AsRef::as_ref)
    }

    pub(crate) fn overlapping_implementations(
        &self,
        definitions: &DefinitionTable,
    ) -> Vec<(&ImplementationSignature, &ImplementationSignature)> {
        let mut by_trait: BTreeMap<&DefinitionId, Vec<&ImplementationSignature>> = BTreeMap::new();
        let mut overlaps = Vec::new();
        let mut implementations = self.implementations.values().collect::<Vec<_>>();
        implementations.sort_by(|left, right| {
            definitions
                .compare(left.id, right.id)
                .expect("scoped implementation identities")
        });
        for implementation in implementations {
            let previous = by_trait
                .entry(&implementation.trait_type.declaration)
                .or_default();
            if let Some(conflict) = previous.iter().copied().find(|candidate| {
                (candidate.trait_type.arguments == implementation.trait_type.arguments
                    || !candidate
                        .trait_type
                        .arguments
                        .iter()
                        .all(TypeId::is_concrete)
                    || !implementation
                        .trait_type
                        .arguments
                        .iter()
                        .all(TypeId::is_concrete))
                    && possibly_overlapping_impls(&candidate.for_type, &implementation.for_type)
            }) {
                overlaps.push((conflict, implementation.as_ref()));
            }
            previous.push(implementation);
        }
        overlaps
    }

    pub(crate) fn add_implementations(
        &mut self,
        lowered: &LoweredModule,
        declarations: &Declarations,
        signatures: &ModuleSignatures,
        cancel: &CancellationToken,
    ) -> Result<(), Cancelled> {
        for (id, trait_type, for_type, generic_params, bounds, methods) in
            signatures.type_table().implementation_entries()
        {
            cancel.check()?;
            let mut method_identities = BTreeMap::new();
            for (trait_method, function) in methods {
                cancel.check()?;
                let Some(declaration) = declarations.definition(ResolvedName::Function(*function))
                else {
                    continue;
                };
                method_identities.insert(*trait_method, *declaration);
            }
            self.implementations.insert(
                *id,
                Arc::new(ImplementationSignature {
                    engine_owned: lowered.registered_native_api,
                    associated_type_families: signatures
                        .type_table()
                        .associated_type_families
                        .iter()
                        .filter_map(|(member, family)| {
                            let definitions = declarations.definitions();
                            if definitions.parent(*member).ok()? != Some(*id) {
                                return None;
                            }
                            let name = definitions.resolve(*member).ok()?.last()?.name;
                            Some((
                                definitions.lookup_child(
                                    trait_type.declaration,
                                    DefinitionKind::AssociatedType,
                                    name,
                                    0,
                                )?,
                                family.clone(),
                            ))
                        })
                        .collect(),
                    id: *id,
                    trait_type: trait_type.clone(),
                    for_type: for_type.clone(),
                    generic_params: generic_params.to_vec(),
                    bounds: bounds.clone(),
                    methods: method_identities,
                }),
            );
        }
        Ok(())
    }

    /// Finds a unique matching method and implementation arguments, including script defaults.
    /// `definitions` must retain the catalog's scope and its registered implementation members.
    /// Returns `None` for no matching method or ambiguity.
    ///
    /// # Errors
    ///
    /// Returns caller cancellation or candidate/depth exhaustion, including cancellation
    /// when the catalog has no candidates.
    pub fn implementation_method(
        &self,
        definitions: &DefinitionTable,
        method: &DefinitionId,
        trait_type: &NominalType,
        receiver: &TypeId,
        context: ImplementationSearchContext<'_>,
    ) -> Result<Option<(DefinitionId, Vec<TypeId>)>, ImplementationSearchError> {
        let mut budget = SearchBudget::new(definitions, context)?;
        let mut selected = None;
        for implementation in self.implementations.values() {
            let Some(matched) = self.implementation_matches(
                implementation,
                trait_type,
                receiver,
                &mut HashSet::new(),
                &mut budget,
            )?
            else {
                continue;
            };
            let Some(arguments) = implementation
                .generic_params
                .iter()
                .map(|parameter| matched.get(parameter).cloned())
                .collect::<Option<Vec<_>>>()
            else {
                continue;
            };
            let Some(target) = implementation.methods.get(method).cloned().or_else(|| {
                self.trait_method(method)
                    .filter(|method| method.default == Some(MethodDefault::Script))
                    .map(|_| implementation_member(definitions, implementation.id, *method))
            }) else {
                continue;
            };
            if selected.is_some() {
                return Ok(None);
            }
            selected = Some((target, arguments));
        }
        Ok(selected)
    }

    /// An omitted method has the same impl-owned identity as an explicit method;
    /// its script body or symbolic native application remains owned by the trait module.
    /// Foreign or absent targets in `definitions` do not select a default.
    pub fn default_method(
        &self,
        definitions: &DefinitionTable,
        target: &DefinitionId,
    ) -> Option<(&ImplementationSignature, &MethodSignature)> {
        let owner = definitions.parent(*target).ok()??;
        let name = definitions.resolve(*target).ok()?.last()?;
        let implementation = self.implementation_signature(&owner)?;
        let method = self
            .trait_(&implementation.trait_type.declaration)?
            .methods
            .iter()
            .find(|method| {
                definitions
                    .resolve(method.id)
                    .ok()
                    .and_then(|view| view.last())
                    == Some(name)
            })?;
        (matches!(
            method.default,
            Some(MethodDefault::Script | MethodDefault::Native(NativeBinding::Default(_)))
        ) && !implementation.methods.contains_key(&method.id))
        .then_some((implementation, method))
    }

    /// Collects selected implementation/default method identities for a recorded implementation.
    /// `definitions` must retain the catalog's scope and its registered implementation members.
    pub fn implementation_methods(
        &self,
        definitions: &DefinitionTable,
        implementation: &ImplementationSignature,
    ) -> Vec<DefinitionId> {
        self.trait_(&implementation.trait_type.declaration)
            .into_iter()
            .flat_map(|contract| &contract.methods)
            .filter_map(|method| {
                implementation.methods.get(&method.id).cloned().or_else(|| {
                    (matches!(
                        method.default,
                        Some(
                            MethodDefault::Script
                                | MethodDefault::Native(NativeBinding::Default(_))
                        )
                    ))
                    .then(|| implementation_member(definitions, implementation.id, method.id))
                })
            })
            .collect()
    }

    /// Selects a concrete implementation identity and applied arguments under explicit search limits.
    ///
    /// Returns `None` when no unique admissible concrete implementation is selected.
    ///
    /// # Errors
    ///
    /// Returns cancellation or candidate/depth exhaustion.
    pub fn concrete_interface_implementation(
        &self,
        definitions: &DefinitionTable,
        trait_type: &NominalType,
        receiver: &TypeId,
        assumptions: &GenericBounds,
        limits: ImplementationSearchLimits,
        cancel: &CancellationToken,
    ) -> Result<Option<(DefinitionId, Vec<TypeId>)>, ImplementationSearchError> {
        let mut budget = SearchBudget::new(
            definitions,
            ImplementationSearchContext {
                assumptions,
                limits,
                cancel,
            },
        )?;
        let mut selected = None;
        for implementation in self.implementations.values() {
            if let Some(matched) = self.implementation_matches(
                implementation,
                trait_type,
                receiver,
                &mut HashSet::new(),
                &mut budget,
            )? {
                if selected.is_some() {
                    return Ok(None);
                }
                let Some(arguments) = implementation
                    .generic_params
                    .iter()
                    .map(|parameter| matched.get(parameter).cloned())
                    .collect::<Option<Vec<_>>>()
                else {
                    return Ok(None);
                };
                selected = Some((implementation.id, arguments));
            }
        }
        Ok(selected)
    }

    /// Counts matching implementations under caller-provided candidate/depth limits.
    ///
    /// # Errors
    ///
    /// Returns cancellation or exhausted search limits instead of treating them as no match.
    pub fn implementation_count_bounded(
        &self,
        definitions: &DefinitionTable,
        trait_type: &NominalType,
        receiver: &TypeId,
        context: ImplementationSearchContext<'_>,
    ) -> Result<usize, ImplementationSearchError> {
        let mut budget = SearchBudget::new(definitions, context)?;
        let mut count = 0;
        for implementation in self.implementations.values() {
            if self
                .implementation_matches(
                    implementation,
                    trait_type,
                    receiver,
                    &mut HashSet::new(),
                    &mut budget,
                )?
                .is_some()
            {
                count += 1;
                if count == 2 {
                    break;
                }
            }
        }
        if count == 0 {
            let intrinsic = if trait_type.arguments.is_empty()
                && trait_type.associated_types.is_empty()
                && let Some(protocol) =
                    Protocol::from_definition(&trait_type.declaration, definitions)
                && protocol.equality_protocol()
            {
                self.standard_holds(protocol, receiver, &mut HashSet::new(), &mut budget)?
            } else {
                traits::intrinsic_applies(
                    definitions,
                    trait_type,
                    receiver,
                    Some(self),
                    context.assumptions,
                )
            };
            count = usize::from(intrinsic);
        }
        context
            .cancel
            .check()
            .map_err(|_| ImplementationSearchError::Cancelled)?;
        Ok(count)
    }

    fn implementation_matches(
        &self,
        implementation: &ImplementationSignature,
        trait_type: &NominalType,
        receiver: &TypeId,
        visiting: &mut HashSet<(NominalType, TypeId)>,
        budget: &mut SearchBudget<'_>,
    ) -> Result<Option<TypeSubstitution>, ImplementationSearchError> {
        budget.check_candidate()?;
        // Operation requirements may be raised from executable value types.
        // Compare canonical nested values while retaining exact root constraints.
        let receiver = self.canonical_value_type(budget.definitions, receiver);
        let Some(matched) = match_implementation(
            &self.canonical_interface_application(budget.definitions, &implementation.trait_type),
            &self.canonical_interface_application(budget.definitions, trait_type),
            &self.canonical_value_type(budget.definitions, &implementation.for_type),
            &receiver,
            &implementation.generic_params,
        ) else {
            return Ok(None);
        };
        if implementation.engine_owned
            && !implementation.accepts_native_receiver(&receiver, &matched)
        {
            return Ok(None);
        }
        let key = (trait_type.clone(), receiver.clone());
        if !visiting.insert(key.clone()) {
            return Ok(None);
        }
        let holds = (|| {
            for (parameter, constraints) in &implementation.bounds {
                let actual =
                    self.normalize_type(budget.definitions, &parameter.instantiate(&matched));
                for constraint in constraints {
                    budget
                        .cancel
                        .check()
                        .map_err(|_| ImplementationSearchError::Cancelled)?;
                    let satisfied = {
                        let required = constraint;

                        if budget.depth >= budget.max_depth {
                            return Err(ImplementationSearchError::LimitExceeded);
                        }
                        let required = required.instantiate(&matched);
                        if budget.assumptions.get(&actual).is_some_and(|bounds| {
                            bounds
                                .iter()
                                .any(|available| available.satisfies(&required))
                        }) {
                            continue;
                        }
                        if required.arguments.is_empty()
                            && required.associated_types.is_empty()
                            && let Some(protocol) =
                                Protocol::from_definition(&required.declaration, budget.definitions)
                            && protocol.equality_protocol()
                        {
                            // Equality depends on explicit implementations
                            // and core enum payloads. A catalog-free
                            // intrinsic answer cannot establish this bound.
                            if self.standard_holds(protocol, &actual, visiting, budget)? {
                                continue;
                            }
                            return Ok(false);
                        }
                        if traits::intrinsic_applies(
                            budget.definitions,
                            &required,
                            &actual,
                            None,
                            budget.assumptions,
                        ) {
                            continue;
                        }
                        if required.arguments.is_empty()
                            && required.associated_types.is_empty()
                            && let Some(protocol) =
                                Protocol::from_definition(&required.declaration, budget.definitions)
                            && self.standard_holds(protocol, &actual, visiting, budget)?
                        {
                            continue;
                        }
                        budget.depth += 1;
                        let found = (|| {
                            for candidate in self.implementations.values() {
                                if self
                                    .implementation_matches(
                                        candidate, &required, &actual, visiting, budget,
                                    )?
                                    .is_some()
                                {
                                    return Ok(true);
                                }
                            }
                            if let Some(iterator) =
                                traits::iterator_requirement(budget.definitions, &required, &actual)
                            {
                                if budget.assumptions.get(&actual).is_some_and(|bounds| {
                                    bounds.iter().any(|n| n.satisfies(&iterator))
                                }) {
                                    return Ok(true);
                                }
                                for candidate in self.implementations.values() {
                                    if self
                                        .implementation_matches(
                                            candidate, &iterator, &actual, visiting, budget,
                                        )?
                                        .is_some()
                                    {
                                        return Ok(true);
                                    }
                                }
                            }
                            Ok(false)
                        })();
                        budget.depth -= 1;
                        found?
                    };
                    if !satisfied {
                        return Ok(false);
                    }
                }
            }
            Ok(true)
        })();
        visiting.remove(&key);
        Ok(holds?.then_some(matched))
    }
}

// From<T> for Wrapper<T> cannot overlap identity: T = Wrapper<T> has no finite solution.
// Associated projections are not injective constructors and provide no such proof.
fn occurs_in_constructor(parameter: &TypeId, ty: &TypeId) -> bool {
    let mut pending = vec![ty];
    while let Some(ty) = pending.pop() {
        if ty == parameter {
            return true;
        }
        match ty {
            TypeId::NativeObject(n) | TypeId::Struct(n) | TypeId::Enum(n) => {
                pending.extend(&n.arguments)
            }
            TypeId::Tuple(items) => pending.extend(items),
            TypeId::Array(item)
            | TypeId::Set(item, _)
            | TypeId::Iter(item)
            | TypeId::Range(item, _) => pending.push(item),
            TypeId::Map { key, value, .. } => pending.extend([key.as_ref(), value.as_ref()]),
            TypeId::Function { params, result } => {
                pending.extend(params);
                pending.push(result);
            }
            _ => {}
        }
    }
    false
}

/// Selection only consumes identities reserved during signature preparation.
fn implementation_member(
    definitions: &DefinitionTable,
    implementation: DefinitionId,
    method: DefinitionId,
) -> DefinitionId {
    let segment = definitions
        .resolve(method)
        .expect("scoped trait method")
        .last()
        .expect("method segment");
    definitions
        .lookup_child(
            implementation,
            segment.kind,
            segment.name,
            segment.occurrence,
        )
        .expect("implementation member registered before aggregate publication")
}

#[cfg(test)]
mod search_tests;

mod mapping;

impl AggregateCatalog {
    /// Borrows one recorded implementation by canonical identity.
    pub fn implementation_signature(&self, id: &DefinitionId) -> Option<&ImplementationSignature> {
        self.implementations.get(id).map(AsRef::as_ref)
    }
}
