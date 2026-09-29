//! Standard protocols have declaration identities and ordinary trait contracts.

use crate::builtin::declarations::{
    ApiBoundSemantics, ApiImplementationSemantics, ApiTypeSemantics,
};
use kagari_abi::{
    numeric as scalar_numeric,
    scalar::BuiltinType,
    standard::{
        surface::{self as standard_surface, StandardEnum, StandardModule},
        traits::{StandardTrait, identity},
    },
};
use kagari_common::{
    collection::CollectionAccess::Mutable,
    identity::{DefinitionId, associated_type_id},
};

use super::{declarations, numeric, surface};
use crate::{
    aggregates::{self, AggregateCatalog},
    typeck::{ConstraintTarget, GenericBounds},
    types::{NominalType, TypeId},
};
use std::collections::{BTreeMap, HashSet};

pub trait StandardTraitSemantics {
    fn nominal(self) -> NominalType;
    fn intrinsic_view(self, receiver: &TypeId) -> NominalType;
}
impl StandardTraitSemantics for StandardTrait {
    fn nominal(self) -> NominalType {
        NominalType {
            declaration: identity(self),
            arguments: vec![],
            associated_types: Default::default(),
        }
    }

    fn intrinsic_view(self, receiver: &TypeId) -> NominalType {
        let mut view = self.nominal();
        if self.binary_operator() {
            view.arguments.push(receiver.clone());
        }
        if self == Self::Fn
            && let TypeId::Function { params, .. } = receiver
        {
            view.arguments.push(callable_arguments(params));
        }
        if self == Self::Index {
            view.arguments.push(TypeId::Builtin(BuiltinType::I32));
        }
        if let Some(output) = intrinsic_output(&view, receiver) {
            view.associated_types
                .insert(associated_type_id(&view.declaration, "Output"), output);
        }
        view
    }
}

/// A zero-argument call uses unit; all other calls pass a positional tuple.
pub fn callable_arguments(params: &[TypeId]) -> TypeId {
    if params.is_empty() {
        TypeId::Builtin(BuiltinType::Unit)
    } else {
        TypeId::Tuple(params.to_vec())
    }
}

/// Parameter context from a callable bound; an unspecified output remains unknown.
pub fn callable_signature(interface: &NominalType) -> Option<TypeId> {
    if StandardTrait::from_id(&interface.declaration) != Some(StandardTrait::Fn) {
        return None;
    }
    let params = match interface.arguments.as_slice() {
        [TypeId::Tuple(params)] => params.clone(),
        [TypeId::Builtin(BuiltinType::Unit)] => vec![],
        _ => return None,
    };
    let result = interface
        .associated_types
        .get(&associated_type_id(&interface.declaration, "Output"))
        .cloned()
        .unwrap_or(TypeId::Unknown);
    Some(TypeId::Function {
        params,
        result: Box::new(result),
    })
}

/// Builtin associated outputs are computed from the applied protocol, not its spelling.
pub fn intrinsic_output(interface: &NominalType, receiver: &TypeId) -> Option<TypeId> {
    let kind = StandardTrait::from_id(&interface.declaration)?;
    if kind == StandardTrait::Fn
        && let TypeId::Function { params, result } = receiver
    {
        return (interface.arguments == [callable_arguments(params)]).then(|| (**result).clone());
    }
    if kind == StandardTrait::Index
        && let TypeId::Array(element, _) = receiver
        && matches!(
            interface.arguments.as_slice(),
            [TypeId::Builtin(
                BuiltinType::I8
                    | BuiltinType::I16
                    | BuiltinType::I32
                    | BuiltinType::I64
                    | BuiltinType::ISize
                    | BuiltinType::U8
                    | BuiltinType::U16
                    | BuiltinType::U32
                    | BuiltinType::U64
                    | BuiltinType::USize
            )]
        )
    {
        return Some((**element).clone());
    }
    if interface.arguments.is_empty()
        && (kind == StandardTrait::Not && *receiver == TypeId::Builtin(BuiltinType::Bool)
            || kind == StandardTrait::Neg
                && matches!(
                    receiver,
                    TypeId::Builtin(
                        BuiltinType::I8
                            | BuiltinType::I16
                            | BuiltinType::I32
                            | BuiltinType::I64
                            | BuiltinType::ISize
                            | BuiltinType::F32
                            | BuiltinType::F64
                    )
                ))
    {
        return Some(receiver.clone());
    }
    if matches!(
        kind,
        StandardTrait::BitAnd
            | StandardTrait::BitOr
            | StandardTrait::BitXor
            | StandardTrait::Shl
            | StandardTrait::Shr
    ) {
        let [TypeId::Builtin(rhs)] = interface.arguments.as_slice() else {
            return None;
        };
        let TypeId::Builtin(lhs) = receiver else {
            return None;
        };
        return (lhs.integer_layout().is_some()
            && rhs.integer_layout().is_some()
            && (matches!(kind, StandardTrait::Shl | StandardTrait::Shr) || lhs == rhs))
            .then(|| receiver.clone());
    }
    if kind == StandardTrait::Not
        && interface.arguments.is_empty()
        && matches!(receiver, TypeId::Builtin(ty) if ty.integer_layout().is_some())
    {
        return Some(receiver.clone());
    }
    if kind.binary_operator()
        && interface.arguments.as_slice() == [receiver.clone()]
        && surface::supports_arithmetic(receiver, receiver)
    {
        Some(receiver.clone())
    } else {
        None
    }
}

/// Native numeric aggregation consumes exactly its destination scalar type.
pub fn numeric_aggregation_item(receiver: &TypeId) -> Option<TypeId> {
    (matches!(
        receiver,
        TypeId::Builtin(
            BuiltinType::I8
                | BuiltinType::I16
                | BuiltinType::I32
                | BuiltinType::I64
                | BuiltinType::ISize
                | BuiltinType::U8
                | BuiltinType::U16
                | BuiltinType::U32
                | BuiltinType::U64
                | BuiltinType::USize
                | BuiltinType::F32
                | BuiltinType::F64
        )
    ))
    .then(|| receiver.clone())
}

pub fn intrinsic_applies(
    interface: &NominalType,
    receiver: &TypeId,
    catalog: Option<&AggregateCatalog>,
    bounds: &GenericBounds,
) -> bool {
    let Some(kind) = StandardTrait::from_id(&interface.declaration) else {
        return false;
    };
    if kind == StandardTrait::FromStr {
        return parsing_error(receiver).is_some_and(|error| {
            interface.arguments.is_empty()
                && interface.associated_types.iter().all(|(id, ty)| {
                    *id == associated_type_id(&interface.declaration, "Err") && *ty == error
                })
        });
    }
    if kind.collection() {
        return declarations::implementations(receiver)
            .iter()
            .any(|implementation| {
                implementation
                    .applied_arguments(receiver, interface)
                    .is_some()
            });
    }
    if kind.aggregation() {
        return interface.arguments.as_slice() == [receiver.clone()]
            && interface.associated_types.is_empty()
            && numeric_aggregation_item(receiver).is_some();
    }
    if kind == StandardTrait::RangeBounds {
        return declarations::implementations(receiver)
            .into_iter()
            .any(|i| i.applied_arguments(receiver, interface).is_some());
    }
    if kind == StandardTrait::FromIterator {
        if let Some((mut required, mut target)) = lifted_collection_requirement(interface, receiver)
        {
            for depth in 0..64 {
                if bounds.get(&target).is_some_and(|bounds| {
                    bounds
                        .iter()
                        .any(|b| matches!(b, ConstraintTarget::Trait(t) if t.satisfies(&required)))
                }) {
                    return true;
                }
                if let Some((inner, destination)) =
                    lifted_collection_requirement(&required, &target)
                {
                    if depth == 63 {
                        return false;
                    }
                    required = inner;
                    target = destination;
                } else {
                    return intrinsic_applies(&required, &target, catalog, bounds)
                        || catalog.is_some_and(|c| {
                            c.concrete_interface_implementation(
                                &required,
                                &target,
                                bounds,
                                4096,
                                64,
                                &Default::default(),
                            )
                            .is_ok_and(|i| i.is_some())
                        });
                }
            }
            return false;
        }
        return declarations::implementations(receiver)
            .into_iter()
            .any(|implementation| {
                let Some(arguments) = implementation.applied_arguments(receiver, interface) else {
                    return false;
                };
                implementation.bounds.iter().all(|(target, constraints)| {
                    let actual = target.instantiate(&arguments);
                    constraints.iter().all(|constraint| {
                        let required = constraint.nominal(&arguments);
                        intrinsic_applies(&required, &actual, catalog, bounds)
                    })
                })
            });
    }

    if kind.iteration() {
        return iteration_outputs(kind, receiver, catalog, bounds).is_some_and(|outputs| {
            interface.arguments.is_empty()
                && interface
                    .associated_types
                    .iter()
                    .all(|(member, ty)| outputs.get(member) == Some(ty))
        });
    }
    if kind.conversion() {
        if let (TypeId::Builtin(target), [TypeId::Builtin(source)]) =
            (receiver, interface.arguments.as_slice())
        {
            if kind == StandardTrait::From
                && interface.associated_types.is_empty()
                && scalar_numeric::lossless_from(*source, *target)
            {
                return true;
            }
            if kind == StandardTrait::TryFrom
                && let Some(error) = numeric::try_error(*source, *target)
            {
                return interface.associated_types.iter().all(|(member, ty)| {
                    *member == associated_type_id(&interface.declaration, "Error") && *ty == error
                });
            }
        }
        return kind == StandardTrait::From
            && interface.arguments.as_slice() == [receiver.clone()]
            && interface.associated_types.is_empty();
    }
    if kind.operator() {
        let Some(output) = intrinsic_output(interface, receiver) else {
            return false;
        };
        interface.associated_types.iter().all(|(member, ty)| {
            *member == associated_type_id(&interface.declaration, "Output") && *ty == output
        })
    } else {
        interface.arguments.is_empty()
            && interface.associated_types.is_empty()
            && intrinsic_holds(kind, receiver, catalog, bounds)
    }
}

pub fn collection_item(receiver: &TypeId) -> Option<TypeId> {
    match receiver {
        TypeId::Array(item, _) | TypeId::Set(item, _) => Some((**item).clone()),
        TypeId::Map { key, value, .. } => {
            Some(TypeId::Tuple(vec![(**key).clone(), (**value).clone()]))
        }
        _ => None,
    }
}

/// Collect successful payloads using the destination's ordinary constructor.
pub fn lifted_collection_requirement(
    interface: &NominalType,
    receiver: &TypeId,
) -> Option<(NominalType, TypeId)> {
    if StandardTrait::from_id(&interface.declaration) != Some(StandardTrait::FromIterator) {
        return None;
    }
    for implementation in declarations::implementations(receiver) {
        let Some(arguments) = implementation.applied_arguments(receiver, interface) else {
            continue;
        };
        for (target, constraints) in implementation.bounds {
            for constraint in *constraints {
                if constraint.name == "FromIterator" {
                    return Some((
                        constraint.nominal(&arguments),
                        target.instantiate(&arguments),
                    ));
                }
            }
        }
    }
    None
}

pub fn ordering_type(optional: bool) -> TypeId {
    let ordering = TypeId::StandardEnum {
        kind: StandardEnum::Ordering,
        args: vec![],
    };
    if optional {
        TypeId::StandardEnum {
            kind: StandardEnum::Option,
            args: vec![ordering],
        }
    } else {
        ordering
    }
}

/// Intrinsic implementations preserve the language's value/identity contract.
/// Explicit Debug/Display implementations are selected before intrinsic fallbacks.
pub fn intrinsic_holds(
    protocol: StandardTrait,
    ty: &TypeId,
    catalog: Option<&AggregateCatalog>,
    bounds: &GenericBounds,
) -> bool {
    if protocol == StandardTrait::FromStr {
        return parsing_error(ty).is_some();
    }
    if protocol.collection() {
        return false;
    }
    if protocol.iteration() {
        return iteration_outputs(protocol, ty, catalog, bounds).is_some();
    }
    if protocol.conversion() || protocol.aggregation() || protocol == StandardTrait::FromIterator {
        return false;
    }
    if protocol.operator() {
        return intrinsic_output(&protocol.intrinsic_view(ty), ty).is_some();
    }
    if matches!(protocol, StandardTrait::PartialOrd | StandardTrait::Ord) {
        if bounds.get(ty).is_some_and(|constraints| constraints.iter().any(|c| matches!(c, ConstraintTarget::Trait(n) if n.declaration == identity(protocol) || protocol == StandardTrait::PartialOrd && n.declaration == identity(StandardTrait::Ord)))) {return true;}
        if let Some(catalog) = catalog
            && matches!(
                catalog.concrete_interface_implementation(
                    &protocol.nominal(),
                    ty,
                    bounds,
                    4096,
                    64,
                    &Default::default()
                ),
                Ok(Some(_))
            )
        {
            return true;
        }
        return match ty {
            TypeId::Unknown | TypeId::Error => true,
            TypeId::Builtin(BuiltinType::F32 | BuiltinType::F64) => {
                protocol == StandardTrait::PartialOrd
            }
            TypeId::Builtin(_) => true,
            TypeId::StandardEnum {
                kind: StandardEnum::Ordering,
                ..
            } => true,
            _ => false,
        };
    }
    if protocol.equality_protocol()
        && let Some(catalog) = catalog
    {
        return catalog.standard_protocol_holds(protocol, ty, bounds);
    }
    let mut pending = vec![(ty.clone(), 0usize)];
    let mut seen = HashSet::new();
    while let Some((ty, depth)) = pending.pop() {
        if seen.len() >= 4096 || depth > 64 {
            return false;
        }
        if !seen.insert(ty.clone()) {
            continue;
        }
        if bounds.get(&ty).is_some_and(|bounds| bounds.iter().any(|bound| matches!(bound, ConstraintTarget::Trait(n) if StandardTrait::from_id(&n.declaration).is_some_and(|p| p == protocol || p == StandardTrait::Eq && protocol == StandardTrait::PartialEq)))) { continue; }
        match ty {
            // Recovery holes cannot disprove a protocol; code generation rejects them.
            TypeId::Unknown | TypeId::Error => {}
            TypeId::Builtin(b) => {
                if matches!(protocol, StandardTrait::Eq | StandardTrait::Hash)
                    && matches!(b, BuiltinType::F32 | BuiltinType::F64)
                {
                    return false;
                }
            }
            TypeId::Trait(ref interface)
                if StandardTrait::from_id(&interface.declaration)
                    .is_some_and(StandardTrait::collection)
                    && protocol != StandardTrait::Display => {}
            TypeId::Enum(_) | TypeId::Host(_) if protocol == StandardTrait::Debug => {}
            TypeId::Struct(_) | TypeId::Array(_, _) | TypeId::Map { .. } | TypeId::Set(_, _)
                if protocol != StandardTrait::Display => {}
            TypeId::Tuple(elements) | TypeId::StandardEnum { args: elements, .. }
                if protocol != StandardTrait::Display =>
            {
                pending.extend(elements.into_iter().map(|ty| (ty, depth + 1)))
            }
            TypeId::Enum(instance) if protocol != StandardTrait::Display => {
                let Some(catalog) = catalog else {
                    continue;
                };
                let Some(contract) = catalog.enumeration(&instance.declaration) else {
                    return false;
                };
                let substitution = contract
                    .generic_params
                    .iter()
                    .cloned()
                    .zip(instance.arguments)
                    .collect();
                for variant in &contract.variants {
                    pending.extend(
                        variant
                            .payload
                            .iter()
                            .map(|ty| (ty.instantiate(&substitution), depth + 1)),
                    );
                }
            }
            _ => return false,
        }
    }
    true
}

pub fn in_module(module: StandardModule, name: &str) -> Option<StandardTrait> {
    StandardTrait::ALL.into_iter().find(|kind| {
        kind.name() == name
            && standard_surface::standard_modules().iter().any(|spec| {
                spec.kind == module && spec.path == format!("std::{}", kind.namespace())
            })
    })
}

/// The reverse protocol is a proof of the corresponding destination conversion.
pub fn conversion_requirement(
    interface: &NominalType,
    receiver: &TypeId,
) -> Option<(NominalType, TypeId)> {
    let kind = StandardTrait::from_id(&interface.declaration)?;
    if !kind.reverse_conversion() || interface.arguments.len() != 1 {
        return None;
    }
    let forward = if kind == StandardTrait::Into {
        StandardTrait::From
    } else {
        StandardTrait::TryFrom
    };
    let mut required = forward.nominal();
    required.arguments.push(receiver.clone());
    for (member, ty) in &interface.associated_types {
        if *member != associated_type_id(&interface.declaration, "Error")
            || !kind.fallible_conversion()
        {
            return None;
        }
        required.associated_types.insert(
            associated_type_id(&required.declaration, "Error"),
            ty.clone(),
        );
    }
    Some((required, interface.arguments[0].clone()))
}

pub fn iteration_outputs(
    kind: StandardTrait,
    receiver: &TypeId,
    catalog: Option<&AggregateCatalog>,
    bounds: &GenericBounds,
) -> Option<BTreeMap<DefinitionId, TypeId>> {
    if let TypeId::Trait(interface) = receiver {
        let inherited = aggregates::trait_inheritance_closure(
            interface,
            receiver,
            &Default::default(),
            &|id| {
                let contract = catalog?.trait_(id)?;
                Some((
                    contract.generic_params.clone(),
                    contract.supertraits.clone(),
                ))
            },
        )
        .ok()?;
        return inherited
            .into_iter()
            .find(|parent| parent.declaration == identity(kind))
            .map(|parent| parent.associated_types);
    }
    let declared_kind = if matches!(receiver, TypeId::Iter(_)) && kind == StandardTrait::Iterable {
        StandardTrait::Iterator
    } else {
        kind
    };
    if let Some(implementation) = declarations::implementations(receiver)
        .into_iter()
        .find(|i| i.interface == declared_kind.name())
    {
        let arguments = implementation.arguments(receiver)?;
        let id = identity(kind);
        let mut outputs: BTreeMap<_, _> = implementation
            .associated_types
            .iter()
            .map(|(member, ty)| {
                (
                    associated_type_id(&id, member.path.last().unwrap().1),
                    ty.instantiate(&arguments),
                )
            })
            .collect();
        if kind != declared_kind {
            outputs.insert(associated_type_id(&id, "Iter"), receiver.clone());
        }
        return Some(outputs);
    }
    if kind != StandardTrait::Iterable {
        return None;
    }
    let iterator = StandardTrait::Iterator.nominal();
    let available = bounds
        .get(receiver)
        .into_iter()
        .flatten()
        .any(|b| matches!(b,ConstraintTarget::Trait(n) if n.declaration==iterator.declaration))
        || catalog.is_some_and(|c| {
            c.concrete_interface_implementation(
                &iterator,
                receiver,
                bounds,
                4096,
                64,
                &Default::default(),
            )
            .is_ok_and(|i| i.is_some())
        });
    if !available {
        return None;
    }
    let member = associated_type_id(&iterator.declaration, "Item");
    let item = bounds
        .get(receiver)
        .into_iter()
        .flatten()
        .find_map(|b| match b {
            ConstraintTarget::Trait(n) if n.declaration == iterator.declaration => {
                n.associated_types.get(&member).cloned()
            }
            _ => None,
        })
        .unwrap_or_else(|| {
            let projection = TypeId::Projection {
                receiver: Box::new(receiver.clone()),
                interface: Box::new(iterator),
                member,
                arguments: vec![],
            };
            catalog.map_or(projection.clone(), |c| c.normalize_type(&projection))
        });
    let id = identity(kind);
    Some(
        [
            (associated_type_id(&id, "Item"), item),
            (associated_type_id(&id, "Iter"), receiver.clone()),
        ]
        .into_iter()
        .collect(),
    )
}

/// Identity Iterable's proof obligation, for recursive searches using one budget.
pub fn iterator_requirement(interface: &NominalType, receiver: &TypeId) -> Option<NominalType> {
    if StandardTrait::from_id(&interface.declaration) != Some(StandardTrait::Iterable)
        || !interface.arguments.is_empty()
    {
        return None;
    }
    let item = associated_type_id(&interface.declaration, "Item");
    let iterator = associated_type_id(&interface.declaration, "Iter");
    let mut required = StandardTrait::Iterator.nominal();
    for (member, ty) in &interface.associated_types {
        if *member == item {
            required.associated_types.insert(
                associated_type_id(&required.declaration, "Item"),
                ty.clone(),
            );
        } else if *member != iterator || ty != receiver {
            return None;
        }
    }
    Some(required)
}

/// Native collection views eligible for ordinary interface dispatch.
pub fn native_interface_applies(interface: &NominalType, receiver: &TypeId) -> bool {
    if matches!(receiver, TypeId::Trait(_)) {
        return false;
    }
    StandardTrait::from_id(&interface.declaration).is_some_and(StandardTrait::dynamic)
        && intrinsic_applies(interface, receiver, None, &Default::default())
}

/// The default concrete storage family corresponding to a collection capability.
pub fn collection_storage(interface: &NominalType) -> Option<TypeId> {
    match (
        StandardTrait::from_id(&interface.declaration)?,
        interface.arguments.as_slice(),
    ) {
        (StandardTrait::List | StandardTrait::MutableList, [item]) => {
            Some(TypeId::Array(Box::new(item.clone()), Mutable))
        }
        (StandardTrait::Set | StandardTrait::MutableSet, [item]) => {
            Some(TypeId::Set(Box::new(item.clone()), Mutable))
        }
        (StandardTrait::Map | StandardTrait::MutableMap, [key, value]) => Some(TypeId::Map {
            key: Box::new(key.clone()),
            value: Box::new(value.clone()),
            access: Mutable,
        }),
        _ => None,
    }
}

pub fn parsing_error(receiver: &TypeId) -> Option<TypeId> {
    let TypeId::Builtin(kind) = receiver else {
        return None;
    };
    scalar_numeric::parsing_error(*kind).map(|kind| TypeId::StandardEnum { kind, args: vec![] })
}
