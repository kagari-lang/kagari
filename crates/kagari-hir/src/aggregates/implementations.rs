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
    identity,
    identity::{DefinitionPath, reference::DefinitionReference},
};
use kagari_types::{
    declaration::conversion::ConversionAdapter, language as standard_traits, language::Protocol,
};
use std::{
    collections::{BTreeMap, HashSet},
    iter,
    sync::Arc,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImplementationSearchError {
    Cancelled,
    LimitExceeded,
}

struct SearchBudget<'a> {
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImplementationSignature<I: DefinitionReference = DefinitionPath> {
    /// Derived from the retained installation object, never a source identity.
    pub(crate) engine_owned: bool,
    pub associated_type_families: BTreeMap<I, AssociatedTypeFamily<I>>,
    pub id: I,
    pub trait_type: NominalType<I>,
    pub for_type: TypeId<I>,
    pub generic_params: Vec<GenericParameterType<I>>,
    pub bounds: GenericBounds<I>,
    /// Trait method identity to implementation method identity.
    pub methods: BTreeMap<I, I>,
}

impl AggregateCatalog {
    /// Standard protocol implementations are type-owned, so dependencies and callers
    /// cannot disagree because a downstream module adds a different override.
    pub fn standard_override_error(
        &self,
        implementation: &ImplementationSignature,
    ) -> Option<&'static str> {
        if self
            .trait_(&implementation.trait_type.declaration)
            .is_some_and(|contract| contract.storage_access.is_some())
        {
            return match &implementation.for_type {
                TypeId::NativeObject(nominal) | TypeId::Struct(nominal) | TypeId::Enum(nominal) => {
                    (nominal.declaration.module != implementation.id.module).then_some(
                        "storage interface implementations must belong to their nominal type",
                    )
                }
                TypeId::Array(_, _) | TypeId::Map { .. } | TypeId::Set(_, _)
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
        let protocol = Protocol::from_id(&implementation.trait_type.declaration);
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
            || matches!(adapter, Some(ConversionAdapter::CheckedNumeric { .. }))
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
                matches!(ty,TypeId::NativeObject(n)|TypeId::Struct(n)|TypeId::Enum(n) if n.declaration.module==implementation.id.module)
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
                candidate.trait_type.declaration == standard_traits::identity(other)
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
        if nominal.declaration.module != implementation.id.module {
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
                self.concrete_interface_implementation(
                    &required.nominal(),
                    &implementation.for_type,
                    &implementation.bounds,
                    4096,
                    64,
                    &CancellationToken::default()
                ),
                Ok(Some(_))
            ) {
                return Some(
                    "custom Eq requires explicit PartialEq; custom Hash requires explicit PartialEq and Eq under the same bounds",
                );
            }
        }
        None
    }

    pub fn standard_protocol_holds(
        &self,
        protocol: Protocol,
        ty: &TypeId,
        assumptions: &GenericBounds,
    ) -> bool {
        let cancel = CancellationToken::default();
        let mut budget = SearchBudget {
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
        if visiting.contains(&(protocol.nominal(), ty.clone())) {
            return Ok(false);
        }
        if let TypeId::Trait(interface) = ty
            && self
                .trait_closure(interface, ty, budget.cancel)?
                .iter()
                .any(|parent| parent.satisfies(&protocol.nominal()))
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
                    && (nominal == &protocol.nominal()
                        || protocol == Protocol::PartialEq && nominal == &Protocol::Eq.nominal())
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
                    if self
                        .implementation_matches(
                            implementation,
                            &protocol.nominal(),
                            ty,
                            visiting,
                            budget,
                        )?
                        .is_some()
                    {
                        return Ok(true);
                    }
                }
                if matches!(protocol, Protocol::Eq | Protocol::Hash) {
                    for implementation in self.implementations.values() {
                        if self
                            .implementation_matches(
                                implementation,
                                &Protocol::PartialEq.nominal(),
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
                    protocol,
                    ty,
                    None,
                    budget.assumptions,
                ));
            }
            let members = match ty {
                TypeId::Tuple(members) | TypeId::StandardEnum { args: members, .. } => {
                    Some(members.clone())
                }
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

    pub fn normalize_type(&self, ty: &TypeId) -> TypeId {
        associated::normalize(ty, &|interface, receiver, member, arguments| {
            if arguments.is_empty()
                && *member == identity::associated_type_id(&interface.declaration, "Error")
                && let Some(error) = traits::conversion_error(interface, receiver, Some(self))
            {
                return Some(error);
            }

            if arguments.is_empty()
                && !matches!(receiver, TypeId::SelfType(_))
                && *member == identity::associated_type_id(&interface.declaration, "Error")
                && let Some((required, target)) =
                    traits::conversion_requirement(interface, receiver, Some(self))
            {
                return Some(TypeId::Projection {
                    receiver: Box::new(target),
                    member: identity::associated_type_id(&required.declaration, "Error"),
                    interface: Box::new(required),
                    arguments: vec![],
                });
            }

            if arguments.is_empty()
                && let Some(kind) = Protocol::from_id(&interface.declaration)
                && kind.iteration()
                && let Some(outputs) =
                    traits::iteration_outputs(kind, receiver, Some(self), &Default::default())
                && let Some(output) = outputs.get(member)
            {
                return Some(output.clone());
            }
            if arguments.is_empty()
                && *member == identity::associated_type_id(&interface.declaration, "Output")
                && let Some(output) = traits::intrinsic_output(interface, receiver)
            {
                return Some(output);
            }
            if let TypeId::Trait(actual) = receiver {
                return self
                    .interface_closure(actual, receiver, &Default::default())
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

    pub fn implementations(&self) -> impl Iterator<Item = &ImplementationSignature> {
        self.implementations.values().map(AsRef::as_ref)
    }

    pub(crate) fn overlapping_implementations(
        &self,
    ) -> Vec<(&ImplementationSignature, &ImplementationSignature)> {
        let mut by_trait: BTreeMap<&DefinitionPath, Vec<&ImplementationSignature>> =
            BTreeMap::new();
        let mut overlaps = Vec::new();
        for implementation in self.implementations.values() {
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
                method_identities.insert(trait_method.clone(), declaration.clone());
            }
            self.implementations.insert(
                id.clone(),
                Arc::new(ImplementationSignature {
                    engine_owned: lowered.registered_native_api,
                    associated_type_families: signatures
                        .type_table()
                        .associated_type_families
                        .iter()
                        .filter_map(|(member, family)| {
                            let mut parent = member.clone();
                            parent.path.pop();
                            if parent != *id {
                                return None;
                            }
                            Some((
                                identity::associated_type_id(
                                    &trait_type.declaration,
                                    &member.path.last()?.name,
                                ),
                                family.clone(),
                            ))
                        })
                        .collect(),
                    id: id.clone(),
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

    pub fn implementation_method(
        &self,
        method: &DefinitionPath,
        trait_type: &NominalType,
        receiver: &TypeId,
    ) -> Option<(DefinitionPath, Vec<TypeId>)> {
        let cancel = CancellationToken::default();
        let mut budget = SearchBudget {
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
                    .map(|_| {
                        let mut target = implementation.id.clone();
                        target
                            .path
                            .push(method.path.last().expect("method identity").clone());
                        target
                    })
            })?;
            Some((target, arguments))
        });
        let result = matches.next()?;
        matches.next().is_none().then_some(result)
    }

    /// An omitted method has the same impl-owned identity as an explicit method;
    /// its script body or symbolic native application remains owned by the trait module.
    pub fn default_method(
        &self,
        target: &DefinitionPath,
    ) -> Option<(&ImplementationSignature, &MethodSignature)> {
        let mut owner = target.clone();
        let name = owner.path.pop()?;
        let implementation = self.implementation_signature(&owner)?;
        let method = self
            .trait_(&implementation.trait_type.declaration)?
            .methods
            .iter()
            .find(|method| method.id.path.last() == Some(&name))?;
        (matches!(
            method.default,
            Some(MethodDefault::Script | MethodDefault::Native(NativeBinding::Default(_)))
        ) && !implementation.methods.contains_key(&method.id))
        .then_some((implementation, method))
    }

    pub fn implementation_methods(
        &self,
        implementation: &ImplementationSignature,
    ) -> Vec<DefinitionPath> {
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
                    .then(|| {
                        let mut target = implementation.id.clone();
                        target
                            .path
                            .push(method.id.path.last().expect("method identity").clone());
                        target
                    })
                })
            })
            .collect()
    }

    pub fn implementation_count(&self, trait_type: &NominalType, receiver: &TypeId) -> usize {
        self.implementation_count_bounded(
            trait_type,
            receiver,
            usize::MAX,
            usize::MAX,
            &CancellationToken::default(),
        )
        .unwrap_or(0)
    }

    pub fn concrete_interface_implementation(
        &self,
        trait_type: &NominalType,
        receiver: &TypeId,
        assumptions: &GenericBounds,
        max_checks: usize,
        max_depth: usize,
        cancel: &CancellationToken,
    ) -> Result<Option<(DefinitionPath, Vec<TypeId>)>, ImplementationSearchError> {
        let mut budget = SearchBudget {
            checks_left: max_checks,
            depth: 0,
            max_depth,
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
                selected = Some((implementation.id.clone(), arguments));
            }
        }
        Ok(selected)
    }

    pub fn implementation_count_bounded(
        &self,
        trait_type: &NominalType,
        receiver: &TypeId,
        max_checks: usize,
        max_depth: usize,
        cancel: &CancellationToken,
    ) -> Result<usize, ImplementationSearchError> {
        let mut budget = SearchBudget {
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
            && traits::intrinsic_applies(trait_type, receiver, Some(self), &Default::default())
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
                let actual = self.normalize_type(&parameter.instantiate(&matched));
                for constraint in constraints {
                    budget
                        .cancel
                        .check()
                        .map_err(|_| ImplementationSearchError::Cancelled)?;
                    let satisfied = match constraint {
                        ConstraintTarget::Standard(standard) => type_satisfies_standard_constraint(
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
                                &required,
                                &actual,
                                None,
                                budget.assumptions,
                            ) {
                                continue;
                            }
                            if required.arguments.is_empty()
                                && required.associated_types.is_empty()
                                && let Some(protocol) = Protocol::from_id(&required.declaration)
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
                                    traits::iterator_requirement(&required, &actual)
                                {
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
            TypeId::Tuple(items) | TypeId::StandardEnum { args: items, .. } => {
                pending.extend(items)
            }
            TypeId::Array(item, _)
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

#[cfg(test)]
mod search_tests {
    use super::*;
    use crate::{typeck::table::ConstraintTarget, types::TypeId};
    use kagari_common::identity::{
        DefinitionKind, DefinitionPathSegment, ModuleIdentity, PackageId,
    };
    use kagari_types::scalar::BuiltinType;

    fn definition(kind: DefinitionKind, name: &str) -> DefinitionPath {
        DefinitionPath {
            module: ModuleIdentity {
                package: PackageId("pkg".into()),
                path: vec!["module".into()],
            },
            path: vec![DefinitionPathSegment {
                kind,
                name: name.into(),
                occurrence: 0,
            }],
        }
    }

    #[test]
    fn bounded_trait_search_reuses_generic_matching_and_stops_growth() {
        let marker = definition(DefinitionKind::Trait, "Marker");
        let owner = definition(DefinitionKind::Impl, "");
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
            catalog.implementation_count_bounded(&required, &actual, 1, 4, &cancel),
            Ok(1)
        );
        assert_eq!(
            catalog.implementation_count_bounded(&required, &actual, 0, 4, &cancel),
            Err(ImplementationSearchError::LimitExceeded)
        );
        cancel.cancel();
        assert_eq!(
            catalog.implementation_count_bounded(&required, &actual, 1, 4, &cancel),
            Err(ImplementationSearchError::Cancelled)
        );

        let mut chained = signature;
        let next = definition(DefinitionKind::Trait, "Next");
        chained.bounds.insert(
            TypeId::Generic(parameter.clone()),
            vec![ConstraintTarget::Trait(NominalType {
                associated_types: Default::default(),
                declaration: next.clone(),
                arguments: vec![TypeId::Generic(parameter.clone())],
            })],
        );
        let next_parameter = GenericParameterType {
            owner: definition(DefinitionKind::Impl, "next"),
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
    pub fn implementation_signature(&self, id: &I) -> Option<&ImplementationSignature<I>> {
        self.implementations.get(id).map(AsRef::as_ref)
    }
}
