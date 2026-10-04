//! Explicit registrations of reserved language traits; no compiled metadata input.
use crate::catalog::{contracts, key, key::RegistrationTrait};
use kagari_common::identity::associated_type_id;
use kagari_types::{
    declaration::TraitDef,
    scalar::BuiltinType,
    surface::StandardEnum,
    ty::{Constraint, NominalTy, Ty},
};
use std::collections::BTreeMap;

fn projection(kind: RegistrationTrait, name: &str, arguments: Vec<Ty>) -> Ty {
    let owner = key::identity(kind);
    Ty::Projection {
        receiver: Box::new(Ty::SelfType(owner.clone())),
        interface: Box::new(NominalTy {
            declaration: owner.clone(),
            arguments,
            associated_types: BTreeMap::new(),
        }),
        member: associated_type_id(&owner, name),
        arguments: vec![],
    }
}

fn operator(kind: RegistrationTrait, name: &str, binary: bool) -> TraitDef {
    let mut definition = contracts::contract(kind, if binary { &["T0"] } else { &[] });
    definition.associated_types.push(contracts::associated(
        &key::identity(kind),
        "Output",
        vec![],
    ));
    let mut params = vec![contracts::receiver(kind)];
    let arguments = definition
        .generic_params
        .iter()
        .map(|parameter| parameter.as_type())
        .collect::<Vec<_>>();
    params.extend(arguments.clone());
    definition.methods.push(contracts::method(
        name,
        params,
        projection(kind, "Output", arguments),
    ));
    definition
}

pub(super) fn declarations() -> Vec<TraitDef> {
    let iterator_kind = RegistrationTrait::Iterator;
    let mut iterator = contracts::contract(iterator_kind, &[]);
    iterator.associated_types.push(contracts::associated(
        &key::identity(iterator_kind),
        "Item",
        vec![],
    ));
    iterator.methods.push(contracts::method(
        "next",
        vec![contracts::receiver(iterator_kind)],
        contracts::option(projection(iterator_kind, "Item", vec![])),
    ));

    let iterable_kind = RegistrationTrait::Iterable;
    let mut iterable = contracts::contract(iterable_kind, &[]);
    iterable.associated_types.push(contracts::associated(
        &key::identity(iterable_kind),
        "Item",
        vec![],
    ));
    let mut iter_bound = key::applied(iterator_kind, vec![]);
    iter_bound.associated_types.insert(
        associated_type_id(&key::identity(iterator_kind), "Item"),
        projection(iterable_kind, "Item", vec![]),
    );
    iterable.associated_types.push(contracts::associated(
        &key::identity(iterable_kind),
        "Iter",
        vec![Constraint::Trait(iter_bound)],
    ));
    iterable.methods.push(contracts::method(
        "iter",
        vec![contracts::receiver(iterable_kind)],
        projection(iterable_kind, "Iter", vec![]),
    ));

    let mut partial_eq = contracts::contract(RegistrationTrait::PartialEq, &[]);
    partial_eq.methods.push(contracts::method(
        "eq",
        vec![contracts::receiver(RegistrationTrait::PartialEq); 2],
        Ty::Builtin(BuiltinType::Bool),
    ));
    let mut eq = contracts::contract(RegistrationTrait::Eq, &[]);
    eq.supertraits
        .push(key::applied(RegistrationTrait::PartialEq, vec![]));
    let mut hash = contracts::contract(RegistrationTrait::Hash, &[]);
    hash.methods.push(contracts::method(
        "hash",
        vec![contracts::receiver(RegistrationTrait::Hash)],
        Ty::Builtin(BuiltinType::I64),
    ));
    let ordering = contracts::enum_type(StandardEnum::Ordering, vec![]);
    let mut partial_ord = contracts::contract(RegistrationTrait::PartialOrd, &[]);
    partial_ord
        .supertraits
        .push(key::applied(RegistrationTrait::PartialEq, vec![]));
    partial_ord.methods.push(contracts::method(
        "partial_cmp",
        vec![contracts::receiver(RegistrationTrait::PartialOrd); 2],
        contracts::option(ordering.clone()),
    ));
    let mut ord = contracts::contract(RegistrationTrait::Ord, &[]);
    ord.supertraits.extend([
        key::applied(RegistrationTrait::Eq, vec![]),
        key::applied(RegistrationTrait::PartialOrd, vec![]),
    ]);
    ord.methods.push(contracts::method(
        "cmp",
        vec![contracts::receiver(RegistrationTrait::Ord); 2],
        ordering,
    ));

    let mut debug = contracts::contract(RegistrationTrait::Debug, &[]);
    debug.methods.push(contracts::method(
        "debug",
        vec![contracts::receiver(RegistrationTrait::Debug)],
        Ty::Builtin(BuiltinType::String),
    ));
    let mut display = contracts::contract(RegistrationTrait::Display, &[]);
    display.methods.push(contracts::method(
        "display",
        vec![contracts::receiver(RegistrationTrait::Display)],
        Ty::Builtin(BuiltinType::String),
    ));
    let mut from = contracts::contract(RegistrationTrait::From, &["T0"]);
    let mut from_method = contracts::method(
        "from",
        vec![from.generic_params[0].as_type()],
        contracts::receiver(RegistrationTrait::From),
    );
    from_method.params[0].name = "value".into();
    from.methods.push(from_method);

    vec![
        iterator,
        iterable,
        partial_eq,
        eq,
        hash,
        partial_ord,
        ord,
        operator(RegistrationTrait::Add, "add", true),
        operator(RegistrationTrait::Sub, "sub", true),
        operator(RegistrationTrait::Mul, "mul", true),
        operator(RegistrationTrait::Div, "div", true),
        operator(RegistrationTrait::Rem, "rem", true),
        operator(RegistrationTrait::BitAnd, "bitand", true),
        operator(RegistrationTrait::BitOr, "bitor", true),
        operator(RegistrationTrait::BitXor, "bitxor", true),
        operator(RegistrationTrait::Shl, "shl", true),
        operator(RegistrationTrait::Shr, "shr", true),
        operator(RegistrationTrait::Neg, "neg", false),
        operator(RegistrationTrait::Not, "not", false),
        operator(RegistrationTrait::Index, "index", true),
        operator(RegistrationTrait::Fn, "call", true),
        debug,
        display,
        from,
    ]
}
