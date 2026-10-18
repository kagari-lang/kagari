//! Base implementations use ordinary native declarations and selected callbacks.
use crate::catalog::{
    contracts, defaults, identities,
    key::{self, RegistrationTrait},
};
use kagari_common::identity::{DefinitionKind, map::DefinitionContext};
use kagari_types::{
    collection::CollectionAccess,
    declaration::{
        module::{DeclarationError, ImplDecl, ModuleDecl},
        requirement::NativeCallableRequirement,
    },
    ty::{GenericParam, NominalTy, Ty},
};

pub(super) fn declare(
    ids: &DefinitionContext,
    module: &mut ModuleDecl,
) -> Result<(), DeclarationError> {
    default_implementations(ids, module)?;
    let parameter = GenericParam {
        owner: identities::implementation(ids, module.implementations.len())?,
        position: 0,
    };
    let item = parameter.as_type();
    implement(
        ids,
        module,
        key::applied(ids, RegistrationTrait::FromIterator, vec![item.clone()])?,
        contracts::vec_type(ids, item.clone())?,
        vec![parameter],
        "$foundation_list_from_iter",
    )?;
    iteration_calls(ids, module, item)?;
    hash_constructors(ids, module)?;

    Ok(())
}

fn default_implementations(
    ids: &DefinitionContext,
    module: &mut ModuleDecl,
) -> Result<(), DeclarationError> {
    let parameter = GenericParam {
        owner: identities::implementation(ids, module.implementations.len())?,
        position: 0,
    };
    implement_default(
        ids,
        module,
        contracts::vec_type(ids, parameter.as_type())?,
        vec![parameter],
        "$foundation_default_Vec",
        "Return a fresh empty Vec<T>. Every call creates independent mutable storage; T need not implement Default.",
    )
}

fn implement_default(
    ids: &DefinitionContext,
    module: &mut ModuleDecl,
    receiver: Ty,
    parameters: Vec<GenericParam>,
    binding: &str,
    description: &str,
) -> Result<(), DeclarationError> {
    let owner = identities::implementation(ids, module.implementations.len())?;
    implement(
        ids,
        module,
        key::applied(ids, RegistrationTrait::Default, vec![])?,
        receiver,
        parameters,
        binding,
    )?;
    module.documentation.insert(
        identities::method(ids, &owner, "default")?,
        format!("{description}\n\nSee `core::default::Default` for the construction contract."),
    );
    Ok(())
}

fn implement(
    ids: &DefinitionContext,
    module: &mut ModuleDecl,
    interface: NominalTy,
    receiver: Ty,
    parameters: Vec<GenericParam>,
    binding: &str,
) -> Result<(), DeclarationError> {
    let contract = module
        .traits
        .iter()
        .find(|contract| {
            ids.snapshot()
                .resolve(interface.declaration)
                .is_ok_and(|view| {
                    view.last()
                        .is_some_and(|member| member.name == contract.name)
                })
        })
        .expect("foundation contract")
        .clone();
    let method = contract.methods[0].name.as_str();
    let binding = identities::definition(ids, DefinitionKind::Function, binding)?;
    module.implement_trait(
        &contract,
        ImplDecl {
            generic_params: parameters,
            bounds: vec![],
            trait_type: Some(interface),
            for_type: receiver,
            methods: vec![],
        },
        &[(method, binding)],
        ids,
    )?;

    Ok(())
}

fn iteration_calls(
    ids: &DefinitionContext,
    module: &mut ModuleDecl,
    item: Ty,
) -> Result<(), DeclarationError> {
    let owner = identities::implementation(ids, module.implementations.len() - 1)?;
    let method = &module.implementations.last().unwrap().methods[0];
    let source = method.params[0].ty.clone();
    let iterable = contracts::applied_item(ids, RegistrationTrait::Iterable, item.clone())?;
    let cursor = Ty::Projection {
        receiver: Box::new(source.clone()),
        member: identities::associated(ids, &iterable.declaration, "Iter")?,
        interface: Box::new(iterable.clone()),
        arguments: vec![],
    };
    let iterator = contracts::applied_item(ids, RegistrationTrait::Iterator, item)?;
    module.callable_requirements.insert(
        identities::method(ids, &owner, &method.name)?,
        vec![
            NativeCallableRequirement {
                receiver: source,
                member: identities::method(ids, &iterable.declaration, "iter")?,
                interface: iterable,
                arguments: vec![],
            },
            NativeCallableRequirement {
                receiver: cursor,
                member: identities::method(ids, &iterator.declaration, "next")?,
                interface: iterator,
                arguments: vec![],
            },
        ],
    );

    Ok(())
}

fn hash_constructors(
    ids: &DefinitionContext,
    module: &mut ModuleDecl,
) -> Result<(), DeclarationError> {
    for map in [false, true] {
        let owner = identities::implementation(ids, module.implementations.len())?;
        let parameters: Vec<_> = (0..if map { 2 } else { 1 })
            .map(|position| GenericParam { owner, position })
            .collect();
        let key_type = parameters[0].as_type();
        let value = if map {
            Some(parameters[1].as_type())
        } else {
            None
        };
        let item = value.as_ref().map_or_else(
            || key_type.clone(),
            |value| Ty::Tuple(vec![key_type.clone(), value.clone()]),
        );
        let receiver = value.as_ref().map_or_else(
            || Ty::Set(Box::new(key_type.clone()), CollectionAccess::Mutable),
            |value| Ty::Map {
                key: Box::new(key_type.clone()),
                value: Box::new(value.clone()),
                access: CollectionAccess::Mutable,
            },
        );
        implement(
            ids,
            module,
            key::applied(ids, RegistrationTrait::FromIterator, vec![item.clone()])?,
            receiver.clone(),
            parameters,
            if map {
                "$foundation_map_from_iter"
            } else {
                "$foundation_set_from_iter"
            },
        )?;
        module.implementations.last_mut().unwrap().bounds =
            defaults::hash_bounds(ids, key_type.clone())?;
        iteration_calls(ids, module, item)?;
        let method = identities::method(ids, &owner, "from_iter")?;
        let iteration = module.callable_requirements.remove(&method).unwrap();
        let mut requirements: Vec<_> = [
            (RegistrationTrait::Hash, "hash"),
            (RegistrationTrait::PartialEq, "eq"),
        ]
        .into_iter()
        .map(|(protocol, name)| {
            let interface = key::applied(ids, protocol, vec![])?;
            Ok(NativeCallableRequirement {
                receiver: key_type.clone(),
                member: identities::method(ids, &interface.declaration, name)?,
                interface,
                arguments: vec![],
            })
        })
        .collect::<Result<_, DeclarationError>>()?;
        let interface = key::applied(
            ids,
            if map {
                RegistrationTrait::MutableMap
            } else {
                RegistrationTrait::MutableSet
            },
            value.map_or_else(
                || vec![key_type.clone()],
                |value| vec![key_type.clone(), value],
            ),
        )?;
        requirements.push(NativeCallableRequirement {
            receiver,
            member: identities::method(ids, &interface.declaration, "insert")?,
            interface,
            arguments: vec![],
        });
        requirements.extend(iteration);
        module.callable_requirements.insert(method, requirements);
    }

    Ok(())
}
