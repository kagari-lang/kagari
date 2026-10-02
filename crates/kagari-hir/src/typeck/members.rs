//! Receiver method surfaces shared by body checking and source tooling.

use crate::{
    aggregates::{AggregateCatalog, InherentMethodSignature},
    language::semantics::{self as traits, ProtocolSemantics},
    typeck::{GenericBounds, inference, table::ConstraintTarget},
    types::{NominalType, TypeId, TypeSubstitution},
};
use kagari_abi::{
    language::{self as standard_traits, Protocol},
    scalar::BuiltinType,
};
use kagari_common::{
    cancellation::CancellationToken, identity::associated_type_id, range::RangeKind,
};

/// Inference alone does not prove that an impl owns this receiver. A method
/// on writable storage cannot be selected through a readonly storage type.
pub(crate) fn inherent_substitution(
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
    )
    .ok()?;
    let owner = aggregates.normalize_type(&method.owner.instantiate(&substitution));
    (!owner.conflicts_with(receiver) || receiver.can_weaken_to(&owner)).then_some(substitution)
}

pub(crate) fn interfaces(
    aggregates: &AggregateCatalog,
    ty: &TypeId,
    assumptions: &GenericBounds,
    cancel: &CancellationToken,
) -> Vec<NominalType> {
    if let TypeId::Trait(interface) = ty {
        let mut bounds = aggregates
            .interface_closure(interface, ty, cancel)
            .unwrap_or_default();
        if Protocol::from_id(&interface.declaration).is_some_and(Protocol::collection) {
            bounds.extend(
                [
                    Protocol::PartialEq,
                    Protocol::Eq,
                    Protocol::Hash,
                    Protocol::Debug,
                ]
                .map(|kind| kind.nominal()),
            );
        }
        add_iterator_view(aggregates, ty, &mut bounds);
        return bounds;
    }
    if !matches!(
        ty,
        TypeId::Generic(_) | TypeId::SelfType(_) | TypeId::Projection { .. }
    ) {
        let mut implemented = Vec::new();
        for implementation in aggregates.implementations() {
            let mut substitution = TypeSubstitution::default();
            if inference::infer(
                &implementation.for_type,
                ty,
                &implementation.generic_params,
                &mut substitution,
                cancel,
            )
            .is_ok()
            {
                let applied = implementation.trait_type.instantiate(&substitution);
                if matches!(
                    aggregates.concrete_interface_implementation(
                        &applied,
                        ty,
                        assumptions,
                        100_000,
                        64,
                        cancel
                    ),
                    Ok(Some(_))
                ) && !implemented.contains(&applied)
                {
                    implemented.push(applied);
                }
            }
        }
        for kind in Protocol::ALL {
            let interface = kind.intrinsic_view(ty);
            if traits::intrinsic_holds(kind, ty, Some(aggregates), assumptions)
                && !implemented.iter().any(|available| {
                    available.declaration == interface.declaration
                        && available.arguments == interface.arguments
                })
            {
                implemented.push(interface);
            }
        }
        if !implemented.is_empty() {
            add_iterator_view(aggregates, ty, &mut implemented);
            return implemented;
        }
    }
    let mut bounds = assumptions
        .get(ty)
        .into_iter()
        .flatten()
        .cloned()
        .collect::<Vec<_>>();
    if let TypeId::Projection {
        receiver,
        interface,
        member,
        arguments,
    } = ty
        && let Some(contract) = aggregates.trait_(&interface.declaration)
    {
        let mut applied = (**interface).clone();
        for bound in assumptions.get(receiver.as_ref()).into_iter().flatten() {
            if let ConstraintTarget::Trait(available) = bound
                && available.declaration == applied.declaration
                && available.arguments == applied.arguments
            {
                applied
                    .associated_types
                    .extend(available.associated_types.clone());
            }
        }
        let mut substitution: TypeSubstitution = contract
            .generic_params
            .iter()
            .cloned()
            .zip(interface.arguments.iter().cloned())
            .collect();
        substitution.insert_receiver(interface.declaration.clone(), (**receiver).clone());
        if let Some(inputs) = contract.associated_type_parameters.get(member) {
            substitution.extend(
                inputs
                    .parameters
                    .iter()
                    .cloned()
                    .zip(arguments.iter().cloned()),
            );
        }
        bounds.extend(
            contract
                .associated_types
                .get(member)
                .into_iter()
                .flatten()
                .map(|bound| match bound {
                    ConstraintTarget::Standard(value) => ConstraintTarget::Standard(*value),
                    ConstraintTarget::Trait(value) => {
                        let TypeId::Trait(value) = TypeId::Trait(value.clone())
                            .instantiate(&substitution)
                            .with_associated_types(&applied)
                        else {
                            unreachable!("associated trait bound");
                        };
                        ConstraintTarget::Trait(value)
                    }
                }),
        );
    }
    let direct = bounds
        .into_iter()
        .filter_map(|bound| match bound {
            ConstraintTarget::Trait(ty) => Some(ty),
            _ => None,
        })
        .collect::<Vec<_>>();
    let mut expanded = Vec::new();
    for interface in direct {
        for parent in aggregates
            .trait_closure(&interface, ty, cancel)
            .unwrap_or_default()
        {
            if !expanded.contains(&parent) {
                expanded.push(parent);
            }
        }
    }
    add_iterator_view(aggregates, ty, &mut expanded);
    expanded
}

fn add_iterator_view(
    aggregates: &AggregateCatalog,
    receiver: &TypeId,
    views: &mut Vec<NominalType>,
) {
    if let TypeId::Range(item, kind) = receiver
        && *kind != RangeKind::Full
    {
        let mut view = Protocol::RangeBounds.nominal();
        view.arguments.push((**item).clone());
        views.push(view);
    }
    for kind in [Protocol::Iterator, Protocol::Iterable] {
        if matches!(
            receiver,
            TypeId::Range(_, _)
                | TypeId::Iter(_)
                | TypeId::Array(_, _)
                | TypeId::Map { .. }
                | TypeId::Set(_, _)
                | TypeId::Builtin(BuiltinType::String)
        ) && let Some(outputs) =
            traits::iteration_outputs(kind, receiver, Some(aggregates), &Default::default())
        {
            if let Some(view) = views
                .iter_mut()
                .find(|n| n.declaration == standard_traits::identity(kind))
            {
                view.associated_types.extend(outputs);
                continue;
            }
            let mut view = kind.nominal();
            view.associated_types = outputs;
            views.push(view);
        }
    }
    if views
        .iter()
        .any(|n| n.declaration == standard_traits::identity(Protocol::Iterable))
    {
        return;
    }
    let Some(iterator) = views
        .iter()
        .find(|n| n.declaration == standard_traits::identity(Protocol::Iterator))
    else {
        return;
    };
    let member = associated_type_id(&iterator.declaration, "Item");
    let item = iterator
        .associated_types
        .get(&member)
        .cloned()
        .unwrap_or_else(|| {
            aggregates.normalize_type(&TypeId::Projection {
                receiver: Box::new(receiver.clone()),
                interface: Box::new(iterator.clone()),
                member,
                arguments: vec![],
            })
        });
    let mut into = Protocol::Iterable.nominal();
    into.associated_types
        .insert(associated_type_id(&into.declaration, "Item"), item);
    into.associated_types.insert(
        associated_type_id(&into.declaration, "Iter"),
        receiver.clone(),
    );
    views.push(into);
}
