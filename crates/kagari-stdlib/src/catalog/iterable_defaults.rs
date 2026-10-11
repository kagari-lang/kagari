//! Checked source/destination requirements for the shared Iterable templates.
use crate::catalog::{
    contracts, identities,
    key::{self, RegistrationTrait},
};
use kagari_common::identity::{DefinitionKind, map::DefinitionContext};
use kagari_types::{
    callable::{CallableImplementation, NativeDefaultApplication},
    declaration::{
        FnDecl,
        module::{DeclarationError, ModuleDecl},
        requirement::NativeCallableRequirement,
    },
    ty::{
        Constraint, GenericBound, GenericParam, Ty,
        substitution::{TypeSubstitution, normalize_projections},
    },
};

struct Source {
    receiver: Ty,
    item: Ty,
    cursor: Ty,
}

fn projection(
    ids: &DefinitionContext,
    receiver: Ty,
    name: &str,
    item: Option<Ty>,
) -> Result<Ty, DeclarationError> {
    let interface = match item {
        Some(item) => contracts::applied_item(ids, RegistrationTrait::Iterable, item)?,
        None => key::applied(ids, RegistrationTrait::Iterable, vec![])?,
    };
    Ok(Ty::Projection {
        receiver: Box::new(receiver),
        member: identities::associated(ids, &interface.declaration, name)?,
        interface: Box::new(interface),
        arguments: vec![],
    })
}

fn destination(name: &str) -> Option<(RegistrationTrait, &str)> {
    match name {
        "collect" => Some((RegistrationTrait::FromIterator, "from_iter")),
        "sum" => Some((RegistrationTrait::Sum, "sum")),
        "product" => Some((RegistrationTrait::Product, "product")),
        _ => None,
    }
}

fn calls(
    ids: &DefinitionContext,
    source: &Source,
) -> Result<Vec<NativeCallableRequirement>, DeclarationError> {
    let mut iterable =
        contracts::applied_item(ids, RegistrationTrait::Iterable, source.item.clone())?;
    iterable.associated_types.insert(
        identities::associated(ids, &iterable.declaration, "Iter")?,
        source.cursor.clone(),
    );
    let iterator = contracts::applied_item(ids, RegistrationTrait::Iterator, source.item.clone())?;
    Ok(vec![
        NativeCallableRequirement {
            receiver: source.receiver.clone(),
            member: identities::method(ids, &iterable.declaration, "iter")?,
            interface: iterable,
            arguments: vec![],
        },
        NativeCallableRequirement {
            receiver: source.cursor.clone(),
            member: identities::method(ids, &iterator.declaration, "next")?,
            interface: iterator,
            arguments: vec![],
        },
    ])
}

pub(super) fn apply(
    ids: &DefinitionContext,
    module: &mut ModuleDecl,
    item: &Ty,
    method: &mut FnDecl,
) -> Result<(), DeclarationError> {
    let owner = key::identity(ids, RegistrationTrait::Iterable)?;
    let id = identities::definition(
        ids,
        DefinitionKind::Function,
        &format!("__default_Iterable_{}", method.name),
    )?;
    let destination = destination(&method.name);
    let paired = matches!(method.name.as_str(), "zip" | "chain");
    let mut arguments: Vec<_> = [Ty::SelfType(owner), item.clone()]
        .into_iter()
        .chain(method.generic_params.iter().map(GenericParam::as_type))
        .collect();
    let right_item = if method.name == "zip" {
        let index = arguments.len();
        arguments.push(projection(ids, arguments[2].clone(), "Item", None)?);
        Some(index)
    } else {
        None
    };
    let left_cursor = if destination.is_none() {
        let index = arguments.len();
        arguments.push(projection(ids, arguments[0].clone(), "Iter", None)?);
        Some(index)
    } else {
        None
    };
    let right_cursor = if paired {
        let index = arguments.len();
        arguments.push(projection(
            ids,
            arguments[2].clone(),
            "Iter",
            (method.name == "chain").then(|| item.clone()),
        )?);
        Some(index)
    } else {
        None
    };
    let parameters: Vec<_> = (0..arguments.len())
        .map(|position| GenericParam {
            owner: id,
            position,
        })
        .collect();
    let types: Vec<_> = parameters.iter().map(GenericParam::as_type).collect();
    let mut sources = vec![];
    if let Some(index) = left_cursor {
        sources.push(Source {
            receiver: types[0].clone(),
            item: types[1].clone(),
            cursor: types[index].clone(),
        });
    }
    if let Some(index) = right_cursor {
        sources.push(Source {
            receiver: types[2].clone(),
            item: right_item.map_or_else(|| types[1].clone(), |index| types[index].clone()),
            cursor: types[index].clone(),
        });
    }
    let mut substitution = TypeSubstitution::default();
    substitution.bind_receiver(&owner, &types[0]);
    for (parameter, ty) in method.generic_params.iter().zip(&types[2..]) {
        substitution.bind(&parameter.owner, parameter.position, ty);
    }
    let cancel = Default::default();
    let item_member = identities::associated(ids, &owner, "Item")?;
    let iter_member = identities::associated(ids, &owner, "Iter")?;
    let normalize = |ty: &Ty| {
        let applied = substitution
            .apply(ty, &cancel)
            .expect("Iterable substitution");
        normalize_projections(
            &applied,
            &|interface, receiver, member, _| {
                if interface.declaration != owner {
                    return Ok(None);
                }
                if *receiver == types[0] && *member == item_member {
                    return Ok(Some(types[1].clone()));
                }
                Ok(sources
                    .iter()
                    .find(|source| source.receiver == *receiver)
                    .and_then(|source| {
                        if *member == item_member {
                            Some(source.item.clone())
                        } else if *member == iter_member {
                            Some(source.cursor.clone())
                        } else {
                            None
                        }
                    }))
            },
            &cancel,
        )
        .expect("Iterable outputs")
    };
    let mut requirements = vec![];
    for source in &sources {
        requirements.extend(calls(ids, source)?);
    }
    if let Some((protocol, target)) = destination {
        let interface = key::applied(ids, protocol, vec![types[1].clone()])?;
        requirements.push(NativeCallableRequirement {
            receiver: types[2].clone(),
            member: identities::method(ids, &interface.declaration, target)?,
            interface,
            arguments: vec![types[0].clone()],
        });
    }
    let mut bounds = method
        .bounds
        .iter()
        .map(|bound| GenericBound {
            ty: normalize(&bound.ty),
            constraints: bound
                .constraints
                .iter()
                .map(|constraint| match constraint {
                    Constraint::Standard(value) => Constraint::Standard(*value),
                    Constraint::Trait(value) => {
                        let Ty::Trait(value) = normalize(&Ty::Trait(value.clone())) else {
                            unreachable!("bound interface")
                        };
                        Constraint::Trait(value)
                    }
                })
                .collect(),
        })
        .collect::<Vec<_>>();
    if destination.is_some() {
        bounds.push(GenericBound {
            ty: types[0].clone(),
            constraints: vec![Constraint::Trait(contracts::applied_item(
                ids,
                RegistrationTrait::Iterable,
                types[1].clone(),
            )?)],
        });
    }
    for source in &sources {
        let mut iterable =
            contracts::applied_item(ids, RegistrationTrait::Iterable, source.item.clone())?;
        iterable.associated_types.insert(
            identities::associated(ids, &owner, "Iter")?,
            source.cursor.clone(),
        );
        bounds.extend([
            GenericBound {
                ty: source.receiver.clone(),
                constraints: vec![Constraint::Trait(iterable)],
            },
            GenericBound {
                ty: source.cursor.clone(),
                constraints: vec![Constraint::Trait(contracts::applied_item(
                    ids,
                    RegistrationTrait::Iterator,
                    source.item.clone(),
                )?)],
            },
        ]);
    }
    let mut template = method.clone();
    template.name = ids
        .snapshot()
        .resolve(id)
        .map_err(identities::invalid)?
        .last()
        .expect("template member")
        .name
        .to_owned();
    template.generic_params = parameters;
    template.implementation = CallableImplementation::Native(id);
    template.bounds = GenericBound::normalize(bounds, &ids.snapshot(), &Default::default())
        .map_err(|error| DeclarationError(format!("foundation bounds: {error:?}")))?;
    for parameter in &mut template.params {
        if parameter.name == "self" {
            parameter.name = "receiver".into();
        }
        parameter.ty = normalize(&parameter.ty);
    }
    template.return_type = normalize(&template.return_type);
    module.private_functions.insert(id);
    module.callable_requirements.insert(id, requirements);
    module.functions.push(template);
    method.implementation = CallableImplementation::NativeDefault(NativeDefaultApplication {
        declaration: id,
        arguments,
    });

    Ok(())
}
