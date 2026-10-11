//! Explicit registrations of reserved language traits; no compiled metadata input.
use crate::catalog::{
    contracts, identities,
    key::{self, RegistrationTrait},
    propagation,
};
use kagari_common::identity::map::DefinitionContext;
use kagari_types::{
    declaration::{TraitDef, module::DeclarationError},
    scalar::BuiltinType,
    ty::{Constraint, NominalTy, Ty},
};
use std::collections::BTreeMap;

fn projection(
    ids: &DefinitionContext,
    kind: RegistrationTrait,
    name: &str,
    arguments: Vec<Ty>,
) -> Result<Ty, DeclarationError> {
    let owner = key::identity(ids, kind)?;
    Ok(Ty::Projection {
        receiver: Box::new(Ty::SelfType(owner)),
        interface: Box::new(NominalTy {
            declaration: owner,
            arguments,
            associated_types: BTreeMap::new(),
        }),
        member: identities::associated(ids, &owner, name)?,
        arguments: vec![],
    })
}

fn operator(
    ids: &DefinitionContext,
    kind: RegistrationTrait,
    name: &str,
    binary: bool,
) -> Result<TraitDef, DeclarationError> {
    let mut definition = contracts::contract(ids, kind, if binary { &["T0"] } else { &[] })?;
    definition.associated_types.push(contracts::associated(
        ids,
        &key::identity(ids, kind)?,
        "Output",
        vec![],
    )?);
    let mut params = vec![contracts::receiver(ids, kind)?];
    let arguments = definition
        .generic_params
        .iter()
        .map(|parameter| parameter.as_type())
        .collect::<Vec<_>>();
    params.extend(arguments.clone());
    definition.methods.push(contracts::method(
        name,
        params,
        projection(ids, kind, "Output", arguments)?,
    ));
    Ok(definition)
}

pub(super) fn declarations(ids: &DefinitionContext) -> Result<Vec<TraitDef>, DeclarationError> {
    let iterator_kind = RegistrationTrait::Iterator;
    let mut iterator = contracts::contract(ids, iterator_kind, &[])?;
    iterator.associated_types.push(contracts::associated(
        ids,
        &key::identity(ids, iterator_kind)?,
        "Item",
        vec![],
    )?);
    iterator.methods.push(contracts::method(
        "next",
        vec![contracts::receiver(ids, iterator_kind)?],
        contracts::option(ids, projection(ids, iterator_kind, "Item", vec![])?)?,
    ));

    let iterable_kind = RegistrationTrait::Iterable;
    let mut iterable = contracts::contract(ids, iterable_kind, &[])?;
    iterable.associated_types.push(contracts::associated(
        ids,
        &key::identity(ids, iterable_kind)?,
        "Item",
        vec![],
    )?);
    let mut iter_bound = key::applied(ids, iterator_kind, vec![])?;
    iter_bound.associated_types.insert(
        identities::associated(ids, &key::identity(ids, iterator_kind)?, "Item")?,
        projection(ids, iterable_kind, "Item", vec![])?,
    );
    iterable.associated_types.push(contracts::associated(
        ids,
        &key::identity(ids, iterable_kind)?,
        "Iter",
        vec![Constraint::Trait(iter_bound)],
    )?);
    iterable.methods.push(contracts::method(
        "iter",
        vec![contracts::receiver(ids, iterable_kind)?],
        projection(ids, iterable_kind, "Iter", vec![])?,
    ));

    let mut partial_eq = contracts::contract(ids, RegistrationTrait::PartialEq, &[])?;
    partial_eq.methods.push(contracts::method(
        "eq",
        vec![contracts::receiver(ids, RegistrationTrait::PartialEq)?; 2],
        Ty::Builtin(BuiltinType::Bool),
    ));
    let mut eq = contracts::contract(ids, RegistrationTrait::Eq, &[])?;
    eq.supertraits
        .push(key::applied(ids, RegistrationTrait::PartialEq, vec![])?);
    let mut hash = contracts::contract(ids, RegistrationTrait::Hash, &[])?;
    hash.methods.push(contracts::method(
        "hash",
        vec![contracts::receiver(ids, RegistrationTrait::Hash)?],
        Ty::Builtin(BuiltinType::I64),
    ));
    let ordering = contracts::enum_type(ids, "Ordering", vec![])?;
    let mut partial_ord = contracts::contract(ids, RegistrationTrait::PartialOrd, &[])?;
    partial_ord
        .supertraits
        .push(key::applied(ids, RegistrationTrait::PartialEq, vec![])?);
    partial_ord.methods.push(contracts::method(
        "partial_cmp",
        vec![contracts::receiver(ids, RegistrationTrait::PartialOrd)?; 2],
        contracts::option(ids, ordering.clone())?,
    ));
    let mut ord = contracts::contract(ids, RegistrationTrait::Ord, &[])?;
    ord.supertraits.extend([
        key::applied(ids, RegistrationTrait::Eq, vec![])?,
        key::applied(ids, RegistrationTrait::PartialOrd, vec![])?,
    ]);
    ord.methods.push(contracts::method(
        "cmp",
        vec![contracts::receiver(ids, RegistrationTrait::Ord)?; 2],
        ordering,
    ));

    let mut debug = contracts::contract(ids, RegistrationTrait::Debug, &[])?;
    debug.methods.push(contracts::method(
        "debug",
        vec![contracts::receiver(ids, RegistrationTrait::Debug)?],
        Ty::Builtin(BuiltinType::String),
    ));
    let mut display = contracts::contract(ids, RegistrationTrait::Display, &[])?;
    display.methods.push(contracts::method(
        "display",
        vec![contracts::receiver(ids, RegistrationTrait::Display)?],
        Ty::Builtin(BuiltinType::String),
    ));
    let mut from = contracts::contract(ids, RegistrationTrait::From, &["T0"])?;
    let mut from_method = contracts::method(
        "from",
        vec![from.generic_params[0].as_type()],
        contracts::receiver(ids, RegistrationTrait::From)?,
    );
    from_method.params[0].name = "value".into();
    from.methods.push(from_method);

    Ok(vec![
        iterator,
        iterable,
        partial_eq,
        eq,
        hash,
        partial_ord,
        ord,
        operator(ids, RegistrationTrait::Add, "add", true)?,
        operator(ids, RegistrationTrait::Sub, "sub", true)?,
        operator(ids, RegistrationTrait::Mul, "mul", true)?,
        operator(ids, RegistrationTrait::Div, "div", true)?,
        operator(ids, RegistrationTrait::Rem, "rem", true)?,
        operator(ids, RegistrationTrait::BitAnd, "bitand", true)?,
        operator(ids, RegistrationTrait::BitOr, "bitor", true)?,
        operator(ids, RegistrationTrait::BitXor, "bitxor", true)?,
        operator(ids, RegistrationTrait::Shl, "shl", true)?,
        operator(ids, RegistrationTrait::Shr, "shr", true)?,
        operator(ids, RegistrationTrait::Neg, "neg", false)?,
        operator(ids, RegistrationTrait::Not, "not", false)?,
        operator(ids, RegistrationTrait::Index, "index", true)?,
        operator(ids, RegistrationTrait::Fn, "call", true)?,
        debug,
        display,
        from,
    ]
    .into_iter()
    .chain(propagation::traits(ids)?)
    .collect())
}
