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
        GenericBounds, ModuleSignatures, associated,
        check::possibly_overlapping_impls,
        constraints::type_satisfies_standard_constraint,
        table::{ConstraintTarget, match_implementation},
    },
    types::{AssociatedTypeFamily, GenericParameterType, NominalType, TypeId, TypeSubstitution},
};
use kagari_common::{
    cancellation::{CancellationToken, Cancelled},
    identity::{
        DefinitionKind,
        reference::DefinitionReference,
        table::{DefinitionId, DefinitionTable},
    },
};
use kagari_types::{declaration::conversion::ConversionAdapter, language::Protocol};
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

impl SearchBudget<'_> {
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
pub struct ImplementationSignature<I: DefinitionReference = DefinitionId> {
    /// Derived from the retained installation object, never a source identity.
    pub(crate) engine_owned: bool,
    /// Associated member definition to parameterized output implementation.
    pub associated_type_families: BTreeMap<I, AssociatedTypeFamily<I>>,
    /// Canonical semantic definition identity used as the catalog key.
    pub id: I,
    /// Implemented trait application, possibly containing implementation binders.
    pub trait_type: NominalType<I>,
    /// Receiver type pattern to match against the requested concrete/generic type.
    pub for_type: TypeId<I>,
    /// Generic binders in declaration order, identified by owner and position.
    pub generic_params: Vec<GenericParameterType<I>>,
    /// Checked generic constraints used during application/implementation selection.
    pub bounds: GenericBounds<I>,
    /// Trait method identity to implementation method identity.
    pub methods: BTreeMap<I, I>,
}

impl AggregateCatalog {
    /// Standard protocol implementations are type-owned, so dependencies and callers
    /// cannot disagree because a downstream module adds a different override.
    pub fn standard_override_error(
        &self,
        definitions: &DefinitionTable,
        implementation: &ImplementationSignature,
    ) -> Option<&'static str> {
        if self
            .trait_(&implementation.trait_type.declaration)
            .is_some_and(|contract| contract.storage_access.is_some())
        {
            return match &implementation.for_type {
                TypeId::NativeObject(nominal) | TypeId::Struct(nominal) | TypeId::Enum(nominal) => {
                    (definitions
                        .resolve(nominal.declaration)
                        .expect("scoped nominal type")
                        .module()
                        != definitions
                            .resolve(implementation.id)
                            .expect("scoped implementation")
                            .module())
                    .then_some(
                        "storage interface implementations must belong to their nominal type",
                    )
                }
                TypeId::Array(_) | TypeId::Map { .. } | TypeId::Set(_, _)
                    if implementation.engine_owned =>
                {
                    None
                }
                _ => Some("storage interface implementations require declared object storage"),
            };
        }
        let adapter = self
            .trait_(&implementation.trait_type.declaration)
            .and_then(|contract| contract.conversion_adapter.as_ref());
        let protocol =
            Protocol::from_reference(&implementation.trait_type.declaration, Some(definitions));
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
        let (TypeId::NativeObject(nominal) | TypeId::Struct(nominal) | TypeId::Enum(nominal)) =
            &implementation.for_type
        else {
            return (!implementation.engine_owned)
                .then_some("language protocol implementations require a declared nominal type");
        };
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
        if let TypeId::Trait(interface) = ty
            && self
                .trait_(&interface.declaration)
                .is_some_and(|contract| contract.storage_access.is_some())
            && (protocol.equality_protocol() || protocol == Protocol::Debug)
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
                if let ConstraintTarget::Trait(nominal) = constraint
                    && (Some(nominal) == interface.as_ref()
                        || protocol == Protocol::PartialEq
                            && Protocol::Eq.nominal(budget.definitions).as_ref() == Some(nominal))
                {
                    return Ok(true);
                }
            }
        }
        budget.depth += 1;
        let result = (|| {
            if matches!(
                ty,
                TypeId::NativeObject(_) | TypeId::Struct(_) | TypeId::Enum(_)
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
                TypeId::Enum(n) => {
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
                && let Some(kind) =
                    Protocol::from_reference(&interface.declaration, Some(definitions))
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
            let script = self.implementations.values().filter_map(|implementation| {
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
            let host = self
                .host_implementations
                .iter()
                .filter_map(|(applied, ty)| {
                    (ty == receiver && applied.satisfies(interface))
                        .then(|| applied.associated_types.get(member).cloned())
                        .flatten()
                });
            let mut matches = script.chain(host);
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

    /// Finds a matching method identity and implementation arguments, including applicable defaults.
    /// `definitions` must retain the catalog's scope and its registered implementation members.
    pub fn implementation_method(
        &self,
        definitions: &DefinitionTable,
        method: &DefinitionId,
        trait_type: &NominalType,
        receiver: &TypeId,
    ) -> Option<(DefinitionId, Vec<TypeId>)> {
        let cancel = CancellationToken::default();
        let mut budget = SearchBudget {
            definitions,
            checks_left: usize::MAX,
            depth: 0,
            max_depth: usize::MAX,
            cancel: &cancel,
            assumptions: &Default::default(),
            defaults: HashSet::new(),
        };
        let mut matches = self.implementations.values().filter_map(|implementation| {
            let matched = self
                .implementation_matches(
                    implementation,
                    trait_type,
                    receiver,
                    &mut HashSet::new(),
                    &mut budget,
                )
                .ok()??;
            let arguments = implementation
                .generic_params
                .iter()
                .map(|parameter| matched.get(parameter).cloned())
                .collect::<Option<Vec<_>>>()?;
            let target = implementation.methods.get(method).cloned().or_else(|| {
                self.trait_method(method)
                    .filter(|method| method.default == Some(MethodDefault::Script))
                    .map(|_| implementation_member(definitions, implementation.id, *method))
            })?;
            Some((target, arguments))
        });
        let result = matches.next()?;
        matches.next().is_none().then_some(result)
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

    /// Counts matching implementations using maximal size/depth bounds and default cancellation; search failure maps to zero.
    pub fn implementation_count(
        &self,
        definitions: &DefinitionTable,
        trait_type: &NominalType,
        receiver: &TypeId,
    ) -> usize {
        self.implementation_count_bounded(
            definitions,
            trait_type,
            receiver,
            usize::MAX,
            usize::MAX,
            &CancellationToken::default(),
        )
        .unwrap_or(0)
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
        let mut budget = SearchBudget {
            definitions,
            checks_left: limits.max_checks,
            depth: 0,
            max_depth: limits.max_depth,
            cancel,
            assumptions,
            defaults: HashSet::new(),
        };
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
        max_checks: usize,
        max_depth: usize,
        cancel: &CancellationToken,
    ) -> Result<usize, ImplementationSearchError> {
        let mut budget = SearchBudget {
            definitions,
            checks_left: max_checks,
            depth: 0,
            max_depth,
            cancel,
            assumptions: &Default::default(),
            defaults: HashSet::new(),
        };
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
        if count == 0
            && traits::intrinsic_applies(
                definitions,
                trait_type,
                receiver,
                Some(self),
                &Default::default(),
            )
        {
            count = 1;
        }
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
        let Some(matched) = match_implementation(
            &implementation.trait_type,
            trait_type,
            &implementation.for_type,
            receiver,
            &implementation.generic_params,
        ) else {
            return Ok(None);
        };
        if implementation.engine_owned
            && !implementation.accepts_native_receiver(receiver, &matched)
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
                    let satisfied = match constraint {
                        ConstraintTarget::Standard(standard) => type_satisfies_standard_constraint(
                            budget.definitions,
                            &actual,
                            *standard,
                            budget.assumptions,
                            Some(self),
                        ),
                        ConstraintTarget::Trait(required) => {
                            if budget.depth >= budget.max_depth {
                                return Err(ImplementationSearchError::LimitExceeded);
                            }
                            let required = required.instantiate(&matched);
                            if budget.assumptions.get(&actual).is_some_and(|bounds| {
                                bounds.iter().any(|bound| {
                                    matches!(bound, ConstraintTarget::Trait(available)
                                        if available.satisfies(&required))
                                })
                            }) {
                                continue;
                            }
                            if budget.assumptions.get(&actual).is_some_and(|bounds| bounds.iter().any(|b| matches!(b, ConstraintTarget::Trait(t) if t.satisfies(&required)))) { continue; }
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
                                && let Some(protocol) = Protocol::from_reference(
                                    &required.declaration,
                                    Some(budget.definitions),
                                )
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
                                if let Some(iterator) = traits::iterator_requirement(
                                    budget.definitions,
                                    &required,
                                    &actual,
                                ) {
                                    if budget.assumptions.get(&actual).is_some_and(|bounds| bounds.iter().any(|b|matches!(b,ConstraintTarget::Trait(n) if n.satisfies(&iterator)))) {return Ok(true);}
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
                        }
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
mod search_tests {
    use super::*;
    use crate::{typeck::table::ConstraintTarget, types::TypeId};
    use kagari_common::identity::{
        DefinitionKind, ModuleIdentity, PackageId, map::DefinitionContext,
    };
    use kagari_types::scalar::BuiltinType;

    fn definition(context: &DefinitionContext, kind: DefinitionKind, name: &str) -> DefinitionId {
        let root = context
            .intern_root(&ModuleIdentity {
                package: PackageId("pkg".into()),
                path: vec!["module".into()],
            })
            .unwrap();
        context.intern_child(root, kind, name, 0).unwrap()
    }

    #[test]
    fn bounded_trait_search_reuses_generic_matching_and_stops_growth() {
        let context = DefinitionContext::new().unwrap();
        let marker = definition(&context, DefinitionKind::Trait, "Marker");
        let owner = definition(&context, DefinitionKind::Impl, "");
        let parameter = GenericParameterType {
            owner: owner.clone(),
            position: 0,
            name: "T".into(),
        };
        let applied = |argument| NominalType {
            associated_types: Default::default(),
            declaration: marker.clone(),
            arguments: vec![argument],
        };
        let signature = ImplementationSignature {
            engine_owned: false,
            associated_type_families: Default::default(),
            id: owner,
            trait_type: applied(TypeId::Generic(parameter.clone())),
            for_type: TypeId::Generic(parameter.clone()),
            generic_params: vec![parameter.clone()],
            bounds: Default::default(),
            methods: Default::default(),
        };
        let catalog = AggregateCatalog {
            implementations: [(signature.id.clone(), Arc::new(signature.clone()))].into(),
            ..Default::default()
        };
        let actual = TypeId::Builtin(BuiltinType::I32);
        let required = applied(actual.clone());
        let cancel = CancellationToken::default();
        assert_eq!(
            catalog.implementation_count_bounded(
                &context.snapshot(),
                &required,
                &actual,
                1,
                4,
                &cancel
            ),
            Ok(1)
        );
        assert_eq!(
            catalog.implementation_count_bounded(
                &context.snapshot(),
                &required,
                &actual,
                0,
                4,
                &cancel
            ),
            Err(ImplementationSearchError::LimitExceeded)
        );
        cancel.cancel();
        assert_eq!(
            catalog.implementation_count_bounded(
                &context.snapshot(),
                &required,
                &actual,
                1,
                4,
                &cancel
            ),
            Err(ImplementationSearchError::Cancelled)
        );

        let mut chained = signature;
        let next = definition(&context, DefinitionKind::Trait, "Next");
        chained.bounds.insert(
            TypeId::Generic(parameter.clone()),
            vec![ConstraintTarget::Trait(NominalType {
                associated_types: Default::default(),
                declaration: next.clone(),
                arguments: vec![TypeId::Generic(parameter.clone())],
            })],
        );
        let next_parameter = GenericParameterType {
            owner: definition(&context, DefinitionKind::Impl, "next"),
            position: 0,
            name: "U".into(),
        };
        let next_signature = ImplementationSignature {
            engine_owned: false,
            associated_type_families: Default::default(),
            id: next_parameter.owner.clone(),
            trait_type: NominalType {
                associated_types: Default::default(),
                declaration: next,
                arguments: vec![TypeId::Generic(next_parameter.clone())],
            },
            for_type: TypeId::Generic(next_parameter.clone()),
            generic_params: vec![next_parameter.clone()],
            bounds: Default::default(),
            methods: Default::default(),
        };
        let catalog = AggregateCatalog {
            implementations: [chained, next_signature]
                .into_iter()
                .map(|signature| (signature.id.clone(), Arc::new(signature)))
                .collect(),
            ..Default::default()
        };
        assert_eq!(
            catalog.implementation_count_bounded(
                &context.snapshot(),
                &required,
                &actual,
                100,
                0,
                &CancellationToken::default(),
            ),
            Err(ImplementationSearchError::LimitExceeded)
        );
        assert_eq!(
            catalog.implementation_count_bounded(
                &context.snapshot(),
                &required,
                &actual,
                100,
                1,
                &CancellationToken::default(),
            ),
            Ok(1)
        );
    }
}

mod mapping;

impl<I: DefinitionReference> AggregateCatalog<I> {
    /// Borrows one recorded implementation by canonical identity.
    pub fn implementation_signature(&self, id: &I) -> Option<&ImplementationSignature<I>> {
        self.implementations.get(id).map(AsRef::as_ref)
    }
}
