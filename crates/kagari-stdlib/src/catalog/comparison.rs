//! Collection content protocols and explicit comparison functions are library policy.
use crate::catalog::{
    contracts::{applied_item, boolean, method, vec_type},
    defaults::hash_bounds,
    identities,
    key::{self, RegistrationTrait},
};
use kagari_common::identity::{DefinitionKind, map::DefinitionContext, table::DefinitionId};
use kagari_types::{
    callable::CallableImplementation,
    collection::CollectionAccess,
    declaration::{
        module::{DeclarationError, ImplDecl, ModuleDecl},
        requirement::NativeCallableRequirement,
    },
    ty::{GenericBound, GenericParam, Ty},
};

pub(super) fn declare(
    ids: &DefinitionContext,
    module: &mut ModuleDecl,
) -> Result<(), DeclarationError> {
    for (family, collection, arity) in [
        ("list", RegistrationTrait::List, 1),
        ("set", RegistrationTrait::Set, 1),
        ("map", RegistrationTrait::Map, 2),
    ] {
        helper(ids, module, family, collection, arity)?;
        for protocol in [
            RegistrationTrait::PartialEq,
            RegistrationTrait::Eq,
            RegistrationTrait::Hash,
        ] {
            concrete(
                ids,
                module,
                family,
                collection,
                arity,
                protocol,
                CollectionAccess::Mutable,
            )?;
            if family != "list" {
                concrete(
                    ids,
                    module,
                    family,
                    collection,
                    arity,
                    protocol,
                    CollectionAccess::ReadOnly,
                )?;
            }
        }
    }
    Ok(())
}

fn parameters(owner: DefinitionId, count: usize) -> Vec<GenericParam> {
    (0..count)
        .map(|position| GenericParam { owner, position })
        .collect()
}

fn bound(
    ids: &DefinitionContext,
    ty: Ty,
    protocol: RegistrationTrait,
) -> Result<GenericBound, DeclarationError> {
    Ok(GenericBound {
        ty,
        constraints: vec![key::applied(ids, protocol, vec![])?],
    })
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

fn helper(
    ids: &DefinitionContext,
    module: &mut ModuleDecl,
    family: &str,
    collection: RegistrationTrait,
    arity: usize,
) -> Result<(), DeclarationError> {
    let name = format!("{family}_content_equal");
    let owner = identities::definition(ids, DefinitionKind::Function, &name)?;
    let params = parameters(owner, arity);
    let items: Vec<_> = params.iter().map(GenericParam::as_type).collect();
    let receiver = Ty::Trait(key::applied(ids, collection, items.clone())?);
    let mut function = method(&name, vec![receiver.clone(), receiver], boolean());
    function.params[0].name = "left".into();
    function.params[1].name = "right".into();
    function.generic_params = params;
    function.implementation = CallableImplementation::Native(owner);
    function.bounds.push(bound(
        ids,
        items[0].clone(),
        if family == "list" {
            RegistrationTrait::PartialEq
        } else {
            RegistrationTrait::Eq
        },
    )?);
    if family == "map" {
        function
            .bounds
            .push(bound(ids, items[1].clone(), RegistrationTrait::PartialEq)?);
    }
    module.callable_requirements.insert(
        owner,
        items
            .iter()
            .map(|item| {
                requirement(
                    ids,
                    item.clone(),
                    RegistrationTrait::PartialEq,
                    vec![],
                    "eq",
                )
            })
            .collect::<Result<_, _>>()?,
    );
    module.documentation.insert(owner, format!("Compare {family} contents across compatible implementations using element protocols. Receiver object equality and lookup policies do not change this comparison. Traversal preserves completed callback effects and iterator invalidation."));
    module.functions.push(function);
    Ok(())
}

fn concrete(
    ids: &DefinitionContext,
    module: &mut ModuleDecl,
    family: &str,
    collection: RegistrationTrait,
    arity: usize,
    protocol: RegistrationTrait,
    access: CollectionAccess,
) -> Result<(), DeclarationError> {
    let index = module.implementations.len();
    let owner = identities::implementation(ids, index)?;
    let params = parameters(owner, arity);
    let items: Vec<_> = params.iter().map(GenericParam::as_type).collect();
    let receiver = match family {
        "list" => vec_type(ids, items[0].clone())?,
        "set" => Ty::Set(Box::new(items[0].clone()), access),
        "map" => Ty::Map {
            key: Box::new(items[0].clone()),
            value: Box::new(items[1].clone()),
            access,
        },
        _ => unreachable!("collection family"),
    };
    let mut bounds = if family == "list" {
        vec![]
    } else {
        hash_bounds(ids, items[0].clone())?
    };
    if family != "set" {
        let item = items.last().unwrap().clone();
        if protocol == RegistrationTrait::Hash {
            bounds.extend(hash_bounds(ids, item)?);
        } else {
            bounds.push(bound(ids, item, protocol)?);
        }
    }
    let contract = module
        .traits
        .iter()
        .find(|item| item.name == protocol.name())
        .expect("declared protocol")
        .clone();
    let member = match protocol {
        RegistrationTrait::PartialEq => Some("eq"),
        RegistrationTrait::Hash => Some("hash"),
        _ => None,
    };
    let binding = member
        .map(|member| {
            identities::definition(
                ids,
                DefinitionKind::Function,
                &format!("$foundation_{family}_{member}"),
            )
        })
        .transpose()?;
    let bindings = member.zip(binding).into_iter().collect::<Vec<_>>();
    module.implement_trait(
        &contract,
        ImplDecl {
            generic_params: params,
            bounds,
            trait_type: Some(key::applied(ids, protocol, vec![])?),
            for_type: receiver.clone(),
            methods: vec![],
        },
        &bindings,
        ids,
    )?;
    if let Some(member) = member {
        let mut calls = items
            .iter()
            .map(|item| requirement(ids, item.clone(), protocol, vec![], member))
            .collect::<Result<Vec<_>, _>>()?;
        calls.push(requirement(
            ids,
            receiver.clone(),
            collection,
            items.clone(),
            "len",
        )?);
        let item = if family == "map" {
            Ty::Tuple(items)
        } else {
            items[0].clone()
        };
        let mut iterable = applied_item(ids, RegistrationTrait::Iterable, item.clone())?;
        let cursor = Ty::Iter(Box::new(item.clone()));
        iterable.associated_types.insert(
            identities::associated(ids, &iterable.declaration, "Iter")?,
            cursor.clone(),
        );
        calls.push(NativeCallableRequirement {
            receiver,
            member: identities::method(ids, &iterable.declaration, "iter")?,
            interface: iterable,
            arguments: vec![],
        });
        let iterator = applied_item(ids, RegistrationTrait::Iterator, item)?;
        calls.push(NativeCallableRequirement {
            receiver: cursor,
            member: identities::method(ids, &iterator.declaration, "next")?,
            interface: iterator,
            arguments: vec![],
        });
        module
            .callable_requirements
            .insert(identities::method(ids, &owner, member)?, calls);
    }
    Ok(())
}
