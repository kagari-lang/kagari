//! Receiver method surfaces shared by body checking and source tooling.

use crate::{
    aggregates::{
        AggregateCatalog, InherentMethodSignature, implementations::ImplementationSearchLimits,
    },
    language::semantics::{self as traits, ProtocolSemantics},
    typeck::{GenericBounds, inference},
    types::{NominalType, TypeId, TypeSubstitution},
};
use kagari_common::{
    cancellation::CancellationToken,
    identity::{DefinitionKind, ModuleIdentity, table::DefinitionTable},
};
use kagari_types::{
    language::{Protocol, role::LangRole},
    scalar::BuiltinType,
};

/// Inference alone does not prove that an impl owns this receiver.
pub(crate) fn inherent_substitution(
    definitions: &DefinitionTable,
    aggregates: &AggregateCatalog,
    method: &InherentMethodSignature,
    receiver: &TypeId,
    cancel: &CancellationToken,
) -> Option<TypeSubstitution> {
    let mut substitution = TypeSubstitution::default();
    inference::infer(
        &method.owner,
        receiver,
        &method.function.generic_params,
        &mut substitution,
        cancel,
        None,
    )
    .ok()?;
    let owner = aggregates.normalize_type(definitions, &method.owner.instantiate(&substitution));

    (!owner.conflicts_with(receiver)).then_some(substitution)
}

/// Enumerates declared, assumed and intrinsic interfaces for this receiver.
/// Definition IDs belong to `definitions`; no call is selected or admitted.
pub fn interfaces(
    definitions: &DefinitionTable,
    aggregates: &AggregateCatalog,
    ty: &TypeId,
    assumptions: &GenericBounds,
    cancel: &CancellationToken,
) -> Vec<NominalType> {
    let mut implemented = aggregates
        .declared_interfaces(definitions, ty, assumptions, cancel)
        .unwrap_or_default();
    if !matches!(
        ty,
        TypeId::Trait(_) | TypeId::Generic(_) | TypeId::SelfType(_) | TypeId::Projection { .. }
    ) {
        for kind in LangRole::ALL.map(LangRole::protocol) {
            let Some(interface) = kind.intrinsic_view(definitions, ty) else {
                continue;
            };
            if traits::intrinsic_holds(definitions, kind, ty, Some(aggregates), assumptions)
                && !implemented.iter().any(|available| {
                    available.declaration == interface.declaration
                        && available.arguments == interface.arguments
                })
            {
                implemented.push(interface);
            }
        }
    }
    add_iterator_view(definitions, aggregates, ty, &mut implemented);
    implemented
}

/// Matches an instance method's receiver and visibility in the requesting module.
/// Generic arguments and bounds are checked separately after candidate selection.
pub fn inherent_method_substitution(
    definitions: &DefinitionTable,
    aggregates: &AggregateCatalog,
    method: &InherentMethodSignature,
    receiver: &TypeId,
    importer: &ModuleIdentity,
    cancel: &CancellationToken,
) -> Option<TypeSubstitution> {
    if !method
        .function
        .params
        .first()
        .is_some_and(|parameter| parameter.name == "self")
        || !method.visibility.allows(
            definitions.resolve(method.declaration).ok()?.module(),
            importer,
        )
    {
        return None;
    }
    inherent_substitution(definitions, aggregates, method, receiver, cancel)
}

fn add_iterator_view(
    definitions: &DefinitionTable,
    aggregates: &AggregateCatalog,
    receiver: &TypeId,
    views: &mut Vec<NominalType>,
) {
    for kind in [Protocol::Iterator, Protocol::Iterable] {
        if matches!(
            receiver,
            TypeId::Range(_, _) | TypeId::Array(_) | TypeId::Builtin(BuiltinType::String)
        ) && let Some(outputs) = traits::iteration_outputs(
            definitions,
            kind,
            receiver,
            Some(aggregates),
            &Default::default(),
        ) {
            if let Some(view) = views
                .iter_mut()
                .find(|n| Some(n.declaration) == kind.definition(definitions))
            {
                view.associated_types.extend(outputs);
                continue;
            }
            let Some(mut view) = kind.nominal(definitions) else {
                continue;
            };
            view.associated_types = outputs;
            views.push(view);
        }
    }
    if views
        .iter()
        .any(|n| Some(n.declaration) == Protocol::Iterable.definition(definitions))
    {
        return;
    }
    let Some(iterator) = views
        .iter()
        .find(|n| Some(n.declaration) == Protocol::Iterator.definition(definitions))
    else {
        return;
    };
    let Some(member) = definitions.lookup_child(
        iterator.declaration,
        DefinitionKind::AssociatedType,
        "Item",
        0,
    ) else {
        return;
    };
    let item = iterator
        .associated_types
        .get(&member)
        .cloned()
        .unwrap_or_else(|| {
            aggregates.normalize_type(
                definitions,
                &TypeId::Projection {
                    receiver: Box::new(receiver.clone()),
                    interface: Box::new(iterator.clone()),
                    member,
                    arguments: vec![],
                },
            )
        });
    let Some(mut into) = Protocol::Iterable.nominal(definitions) else {
        return;
    };
    let Some(item_id) =
        definitions.lookup_child(into.declaration, DefinitionKind::AssociatedType, "Item", 0)
    else {
        return;
    };
    let Some(iter_id) =
        definitions.lookup_child(into.declaration, DefinitionKind::AssociatedType, "Iter", 0)
    else {
        return;
    };
    into.associated_types.insert(item_id, item);
    into.associated_types.insert(iter_id, receiver.clone());
    views.push(into);
}

/// Semantic receiver and constraint environment shared by member consumers.
/// Incomplete candidates never constitute positive checking evidence.
pub struct MemberQuery<'a> {
    pub definitions: &'a DefinitionTable,
    pub aggregates: &'a AggregateCatalog,
    pub receiver: &'a TypeId,
    pub assumptions: &'a GenericBounds,
    pub cancel: &'a CancellationToken,
}

impl MemberQuery<'_> {
    /// Tests substituted constraints using semantic interface and implementation search.
    /// `allow_unresolved` is a caller policy; it does not establish executable validity.
    pub fn bounds_allow(
        &self,
        bounds: &GenericBounds,
        substitution: &TypeSubstitution,
        interface: Option<&NominalType>,
        allow_unresolved: bool,
    ) -> bool {
        let instantiate = |ty: &TypeId| {
            let applied = if let Some(interface) = interface {
                ty.with_self(&interface.declaration, self.receiver)
                    .instantiate(substitution)
                    .with_associated_types(interface, self.receiver)
            } else {
                ty.instantiate(substitution)
            };
            self.aggregates.normalize_type(self.definitions, &applied)
        };
        bounds.iter().all(|(target, constraints)| {
            let actual = instantiate(target);

            if actual.is_unresolved() {
                return allow_unresolved;
            }
            constraints.iter().all(|required| {
                let required = instantiate(&TypeId::Trait(required.clone()));
                if required.is_unresolved() {
                    return allow_unresolved;
                }
                let TypeId::Trait(required) = required else {
                    return false;
                };
                self.aggregates.intrinsic_implementation(
                    self.definitions,
                    &required,
                    &actual,
                    self.assumptions,
                ) || interfaces(
                    self.definitions,
                    self.aggregates,
                    &actual,
                    self.assumptions,
                    self.cancel,
                )
                .iter()
                .any(|available| available.satisfies(&required))
                    || self
                        .aggregates
                        .concrete_interface_implementation(
                            self.definitions,
                            &required,
                            &actual,
                            self.assumptions,
                            ImplementationSearchLimits {
                                max_checks: 4096,
                                max_depth: 64,
                            },
                            self.cancel,
                        )
                        .is_ok_and(|selected| selected.is_some())
            })
        })
    }
}
