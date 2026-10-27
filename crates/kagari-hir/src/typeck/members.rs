//! Receiver method surfaces shared by body checking and source tooling.

use crate::{
    aggregates::{AggregateCatalog, InherentMethodSignature},
    language::semantics::{self as traits, ProtocolSemantics},
    typeck::{GenericBounds, inference},
    types::{NominalType, TypeId, TypeSubstitution},
};
use kagari_common::{
    cancellation::CancellationToken,
    identity::{DefinitionKind, table::DefinitionTable},
};
use kagari_types::{
    language::{Protocol, role::LangRole},
    scalar::BuiltinType,
};

/// Inference alone does not prove that an impl owns this receiver. A method
/// on writable storage cannot be selected through a readonly storage type.
pub(crate) fn inherent_substitution(
    definitions: &DefinitionTable,
    aggregates: &AggregateCatalog,
    method: &InherentMethodSignature,
    receiver: &TypeId,
    cancel: &CancellationToken,
) -> Option<TypeSubstitution> {
    let scoped = matches!(receiver, TypeId::Reference { .. });
    let receiver = if let TypeId::Reference { target, access } = receiver {
        let required = method.receiver_access?;
        if !access.permits(required) {
            return None;
        }
        target.as_ref()
    } else {
        receiver
    };
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
    if !scoped
        && method.receiver_access.is_some()
        && !matches!(
            receiver,
            TypeId::NativeObject(_) | TypeId::Generic(_) | TypeId::SelfType(_)
        )
    {
        // The outer scoped reference was unwrapped above. Preserve that fact:
        // plain GC structs cannot become Rust references through a method call.
        return None;
    }
    (!owner.conflicts_with(receiver) || receiver.can_weaken_to(&owner)).then_some(substitution)
}

pub(crate) fn interfaces(
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

fn add_iterator_view(
    definitions: &DefinitionTable,
    aggregates: &AggregateCatalog,
    receiver: &TypeId,
    views: &mut Vec<NominalType>,
) {
    for kind in [Protocol::Iterator, Protocol::Iterable] {
        if matches!(
            receiver,
            TypeId::Range(_, _)
                | TypeId::Iter(_)
                | TypeId::Array(_)
                | TypeId::Builtin(BuiltinType::String)
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
