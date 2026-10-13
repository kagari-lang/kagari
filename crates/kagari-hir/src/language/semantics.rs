//! Language protocols have declaration identities and ordinary trait contracts.

use crate::builtin::surface;
use crate::{
    aggregates::{AggregateCatalog, implementations::ImplementationSearchLimits},
    typeck::{GenericBounds, table::ConstraintTarget},
    types::{NominalType, TypeId},
};
use kagari_common::identity::{
    DefinitionKind,
    table::{DefinitionId, DefinitionTable},
};
use kagari_types::{
    conversion as scalar_numeric,
    declaration::conversion::ConversionAdapter,
    language::{Protocol, binding, role::LangRole},
    scalar::BuiltinType,
};
use std::collections::{BTreeMap, HashSet};

/// HIR type views of compiler-recognized protocols whose declarations remain ordinary traits.
pub trait ProtocolSemantics {
    /// Builds the protocol's canonical nominal identity before applying arguments.
    fn nominal(self, definitions: &DefinitionTable) -> Option<NominalType>;

    /// Applies intrinsic receiver-dependent arguments and associated outputs.
    fn intrinsic_view(
        self,
        definitions: &DefinitionTable,
        receiver: &TypeId,
    ) -> Option<NominalType>;
}

impl ProtocolSemantics for Protocol {
    fn nominal(self, definitions: &DefinitionTable) -> Option<NominalType> {
        Some(NominalType {
            declaration: self.definition(definitions)?,
            arguments: vec![],
            associated_types: Default::default(),
        })
    }

    fn intrinsic_view(
        self,
        definitions: &DefinitionTable,
        receiver: &TypeId,
    ) -> Option<NominalType> {
        let mut view = self.nominal(definitions)?;
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
        if let Some(output) = intrinsic_output(definitions, &view, receiver) {
            view.associated_types.insert(
                definitions.lookup_child(
                    view.declaration,
                    DefinitionKind::AssociatedType,
                    "Output",
                    0,
                )?,
                output,
            );
        }
        Some(view)
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
pub fn callable_signature(
    definitions: &DefinitionTable,
    interface: &NominalType,
) -> Option<TypeId> {
    if Protocol::from_definition(&interface.declaration, definitions) != Some(Protocol::Fn) {
        return None;
    }
    let params = match interface.arguments.as_slice() {
        [TypeId::Tuple(params)] => params.clone(),
        [TypeId::Builtin(BuiltinType::Unit)] => vec![],
        _ => return None,
    };
    let result = interface
        .associated_types
        .get(&definitions.lookup_child(
            interface.declaration,
            DefinitionKind::AssociatedType,
            "Output",
            0,
        )?)
        .cloned()
        .unwrap_or(TypeId::Unknown);
    Some(TypeId::Function {
        params,
        result: Box::new(result),
    })
}

/// Builtin associated outputs are computed from the applied protocol, not its spelling.
pub fn intrinsic_output(
    definitions: &DefinitionTable,
    interface: &NominalType,
    receiver: &TypeId,
) -> Option<TypeId> {
    let kind = Protocol::from_definition(&interface.declaration, definitions)?;
    if kind == Protocol::Fn
        && let TypeId::Function { params, result } = receiver
    {
        return (interface.arguments == [callable_arguments(params)]).then(|| (**result).clone());
    }
    if kind == Protocol::Index
        && let TypeId::Array(element) = receiver
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
        && (kind == Protocol::Not && *receiver == TypeId::Builtin(BuiltinType::Bool)
            || kind == Protocol::Neg
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
        Protocol::BitAnd | Protocol::BitOr | Protocol::BitXor | Protocol::Shl | Protocol::Shr
    ) {
        let [TypeId::Builtin(rhs)] = interface.arguments.as_slice() else {
            return None;
        };
        let TypeId::Builtin(lhs) = receiver else {
            return None;
        };
        return (lhs.integer_layout().is_some()
            && rhs.integer_layout().is_some()
            && (matches!(kind, Protocol::Shl | Protocol::Shr) || lhs == rhs))
            .then(|| receiver.clone());
    }
    if kind == Protocol::Not
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

/// Checks whether a protocol application is supported by intrinsic receiver semantics and available assumptions.
pub fn intrinsic_applies(
    definitions: &DefinitionTable,
    interface: &NominalType,
    receiver: &TypeId,
    catalog: Option<&AggregateCatalog>,
    bounds: &GenericBounds,
) -> bool {
    if catalog.is_some_and(|catalog| {
        catalog
            .engine_implementation(definitions, interface, receiver, bounds)
            .is_some()
    }) {
        return true;
    }
    if let Some((required, target)) = conversion_requirement(interface, receiver, catalog) {
        return intrinsic_applies(definitions, &required, &target, catalog, bounds)
            || bounds.get(&target).is_some_and(|available| available.iter().any(|bound| matches!(bound, ConstraintTarget::Trait(bound) if bound.satisfies(&required))))
            || catalog.is_some_and(|catalog| catalog.concrete_interface_implementation(definitions, &required, &target, bounds, ImplementationSearchLimits { max_checks: 100_000, max_depth: 64 }, &Default::default()).is_ok_and(|found| found.is_some()));
    }
    let Some(kind) = Protocol::from_definition(&interface.declaration, definitions) else {
        return false;
    };
    if kind.iteration() {
        return iteration_outputs(definitions, kind, receiver, catalog, bounds).is_some_and(
            |outputs| {
                interface.arguments.is_empty()
                    && interface
                        .associated_types
                        .iter()
                        .all(|(member, ty)| outputs.get(member) == Some(ty))
            },
        );
    }
    if kind == Protocol::From {
        return interface.arguments.as_slice() == [receiver.clone()]
            && interface.associated_types.is_empty()
            || matches!((receiver, interface.arguments.as_slice()), (TypeId::Builtin(target), [TypeId::Builtin(source)]) if interface.associated_types.is_empty() && scalar_numeric::lossless_from(*source, *target));
    }
    if kind.operator() {
        let Some(output) = intrinsic_output(definitions, interface, receiver) else {
            return false;
        };
        interface.associated_types.iter().all(|(member, ty)| {
            Some(*member)
                == definitions.lookup_child(
                    interface.declaration,
                    DefinitionKind::AssociatedType,
                    "Output",
                    0,
                )
                && *ty == output
        })
    } else {
        interface.arguments.is_empty()
            && interface.associated_types.is_empty()
            && intrinsic_holds(definitions, kind, receiver, catalog, bounds)
    }
}

/// Builds the ordinary Ordering type, optionally wrapped in the registered Option type.
pub fn ordering_type(definitions: &DefinitionTable, optional: bool) -> TypeId {
    let Some(ordering) = definitions.lookup(&binding::ordering_declaration()) else {
        return TypeId::Error;
    };
    let ordering = TypeId::Enum(NominalType {
        declaration: ordering,
        arguments: vec![],
        associated_types: Default::default(),
    });
    if !optional {
        return ordering;
    }
    let Some(option) = definitions.lookup(&binding::option_declaration()) else {
        return TypeId::Error;
    };
    TypeId::Enum(NominalType {
        declaration: option,
        arguments: vec![ordering],
        associated_types: Default::default(),
    })
}

/// Intrinsic implementations preserve the language's value/identity contract.
/// Explicit Debug/Display implementations are selected before intrinsic fallbacks.
pub fn intrinsic_holds(
    definitions: &DefinitionTable,
    protocol: Protocol,
    ty: &TypeId,
    catalog: Option<&AggregateCatalog>,
    bounds: &GenericBounds,
) -> bool {
    if LangRole::from_protocol(protocol).is_none() {
        return false;
    }
    if protocol.iteration() {
        return iteration_outputs(definitions, protocol, ty, catalog, bounds).is_some();
    }
    if matches!(
        protocol,
        Protocol::From | Protocol::Try | Protocol::FromResidual
    ) {
        return false;
    }
    if protocol.operator() {
        return protocol
            .intrinsic_view(definitions, ty)
            .is_some_and(|interface| intrinsic_output(definitions, &interface, ty).is_some());
    }
    if matches!(protocol, Protocol::PartialOrd | Protocol::Ord) {
        if bounds.get(ty).is_some_and(|constraints| constraints.iter().any(|c| matches!(c, ConstraintTarget::Trait(n) if Some(n.declaration) == protocol.definition(definitions) || protocol == Protocol::PartialOrd && Some(n.declaration) == Protocol::Ord.definition(definitions)))) {return true;}
        if let Some(catalog) = catalog
            && let Some(interface) = protocol.nominal(definitions)
            && matches!(
                catalog.concrete_interface_implementation(
                    definitions,
                    &interface,
                    ty,
                    bounds,
                    ImplementationSearchLimits {
                        max_checks: 4096,
                        max_depth: 64
                    },
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
                protocol == Protocol::PartialOrd
            }
            TypeId::Builtin(_) => true,
            TypeId::Enum(nominal)
                if Some(nominal.declaration)
                    == definitions.lookup(&binding::ordering_declaration()) =>
            {
                true
            }
            _ => false,
        };
    }
    if protocol.equality_protocol()
        && let Some(catalog) = catalog
    {
        return catalog.standard_protocol_holds(definitions, protocol, ty, bounds);
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
        if bounds.get(&ty).is_some_and(|bounds| bounds.iter().any(|bound| matches!(bound, ConstraintTarget::Trait(n) if Protocol::from_definition(&n.declaration, definitions).is_some_and(|p| p == protocol || p == Protocol::Eq && protocol == Protocol::PartialEq)))) { continue; }
        match ty {
            // Recovery holes cannot disprove a protocol; code generation rejects them.
            TypeId::Unknown | TypeId::Error => {}
            TypeId::Builtin(b) => {
                if matches!(protocol, Protocol::Eq | Protocol::Hash)
                    && matches!(b, BuiltinType::F32 | BuiltinType::F64)
                {
                    return false;
                }
            }
            TypeId::Enum(_) | TypeId::Host(_) if protocol == Protocol::Debug => {}
            TypeId::NativeObject(_)
            | TypeId::Struct(_)
            | TypeId::Array(_)
            | TypeId::Map { .. }
            | TypeId::Set(_, _)
                if protocol == Protocol::Debug => {}
            TypeId::Tuple(elements) if protocol != Protocol::Display => {
                pending.extend(elements.into_iter().map(|ty| (ty, depth + 1)))
            }
            TypeId::Enum(instance)
                if protocol != Protocol::Display
                    && binding::member_comparison(&instance.declaration, definitions) =>
            {
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

/// Returns supported Iterable/Iterator associated outputs under the current catalog/bounds, or `None`.
pub fn iteration_outputs(
    definitions: &DefinitionTable,
    kind: Protocol,
    receiver: &TypeId,
    catalog: Option<&AggregateCatalog>,
    bounds: &GenericBounds,
) -> Option<BTreeMap<DefinitionId, TypeId>> {
    if let TypeId::Trait(interface) = receiver {
        let inherited = catalog?
            .interface_closure(definitions, interface, receiver, &Default::default())
            .ok()?;
        if let Some(parent) = inherited
            .iter()
            .find(|parent| Some(parent.declaration) == kind.definition(definitions))
        {
            return Some(parent.associated_types.clone());
        }
        if kind == Protocol::Iterable
            && let Some(iterator) = inherited.iter().find(|parent| {
                Some(parent.declaration) == Protocol::Iterator.definition(definitions)
            })
        {
            let item = iterator
                .associated_types
                .get(&definitions.lookup_child(
                    iterator.declaration,
                    DefinitionKind::AssociatedType,
                    "Item",
                    0,
                )?)?
                .clone();
            return Some(BTreeMap::from([
                (
                    definitions.lookup_child(
                        kind.definition(definitions)?,
                        DefinitionKind::AssociatedType,
                        "Item",
                        0,
                    )?,
                    item,
                ),
                (
                    definitions.lookup_child(
                        kind.definition(definitions)?,
                        DefinitionKind::AssociatedType,
                        "Iter",
                        0,
                    )?,
                    receiver.clone(),
                ),
            ]));
        }
        return None;
    }
    if let Some((implementation, arguments)) = catalog.and_then(|catalog| {
        catalog
            .concrete_interface_implementation(
                definitions,
                &kind.nominal(definitions)?,
                receiver,
                bounds,
                ImplementationSearchLimits {
                    max_checks: 4096,
                    max_depth: 64,
                },
                &Default::default(),
            )
            .ok()
            .flatten()
    }) {
        let catalog = catalog?;
        let implementation = catalog.implementation_signature(&implementation)?;
        let substitution = implementation
            .generic_params
            .iter()
            .cloned()
            .zip(arguments)
            .collect();
        return Some(
            implementation
                .trait_type
                .associated_types
                .iter()
                .map(|(member, ty)| (*member, ty.instantiate(&substitution)))
                .collect(),
        );
    }
    if kind != Protocol::Iterable {
        return None;
    }
    let iterator = Protocol::Iterator.nominal(definitions)?;
    let available = bounds
        .get(receiver)
        .into_iter()
        .flatten()
        .any(|b| matches!(b,ConstraintTarget::Trait(n) if n.declaration==iterator.declaration))
        || catalog.is_some_and(|c| {
            c.concrete_interface_implementation(
                definitions,
                &iterator,
                receiver,
                bounds,
                ImplementationSearchLimits {
                    max_checks: 4096,
                    max_depth: 64,
                },
                &Default::default(),
            )
            .is_ok_and(|i| i.is_some())
        });
    if !available {
        return None;
    }
    let member = definitions.lookup_child(
        iterator.declaration,
        DefinitionKind::AssociatedType,
        "Item",
        0,
    )?;
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
            catalog.map_or(projection.clone(), |c| {
                c.normalize_type(definitions, &projection)
            })
        });
    let id = kind.definition(definitions)?;
    Some(
        [
            (
                definitions.lookup_child(id, DefinitionKind::AssociatedType, "Item", 0)?,
                item,
            ),
            (
                definitions.lookup_child(id, DefinitionKind::AssociatedType, "Iter", 0)?,
                receiver.clone(),
            ),
        ]
        .into_iter()
        .collect(),
    )
}

/// Identity Iterable's proof obligation, for recursive searches using one budget.
pub fn iterator_requirement(
    definitions: &DefinitionTable,
    interface: &NominalType,
    receiver: &TypeId,
) -> Option<NominalType> {
    if Protocol::from_definition(&interface.declaration, definitions) != Some(Protocol::Iterable)
        || !interface.arguments.is_empty()
    {
        return None;
    }
    let item = definitions.lookup_child(
        interface.declaration,
        DefinitionKind::AssociatedType,
        "Item",
        0,
    )?;
    let iterator = definitions.lookup_child(
        interface.declaration,
        DefinitionKind::AssociatedType,
        "Iter",
        0,
    )?;
    let mut required = Protocol::Iterator.nominal(definitions)?;
    for (member, ty) in &interface.associated_types {
        if *member == item {
            required.associated_types.insert(
                definitions.lookup_child(
                    required.declaration,
                    DefinitionKind::AssociatedType,
                    "Item",
                    0,
                )?,
                ty.clone(),
            );
        } else if *member != iterator || ty != receiver {
            return None;
        }
    }
    Some(required)
}

/// Reverse conversions retain the registered forward member and error identities.
pub fn conversion_requirement(
    interface: &NominalType,
    receiver: &TypeId,
    catalog: Option<&AggregateCatalog>,
) -> Option<(NominalType, TypeId)> {
    let ConversionAdapter::Reverse { origin, error, .. } = catalog?
        .trait_(&interface.declaration)?
        .conversion_adapter
        .as_ref()?
    else {
        return None;
    };
    let [target] = interface.arguments.as_slice() else {
        return None;
    };
    let mut required = NominalType {
        declaration: *origin,
        arguments: vec![receiver.clone()],
        associated_types: Default::default(),
    };
    for (member, ty) in &interface.associated_types {
        let (source, target) = error.as_ref()?;
        if member != source {
            return None;
        }
        required.associated_types.insert(*target, ty.clone());
    }
    Some((required, target.clone()))
}
