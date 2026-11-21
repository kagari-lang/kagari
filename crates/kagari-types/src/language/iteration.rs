//! Portable signatures consumed by loop lowering and source-free admission.
use super::{Protocol, binding, requirements::required_method};
use crate::{
    callable::{CallableImplementation, MethodPolicy},
    declaration::{Param, TraitDef},
    ty::{GenericBound, GenericParam, NominalTy, Ty},
};
use kagari_common::identity::{
    DefinitionKind,
    table::{DefinitionId, DefinitionTable},
};

pub(super) fn valid(
    owner: DefinitionId,
    actual: &TraitDef,
    table: &DefinitionTable,
) -> Option<bool> {
    if !actual.bounds.is_empty()
        || !actual.associated_consts.is_empty()
        || !actual.supertraits.is_empty()
        || actual.conversion_adapter.is_some()
    {
        return None;
    }
    let this = Ty::SelfType(owner);
    let method = actual.methods.first()?;
    let iterable = Protocol::Iterable.definition(table)?;
    let iterator = Protocol::Iterator.definition(table)?;
    let item = |owner| table.lookup_child(owner, DefinitionKind::AssociatedType, "Item", 0);
    let bound = |owner, member, ty| NominalTy {
        declaration: owner,
        arguments: vec![],
        associated_types: [(member, ty)].into(),
    };
    if owner != iterator && owner != iterable {
        let selector = match actual.name.as_str() {
            "FromIterator" => "from_iter",
            "Sum" => "sum",
            "Product" => "product",
            _ => return None,
        };
        let parameter = GenericParam { owner, position: 0 };
        let input = GenericParam {
            owner: table.lookup_child(owner, DefinitionKind::Method, selector, 0)?,
            position: 0,
        };
        return Some(
            actual.associated_types.is_empty()
                && actual.methods.len() == 1
                && actual.generic_params == [parameter.clone()]
                && method.name == selector
                && method.implementation == CallableImplementation::Required
                && method.method_policy == MethodPolicy::default()
                && method.host_policy.is_none()
                && method.generic_params == [input.clone()]
                && method.bounds
                    == [GenericBound {
                        ty: input.as_type(),
                        constraints: vec![bound(iterable, item(iterable)?, parameter.as_type())],
                    }]
                && method.params
                    == [Param {
                        name: "source".into(),
                        mutable: false,
                        ty: input.as_type(),
                    }]
                && method.return_type == this,
        );
    }
    if !actual.generic_params.is_empty()
        || !actual
            .associated_types
            .iter()
            .all(|a| a.generic_params.is_empty() && a.parameter_bounds.is_empty())
    {
        return None;
    }
    let member = item(owner)?;
    if !actual
        .associated_types
        .iter()
        .find(|a| a.declaration == member)?
        .bounds
        .is_empty()
    {
        return None;
    }
    let projection = |member| Ty::Projection {
        receiver: Box::new(this.clone()),
        interface: Box::new(NominalTy {
            declaration: owner,
            arguments: vec![],
            associated_types: Default::default(),
        }),
        member,
        arguments: vec![],
    };
    let (name, result) = if owner == iterator {
        if actual.associated_types.len() != 1
            || actual.methods[1..].iter().any(|m| {
                m.implementation != CallableImplementation::Script
                    || m.host_policy.is_some()
                    || m.method_policy != MethodPolicy::default()
            })
        {
            return None;
        }
        ("next", binding::option(table, projection(member))?)
    } else {
        let iter = table.lookup_child(owner, DefinitionKind::AssociatedType, "Iter", 0)?;
        if actual.associated_types.len() != 2
            || actual.methods.len() != 1
            || actual
                .associated_types
                .iter()
                .find(|a| a.declaration == iter)?
                .bounds
                != [bound(iterator, item(iterator)?, projection(member))]
        {
            return None;
        }
        ("iter", projection(iter))
    };
    Some(required_method(
        owner,
        table,
        method,
        name,
        &[("self", this)],
        &result,
    ))
}
