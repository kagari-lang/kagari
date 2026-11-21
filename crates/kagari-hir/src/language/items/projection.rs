//! Project checked HIR headers for the portable language-shape validators.
use crate::{
    aggregates::{
        EnumSignature,
        traits::{MethodDefault, MethodSignature, TraitSignature},
    },
    types::{
        GenericParameterType, NominalType, TypeId,
        semantic::{lower_nominal_type, lower_type},
    },
};
use kagari_common::identity::{DefinitionKind, table::DefinitionTable};
use kagari_types::{
    callable::CallableImplementation,
    declaration::{AssociatedTypeDef, FnDecl, Param, TraitDef, TypeDef, TypeDefKind, VariantDef},
    ty::{GenericParam, NominalTy, Ty},
};

fn parameters(inputs: &[GenericParameterType]) -> Vec<GenericParam> {
    inputs
        .iter()
        .map(|param| GenericParam {
            owner: param.owner,
            position: param.position,
        })
        .collect()
}

fn ty(actual: &TypeId) -> Option<Ty> {
    (!actual.is_unresolved()).then(|| lower_type(actual))
}

fn nominal(actual: &NominalType) -> Option<NominalTy> {
    (!actual
        .arguments
        .iter()
        .chain(actual.associated_types.values())
        .any(TypeId::is_unresolved))
    .then(|| lower_nominal_type(actual))
}

fn method(
    actual: &MethodSignature,
    enclosing: &TraitSignature,
    slot: usize,
    table: &DefinitionTable,
) -> Option<FnDecl> {
    if actual.owner != enclosing.id
        || actual.slot != slot
        || table.lookup_child(enclosing.id, DefinitionKind::Method, &actual.name, 0)
            != Some(actual.id)
        || actual.bounds.values().any(|bounds| !bounds.is_empty())
    {
        return None;
    }
    let local = actual
        .generic_params
        .strip_prefix(enclosing.generic_params.as_slice())?;
    let (implementation, host_policy) = match &actual.default {
        None => (CallableImplementation::Required, None),
        Some(MethodDefault::Script) => (CallableImplementation::Script, None),
        Some(MethodDefault::Native(native)) => (
            CallableImplementation::Native(actual.id),
            native.host_policy.clone(),
        ),
    };
    Some(FnDecl {
        name: actual.name.clone(),
        implementation,
        host_policy,
        method_policy: actual.policy,
        generic_params: parameters(local),
        bounds: vec![],
        params: actual
            .params
            .iter()
            .map(|param| {
                Some(Param {
                    name: param.name.clone(),
                    mutable: param.writeability.is_var(),
                    ty: ty(&param.ty)?,
                })
            })
            .collect::<Option<_>>()?,
        return_type: ty(&actual.return_type)?,
    })
}

/// Comparison and static conversion both require unconstrained binders, with
/// no associated constants or generic families. Reject unsupported HIR facts
/// rather than erasing them to make a declaration fit the portable check.
pub(super) fn simple_trait(actual: &TraitSignature, table: &DefinitionTable) -> Option<TraitDef> {
    if !actual.associated_consts.is_empty()
        || !actual.associated_type_parameters.is_empty()
        || actual.bounds.values().any(|bounds| !bounds.is_empty())
    {
        return None;
    }
    Some(TraitDef {
        name: actual.declaration.name.clone(),
        generic_params: parameters(&actual.generic_params),
        bounds: vec![],
        supertraits: actual
            .supertraits
            .iter()
            .map(nominal)
            .collect::<Option<_>>()?,
        associated_consts: vec![],
        conversion_adapter: actual.conversion_adapter.clone(),
        associated_types: actual
            .associated_types
            .iter()
            .map(|(&declaration, bounds)| {
                Some(AssociatedTypeDef {
                    declaration,
                    bounds: bounds.iter().map(nominal).collect::<Option<_>>()?,
                    generic_params: vec![],
                    parameter_bounds: vec![],
                })
            })
            .collect::<Option<_>>()?,
        methods: actual
            .methods
            .iter()
            .enumerate()
            .map(|(slot, member)| method(member, actual, slot, table))
            .collect::<Option<_>>()?,
    })
}

/// Portable enum schemas identify variants by owner, kind, name and declaration
/// order. Verify the explicit HIR coordinates before projecting away their IDs.
pub(super) fn enumeration(actual: &EnumSignature, table: &DefinitionTable) -> Option<TypeDef> {
    if actual.bounds.values().any(|bounds| !bounds.is_empty()) {
        return None;
    }
    Some(TypeDef {
        name: actual.declaration.name.clone(),
        kind: TypeDefKind::Enum,
        fields: vec![],
        generic_params: parameters(&actual.generic_params),
        bounds: vec![],
        variants: actual
            .variants
            .iter()
            .enumerate()
            .map(|(slot, variant)| {
                if variant.owner != actual.id
                    || variant.slot != slot
                    || table.lookup_child(actual.id, DefinitionKind::Variant, &variant.name, 0)
                        != Some(variant.id)
                {
                    return None;
                }
                Some(VariantDef {
                    name: variant.name.clone(),
                    reports_failure: variant.reports_failure,
                    payload: variant.payload.iter().map(ty).collect::<Option<_>>()?,
                })
            })
            .collect::<Option<_>>()?,
    })
}
