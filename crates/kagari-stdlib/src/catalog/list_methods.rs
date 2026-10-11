//! List algorithms are ordinary native defaults with concrete storage overrides.
use crate::catalog::{
    contracts, identities,
    key::{self, RegistrationTrait},
};
use kagari_common::identity::{DefinitionKind, map::DefinitionContext};
use kagari_types::{
    callable::{CallableImplementation, NativeDefaultApplication},
    declaration::{
        FnDecl, Param,
        module::{DeclarationError, ModuleDecl},
        requirement::NativeCallableRequirement,
    },
    ty::{Constraint, GenericBound, GenericParam, Ty, substitution::TypeSubstitution},
};

pub(super) fn declare(
    ids: &DefinitionContext,
    module: &mut ModuleDecl,
) -> Result<(), DeclarationError> {
    for (protocol, names) in [
        (
            RegistrationTrait::List,
            &[
                "sorted",
                "sorted_by",
                "sorted_by_key",
                "reversed",
                "distinct",
            ][..],
        ),
        (
            RegistrationTrait::MutableList,
            &[
                "sort",
                "sort_by",
                "sort_by_key",
                "reverse",
                "retain",
                "dedup",
            ][..],
        ),
    ] {
        let owner = key::identity(ids, protocol)?;
        let item = GenericParam { owner, position: 0 }.as_type();
        for name in names {
            let mut method = contracts::method(
                name,
                vec![Ty::SelfType(owner)],
                if protocol == RegistrationTrait::List {
                    Ty::Trait(key::applied(
                        ids,
                        RegistrationTrait::List,
                        vec![item.clone()],
                    )?)
                } else {
                    contracts::unit()
                },
            );
            let key = if name.ends_with("_by_key") {
                let parameter = GenericParam {
                    owner: identities::method(ids, &owner, name)?,
                    position: 0,
                };
                let ty = parameter.as_type();
                method.generic_params.push(parameter);
                ty
            } else {
                item.clone()
            };
            if let Some(kind) = comparison(name) {
                method.bounds.push(GenericBound {
                    ty: key.clone(),
                    constraints: vec![Constraint::Trait(key::applied(ids, kind, vec![])?)],
                });
            }
            let callback = if name.ends_with("_by_key") {
                Some((vec![item.clone()], key))
            } else if name.ends_with("_by") {
                Some((
                    vec![item.clone(), item.clone()],
                    contracts::enum_type(ids, "Ordering", vec![])?,
                ))
            } else if *name == "retain" {
                Some((vec![item.clone()], contracts::boolean()))
            } else {
                None
            };
            if let Some((params, result)) = callback {
                method.params.push(Param {
                    name: "callback".into(),
                    ty: Ty::Function {
                        params,
                        result: Box::new(result),
                    },
                    mutable: false,
                });
            }
            module.documentation.insert(
                identities::method(ids, &owner, name)?,
                documentation(name).into(),
            );
            default(ids, module, protocol, &item, &mut method)?;
            module
                .traits
                .iter_mut()
                .find(|t| t.name == protocol.name())
                .expect("list contract")
                .methods
                .push(method);
        }
    }

    Ok(())
}

fn documentation(name: &str) -> &'static str {
    match name {
        "sorted" => "Return a stably sorted List using T: Ord; preserve the original slots.",
        "sorted_by" => {
            "Return a stably sorted List using the comparator; preserve the original slots."
        }
        "sorted_by_key" => {
            "Return a stably sorted List. Evaluate keys during comparisons without caching them."
        }
        "reversed" => "Return a reversed List without changing the original slots.",
        "distinct" => {
            "Return first occurrences in input order using T: Eq; no Hash bound is required."
        }
        "sort" => "Stably sort in place using T: Ord. Failure may leave a partial reordering.",
        "sort_by" => {
            "Stably sort in place using the comparator. Failure stops later callbacks; no rollback is promised."
        }
        "sort_by_key" => {
            "Stably sort in place with keys evaluated during comparisons. Failure does not promise rollback."
        }
        "reverse" => {
            "Reverse the list in place. Custom receiver failures may leave completed writes."
        }
        "retain" => {
            "Keep matching elements in input order. Failure preserves completed removals and callback effects."
        }
        "dedup" => {
            "Remove consecutive equal elements using T: Eq, preserving the first in each run. Failure preserves completed removals."
        }
        _ => unreachable!("declared list method"),
    }
}

fn comparison(name: &str) -> Option<RegistrationTrait> {
    match name {
        "sorted" | "sort" | "sorted_by_key" | "sort_by_key" => Some(RegistrationTrait::Ord),
        "distinct" | "dedup" => Some(RegistrationTrait::Eq),
        _ => None,
    }
}

fn requirement(
    ids: &DefinitionContext,
    receiver: Ty,
    protocol: RegistrationTrait,
    arguments: Vec<Ty>,
    member: &str,
) -> Result<NativeCallableRequirement, DeclarationError> {
    let interface = key::applied(ids, protocol, arguments)?;
    Ok(NativeCallableRequirement {
        receiver,
        member: identities::method(ids, &interface.declaration, member)?,
        interface,
        arguments: vec![],
    })
}

fn calls(
    ids: &DefinitionContext,
    method: &FnDecl,
    item: Ty,
    generic_receiver: bool,
) -> Result<Vec<NativeCallableRequirement>, DeclarationError> {
    let mut calls = vec![];
    if let Some(protocol) = comparison(&method.name) {
        let key = if method.name.ends_with("_by_key") {
            let Ty::Function { result, .. } = &method.params[1].ty else {
                unreachable!("key callback")
            };
            *result.clone()
        } else {
            item.clone()
        };
        let (protocol, member) = if protocol == RegistrationTrait::Eq {
            (RegistrationTrait::PartialEq, "eq")
        } else {
            (protocol, "cmp")
        };
        calls.push(requirement(ids, key, protocol, vec![], member)?);
    }
    if generic_receiver {
        let receiver = method.params[0].ty.clone();
        let iterable = contracts::applied_item(ids, RegistrationTrait::Iterable, item.clone())?;
        let cursor = Ty::Projection {
            receiver: Box::new(receiver.clone()),
            member: identities::associated(ids, &iterable.declaration, "Iter")?,
            interface: Box::new(iterable.clone()),
            arguments: vec![],
        };
        calls.push(NativeCallableRequirement {
            receiver: receiver.clone(),
            member: identities::method(ids, &iterable.declaration, "iter")?,
            interface: iterable,
            arguments: vec![],
        });
        let iterator = contracts::applied_item(ids, RegistrationTrait::Iterator, item.clone())?;
        calls.push(NativeCallableRequirement {
            receiver: cursor,
            member: identities::method(ids, &iterator.declaration, "next")?,
            interface: iterator,
            arguments: vec![],
        });
        if matches!(
            method.name.as_str(),
            "sort" | "sort_by" | "sort_by_key" | "reverse"
        ) {
            calls.push(requirement(
                ids,
                receiver,
                RegistrationTrait::MutableList,
                vec![item],
                "set",
            )?);
        } else if matches!(method.name.as_str(), "retain" | "dedup") {
            calls.push(requirement(
                ids,
                receiver,
                RegistrationTrait::MutableList,
                vec![item],
                "remove",
            )?);
        }
    }
    Ok(calls)
}

fn default(
    ids: &DefinitionContext,
    module: &mut ModuleDecl,
    protocol: RegistrationTrait,
    item: &Ty,
    method: &mut FnDecl,
) -> Result<(), DeclarationError> {
    let owner = key::identity(ids, protocol)?;
    let name = format!("__default_{}_{}", protocol.name(), method.name);
    let id = identities::definition(ids, DefinitionKind::Function, &name)?;
    let mut arguments: Vec<_> = [Ty::SelfType(owner), item.clone()]
        .into_iter()
        .chain(method.generic_params.iter().map(GenericParam::as_type))
        .collect();
    // Pass the selected associated iterator as an ordinary hidden template
    // parameter. Shared bodies need no runtime projection or trait search.
    let iterable = contracts::applied_item(ids, RegistrationTrait::Iterable, item.clone())?;
    arguments.push(Ty::Projection {
        receiver: Box::new(arguments[0].clone()),
        member: identities::associated(ids, &iterable.declaration, "Iter")?,
        interface: Box::new(iterable),
        arguments: vec![],
    });
    let parameters: Vec<_> = (0..arguments.len())
        .map(|position| GenericParam {
            owner: id,
            position,
        })
        .collect();
    let types: Vec<_> = parameters.iter().map(GenericParam::as_type).collect();
    let mut substitution = TypeSubstitution::default();
    substitution.bind_receiver(&owner, &types[0]);
    substitution.bind(&owner, 0, &types[1]);
    for (parameter, ty) in method.generic_params.iter().zip(&types[2..]) {
        substitution.bind(&parameter.owner, parameter.position, ty);
    }
    let cancel = Default::default();
    let requirements = calls(ids, method, item.clone(), true)?
        .iter()
        .map(|call| {
            let mut call = call
                .apply(&substitution, &cancel)
                .expect("default operation");
            match RegistrationTrait::from_id(&call.interface.declaration, &ids.snapshot()) {
                Some(RegistrationTrait::Iterable) => {
                    call.interface.associated_types.insert(
                        identities::associated(ids, &call.interface.declaration, "Iter")?,
                        types.last().unwrap().clone(),
                    );
                }
                Some(RegistrationTrait::Iterator) => call.receiver = types.last().unwrap().clone(),
                _ => {}
            }
            Ok(call)
        })
        .collect::<Result<_, DeclarationError>>()?;
    let mut template = method.clone();
    template.name = name;
    template.generic_params = parameters;
    template.implementation = CallableImplementation::Native(id);
    template.bounds.push(GenericBound {
        ty: arguments[0].clone(),
        constraints: vec![Constraint::Trait(key::applied(
            ids,
            protocol,
            vec![item.clone()],
        )?)],
    });
    let mut bounds = substitution
        .apply_bounds(&template.bounds, &ids.snapshot(), &cancel)
        .expect("default bounds");
    let mut iterable = contracts::applied_item(ids, RegistrationTrait::Iterable, types[1].clone())?;
    iterable.associated_types.insert(
        identities::associated(ids, &iterable.declaration, "Iter")?,
        types.last().unwrap().clone(),
    );
    bounds.push(GenericBound {
        ty: types[0].clone(),
        constraints: vec![Constraint::Trait(iterable)],
    });
    bounds.push(GenericBound {
        ty: types.last().unwrap().clone(),
        constraints: vec![Constraint::Trait(contracts::applied_item(
            ids,
            RegistrationTrait::Iterator,
            types[1].clone(),
        )?)],
    });
    template.bounds = GenericBound::normalize(bounds, &ids.snapshot(), &Default::default())
        .map_err(|error| DeclarationError(format!("foundation bounds: {error:?}")))?;
    for parameter in &mut template.params {
        if parameter.name == "self" {
            parameter.name = "receiver".into();
        }
        parameter.ty = substitution
            .apply(&parameter.ty, &cancel)
            .expect("default parameter");
    }
    template.return_type = substitution
        .apply(&template.return_type, &cancel)
        .expect("default result");
    if protocol == RegistrationTrait::List {
        module
            .concrete_results
            .insert(id, contracts::vec_type(ids, types[1].clone())?);
    }
    module.private_functions.insert(id);
    module.callable_requirements.insert(id, requirements);
    module.functions.push(template);
    method.implementation = CallableImplementation::NativeDefault(NativeDefaultApplication {
        declaration: id,
        arguments,
    });

    Ok(())
}

pub(super) fn configure_overrides(
    ids: &DefinitionContext,
    module: &mut ModuleDecl,
) -> Result<(), DeclarationError> {
    for index in 0..module.implementations.len() {
        let implementation = &module.implementations[index];
        let Some(interface) = &implementation.trait_type else {
            continue;
        };
        if !matches!(
            RegistrationTrait::from_id(&interface.declaration, &ids.snapshot()),
            Some(RegistrationTrait::List | RegistrationTrait::MutableList)
        ) {
            continue;
        }
        let item = interface.arguments[0].clone();
        let owner = identities::implementation(ids, index)?;
        for method in &implementation.methods {
            if !matches!(
                method.name.as_str(),
                "sorted"
                    | "sorted_by"
                    | "sorted_by_key"
                    | "reversed"
                    | "distinct"
                    | "sort"
                    | "sort_by"
                    | "sort_by_key"
                    | "reverse"
                    | "retain"
                    | "dedup"
            ) {
                continue;
            }
            let id = identities::method(ids, &owner, &method.name)?;
            module
                .callable_requirements
                .insert(id, calls(ids, method, item.clone(), false)?);
            if RegistrationTrait::from_id(&interface.declaration, &ids.snapshot())
                == Some(RegistrationTrait::List)
            {
                module
                    .concrete_results
                    .insert(id, implementation.for_type.clone());
            }
        }
    }

    Ok(())
}
