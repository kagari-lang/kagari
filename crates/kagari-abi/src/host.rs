//! Nominal host dependencies include signatures and layouts, even without a call.

use crate::{
    layout::{EnumLayout, LayoutValidationError, StructLayout},
    standard::{surface::StandardTypeConstraint, traits::StandardTrait},
    types::{
        self as abi, AbiType, ConstraintAbi, FunctionAbi, InterfaceTableAbi, PublicAbiItem,
        TraitAbi, TraitContract,
        substitution::{MAX_TYPE_DEPTH, MAX_TYPE_NODES},
    },
};

use kagari_common::{
    cancellation::{CancellationToken, Cancelled},
    host_interface::{
        HostInterface,
        type_declaration::{HostTraitImplementationDeclaration, HostTypeDeclaration},
        value_type::HostValueType,
    },
    identity::{DefinitionId, DefinitionKind, ModuleIdentity},
};
use std::{
    collections::{BTreeMap, BTreeSet},
    slice,
};

/// Check host method tables against the defining module's executable trait
/// contracts, including declarations absent from its public ABI.
pub fn trait_bindings_match(
    interface: &HostInterface,
    module: &ModuleIdentity,
    items: &[PublicAbiItem],
    contracts: &[TraitContract],
    cancel: &CancellationToken,
) -> Result<bool, Cancelled> {
    for host in &interface.types {
        cancel.check()?;
        for implementation in &host.trait_implementations {
            cancel.check()?;
            let id = &implementation.trait_id;
            if StandardTrait::from_id(id).is_some_and(|kind| !kind.host_implementable()) {
                return Ok(false);
            }
            if &id.module != module {
                continue;
            }
            let Some(trait_abi) = abi::trait_contract(module, items, contracts, id) else {
                return Ok(false);
            };
            if !host_trait_matches(implementation, host, trait_abi, cancel)? {
                return Ok(false);
            }
        }
    }
    Ok(true)
}

fn host_trait_matches(
    implementation: &HostTraitImplementationDeclaration,
    host: &HostTypeDeclaration,
    trait_abi: &TraitAbi,
    cancel: &CancellationToken,
) -> Result<bool, Cancelled> {
    let args = implementation
        .trait_arguments
        .iter()
        .map(AbiType::from_host_type)
        .collect::<Vec<_>>();
    let outputs: BTreeMap<_, _> = implementation
        .associated_types
        .iter()
        .map(|output| {
            (
                output.declaration.clone(),
                AbiType::from_host_type(&output.ty),
            )
        })
        .collect();
    if !trait_abi.associated_consts.is_empty()
        || trait_abi
            .associated_types
            .iter()
            .any(|member| !member.generic_params.is_empty())
        || args.len() != trait_abi.generic_params.len()
        || outputs.len() != trait_abi.associated_types.len()
        || trait_abi
            .associated_types
            .iter()
            .any(|member| !outputs.contains_key(&member.declaration))
        || implementation.methods.len() != trait_abi.methods.len()
    {
        return Ok(false);
    }
    for member in &trait_abi.associated_types {
        let output = implementation
            .associated_types
            .iter()
            .find(|output| output.declaration == member.declaration)
            .expect("validated output schema");
        for constraint in &member.bounds {
            if let ConstraintAbi::Standard(standard) = constraint
                && !satisfies_standard_constraint(&output.ty, *standard)
            {
                return Ok(false);
            }
        }
    }
    for bound in &trait_abi.bounds {
        cancel.check()?;
        let AbiType::Parameter { owner, position } = &bound.ty else {
            return Ok(false);
        };
        if owner != &implementation.trait_id {
            return Ok(false);
        }
        let Some(argument) = implementation.trait_arguments.get(*position) else {
            return Ok(false);
        };
        for constraint in &bound.constraints {
            cancel.check()?;
            if let ConstraintAbi::Standard(standard) = constraint
                && !satisfies_standard_constraint(argument, *standard)
            {
                return Ok(false);
            }
        }
    }
    for method in &trait_abi.methods {
        cancel.check()?;
        if !method.generic_params.is_empty() || method.params.is_empty() {
            return Ok(false);
        }
        let Some(binding) = implementation.methods.iter().find(|binding| {
            binding.trait_method.path.last().is_some_and(|part| {
                part.name == method.name
                    && part.kind == DefinitionKind::Method
                    && part.occurrence == 0
            })
        }) else {
            return Ok(false);
        };
        let Some(host_method) = host
            .methods
            .iter()
            .find(|candidate| candidate.id == binding.host_method)
        else {
            return Ok(false);
        };
        let receiver = AbiType::Host(host.id.clone());
        if method.params.len() != host_method.params.len() + 1
            || !matches_host_type(
                &method.params[0].ty,
                &receiver,
                &implementation.trait_id,
                &args,
                &outputs,
                &receiver,
                cancel,
            )?
        {
            return Ok(false);
        }
        for (expected, actual) in method.params[1..].iter().zip(&host_method.params) {
            cancel.check()?;
            if !matches_host_type(
                &expected.ty,
                &AbiType::from_host_type(&actual.ty),
                &implementation.trait_id,
                &args,
                &outputs,
                &receiver,
                cancel,
            )? {
                return Ok(false);
            }
        }
        if !matches_host_type(
            &method.return_type,
            &AbiType::from_host_type(&host_method.return_type),
            &implementation.trait_id,
            &args,
            &outputs,
            &receiver,
            cancel,
        )? {
            return Ok(false);
        }
    }
    Ok(true)
}

fn matches_host_type(
    expected: &AbiType,
    actual: &AbiType,
    owner: &DefinitionId,
    arguments: &[AbiType],
    outputs: &BTreeMap<DefinitionId, AbiType>,
    receiver: &AbiType,
    cancel: &CancellationToken,
) -> Result<bool, Cancelled> {
    let mut pending = vec![(expected, actual)];
    while let Some((expected, actual)) = pending.pop() {
        cancel.check()?;
        match (expected, actual) {
            (
                AbiType::Projection {
                    receiver: source,
                    interface,
                    member,
                    arguments: member_arguments,
                },
                actual,
            ) if member_arguments.is_empty()
                && interface.declaration == *owner
                && matches!(source.as_ref(), AbiType::SelfType(id) if id == owner) =>
            {
                let Some(output) = outputs.get(member) else {
                    return Ok(false);
                };
                pending.push((output, actual));
            }
            (AbiType::SelfType(id), actual) if id == owner => pending.push((receiver, actual)),
            (
                AbiType::Parameter {
                    owner: id,
                    position,
                },
                actual,
            ) if id == owner => {
                let Some(argument) = arguments.get(*position) else {
                    return Ok(false);
                };
                pending.push((argument, actual));
            }
            (AbiType::Builtin(a), AbiType::Builtin(b)) if a == b => {}
            (AbiType::Host(a), AbiType::Host(b)) if a == b => {}
            (AbiType::Tuple(a), AbiType::Tuple(b)) if a.len() == b.len() => {
                pending.extend(a.iter().zip(b));
            }
            (
                AbiType::Function {
                    params: ap,
                    result: ar,
                },
                AbiType::Function {
                    params: bp,
                    result: br,
                },
            ) if ap.len() == bp.len() => {
                pending.push((ar, br));
                pending.extend(ap.iter().zip(bp));
            }
            (AbiType::Iter(a), AbiType::Iter(b))
            | (AbiType::Array(a, _), AbiType::Array(b, _))
            | (AbiType::Set(a, _), AbiType::Set(b, _)) => {
                pending.push((a, b));
            }
            (
                AbiType::Map {
                    key: ak, value: av, ..
                },
                AbiType::Map {
                    key: bk, value: bv, ..
                },
            ) => {
                pending.extend([(ak.as_ref(), bk.as_ref()), (av.as_ref(), bv.as_ref())]);
            }
            (
                AbiType::StandardEnum { kind: ak, args: aa },
                AbiType::StandardEnum { kind: bk, args: ba },
            ) if ak == bk && aa.len() == ba.len() => pending.extend(aa.iter().zip(ba)),
            _ => return Ok(false),
        }
    }
    Ok(true)
}

pub fn references(
    items: &[PublicAbiItem],
    structures: &[StructLayout],
    enums: &[EnumLayout],
    cancel: &CancellationToken,
) -> Result<BTreeSet<DefinitionId>, Cancelled> {
    let mut pending = Vec::new();
    for item in items {
        cancel.check()?;
        let functions: &[FunctionAbi] = match item {
            PublicAbiItem::Function(function) => slice::from_ref(function),
            PublicAbiItem::Const(value) => {
                pending.push(&value.ty);
                &[]
            }
            PublicAbiItem::Type(ty) => {
                pending.extend(ty.fields.iter().map(|field| &field.ty));
                pending.extend(ty.variants.iter().flat_map(|variant| &variant.payload));
                &[]
            }
            PublicAbiItem::Trait(ty) => &ty.methods,
            PublicAbiItem::InterfaceTable(table) => {
                pending.extend([&table.trait_type, &table.for_type]);
                &table.methods
            }
        };
        for function in functions {
            pending.extend(function.params.iter().map(|param| &param.ty));
            pending.push(&function.return_type);
        }
    }
    for layout in structures {
        pending.extend(&layout.arguments);
        pending.extend(layout.fields.iter().map(|field| &field.ty));
    }
    for layout in enums {
        pending.extend(&layout.arguments);
        pending.extend(layout.variants.iter().flat_map(|variant| &variant.payload));
    }
    let mut result = BTreeSet::new();
    while let Some(ty) = pending.pop() {
        cancel.check()?;
        match ty {
            AbiType::Host(id) => {
                result.insert(id.clone());
            }
            AbiType::Tuple(types) | AbiType::StandardEnum { args: types, .. } => {
                pending.extend(types)
            }
            AbiType::Function { params, result } => {
                pending.extend(params);
                pending.push(result);
            }
            AbiType::Array(ty, _) | AbiType::Set(ty, _) | AbiType::Iter(ty) => pending.push(ty),
            AbiType::Map { key, value, .. } => pending.extend([key.as_ref(), value.as_ref()]),
            AbiType::Struct(ty) | AbiType::Enum(ty) | AbiType::Trait(ty) => {
                pending.extend(&ty.arguments);
                pending.extend(ty.associated_types.values());
            }
            _ => {}
        }
    }
    Ok(result)
}

pub fn validate(
    interface: &HostInterface,
    items: &[PublicAbiItem],
    structures: &[StructLayout],
    enums: &[EnumLayout],
    cancel: &CancellationToken,
) -> Result<(), LayoutValidationError> {
    cancel
        .check()
        .map_err(|_| LayoutValidationError::Cancelled)?;
    interface
        .validate()
        .map_err(|_| LayoutValidationError::Invalid)?;
    if items.iter().any(|item| {
        matches!(item, PublicAbiItem::InterfaceTable(table)
            if table.host_bridge && host_bridge_implementation(table, interface).is_none())
    }) {
        return Err(LayoutValidationError::Invalid);
    }
    let ids: BTreeSet<_> = interface.types.iter().map(|ty| &ty.id).collect();
    if references(items, structures, enums, cancel)
        .map_err(|_| LayoutValidationError::Cancelled)?
        .iter()
        .all(|id| ids.contains(id))
    {
        Ok(())
    } else {
        Err(LayoutValidationError::Invalid)
    }
}

pub fn host_bridge_implementation<'a>(
    table: &InterfaceTableAbi,
    interface: &'a HostInterface,
) -> Option<(
    &'a HostTypeDeclaration,
    &'a HostTraitImplementationDeclaration,
)> {
    let AbiType::Host(id) = &table.for_type else {
        return None;
    };
    let AbiType::Trait(applied) = &table.trait_type else {
        return None;
    };
    let host = interface.types.iter().find(|host| &host.id == id)?;
    let implementation = host.trait_implementations.iter().find(|implementation| {
        implementation.trait_id == applied.declaration
            && implementation.trait_arguments.len() == applied.arguments.len()
            && implementation
                .trait_arguments
                .iter()
                .zip(&applied.arguments)
                .all(|(expected, actual)| AbiType::from_host_type(expected) == *actual)
            && implementation.associated_types.len() == applied.associated_types.len()
            && implementation.associated_types.iter().all(|output| {
                applied
                    .associated_types
                    .get(&output.declaration)
                    .is_some_and(|actual| AbiType::from_host_type(&output.ty) == *actual)
            })
    })?;
    Some((host, implementation))
}

/// Intrinsic standard constraints of portable host values. Collections use shared
/// identity; tuple/Option/Result equality and hashing recurse into their payloads.
/// Nominal host objects need declared trait implementations, not this fallback.
pub fn satisfies_standard_constraint(
    ty: &HostValueType,
    constraint: StandardTypeConstraint,
) -> bool {
    if matches!(
        constraint,
        StandardTypeConstraint::OrderedNumber | StandardTypeConstraint::SignedNumber
    ) {
        return matches!(
            ty,
            HostValueType::I32 | HostValueType::I64 | HostValueType::F32 | HostValueType::F64
        );
    }
    let mut remaining = MAX_TYPE_NODES;
    let mut pending = vec![(ty, 1usize)];
    while let Some((ty, depth)) = pending.pop() {
        if remaining == 0 || depth > MAX_TYPE_DEPTH {
            return false;
        }
        remaining -= 1;
        match ty {
            HostValueType::Opaque(_) => return false,
            HostValueType::F32 | HostValueType::F64
                if constraint == StandardTypeConstraint::HashKey =>
            {
                return false;
            }
            HostValueType::Tuple(items) => {
                if items.len() > remaining {
                    return false;
                }
                pending.extend(items.iter().map(|ty| (ty, depth + 1)));
            }
            HostValueType::Option(ty) => pending.push((ty, depth + 1)),
            HostValueType::Result { ok, error } => {
                pending.extend([(ok.as_ref(), depth + 1), (error.as_ref(), depth + 1)])
            }
            HostValueType::Unit
            | HostValueType::Bool
            | HostValueType::I32
            | HostValueType::I64
            | HostValueType::F32
            | HostValueType::F64
            | HostValueType::String
            | HostValueType::Array(_, _)
            | HostValueType::Map { .. }
            | HostValueType::Set(_, _) => {}
        }
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use kagari_common::collection::CollectionAccess;

    #[test]
    fn host_standard_constraints_distinguish_payload_and_collection_identity() {
        let float = HostValueType::F64;
        assert!(satisfies_standard_constraint(
            &float,
            StandardTypeConstraint::Comparable
        ));
        assert!(!satisfies_standard_constraint(
            &float,
            StandardTypeConstraint::HashKey
        ));
        assert!(!satisfies_standard_constraint(
            &HostValueType::Option(Box::new(float.clone())),
            StandardTypeConstraint::HashKey
        ));
        let array = HostValueType::Array(Box::new(float), CollectionAccess::ReadOnly);
        assert!(satisfies_standard_constraint(
            &array,
            StandardTypeConstraint::HashKey
        ));
        assert!(satisfies_standard_constraint(
            &HostValueType::Tuple(vec![array, HostValueType::I32]),
            StandardTypeConstraint::Comparable
        ));
        assert!(!satisfies_standard_constraint(
            &HostValueType::Bool,
            StandardTypeConstraint::SignedNumber
        ));
        assert!(satisfies_standard_constraint(
            &HostValueType::F32,
            StandardTypeConstraint::SignedNumber
        ));
    }
}
