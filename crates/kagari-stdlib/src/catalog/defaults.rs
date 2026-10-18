//! Canonical concrete declarations and their ordinary native implementation slots.
use crate::catalog::{
    contracts::{applied_item, method, unit, vec_type},
    identities,
    key::{self, RegistrationTrait},
};
use kagari_common::identity::{DefinitionKind, map::DefinitionContext, table::DefinitionId};
use kagari_types::{
    callable::CallableImplementation,
    collection::CollectionAccess,
    declaration::{
        FnDecl, TypeDef, TypeDefKind,
        module::{DeclarationError, ImplDecl, ModuleDecl},
        native::{NativeStorageLayout, NativeTypeConstructor},
        requirement::NativeCallableRequirement,
    },
    reference::ReferenceAccess,
    scalar::BuiltinType,
    ty::{GenericBound, GenericParam, Ty, substitution::TypeSubstitution},
};

fn define_type(
    ids: &DefinitionContext,
    module: &mut ModuleDecl,
    name: &str,
    layout: NativeTypeConstructor,
    parameters: &[&str],
    hash_key: bool,
) -> Result<(), DeclarationError> {
    let owner = identities::definition(ids, layout.declaration_kind(), name)?;
    let generic_params: Vec<_> = parameters
        .iter()
        .enumerate()
        .map(|(position, _)| GenericParam { owner, position })
        .collect();
    let bounds = if hash_key {
        hash_bounds(ids, generic_params[0].as_type())?
    } else {
        vec![]
    };
    module.types.push(TypeDef {
        name: name.into(),
        kind: TypeDefKind::Native(layout),
        generic_params,
        bounds,
        fields: vec![],
        variants: vec![],
    });
    module
        .documentation
        .insert(owner, format!("Language-owned {name} type."));

    Ok(())
}

pub(super) fn hash_bounds(
    ids: &DefinitionContext,
    key: Ty,
) -> Result<Vec<GenericBound>, DeclarationError> {
    Ok(vec![GenericBound {
        ty: key,
        constraints: vec![
            key::applied(ids, RegistrationTrait::Eq, vec![])?,
            key::applied(ids, RegistrationTrait::Hash, vec![])?,
            // The selected equality member belongs to PartialEq. Carry its
            // inherited obligation explicitly in the portable callback template.
            key::applied(ids, RegistrationTrait::PartialEq, vec![])?,
        ],
    }])
}

fn parameters(owner: &DefinitionId, names: &[&str]) -> Vec<GenericParam> {
    names
        .iter()
        .enumerate()
        .map(|(position, _)| GenericParam {
            owner: *owner,
            position,
        })
        .collect()
}

fn binding(
    ids: &DefinitionContext,
    family: &str,
    name: &str,
) -> Result<DefinitionId, DeclarationError> {
    identities::definition(
        ids,
        DefinitionKind::Function,
        &format!("$foundation_{family}_{name}"),
    )
}

fn implement(
    ids: &DefinitionContext,
    module: &mut ModuleDecl,
    kind: RegistrationTrait,
    implementation: ImplDecl,
    family: &str,
) -> Result<(), DeclarationError> {
    let contract = module
        .traits
        .iter()
        .find(|contract| contract.name == kind.name())
        .expect("language contract")
        .clone();
    let bindings: Vec<_> = contract
        .methods
        .iter()
        .filter(|method| {
            kind != RegistrationTrait::Iterable
                || !matches!(
                    method.implementation,
                    CallableImplementation::NativeDefault(_)
                )
        })
        .map(|method| Ok((method.name.as_str(), binding(ids, family, &method.name)?)))
        .collect::<Result<_, DeclarationError>>()?;
    module.implement_trait(&contract, implementation, &bindings, ids)?;
    declare_key_calls(ids, module, family)?;

    Ok(())
}

fn declare_key_calls(
    ids: &DefinitionContext,
    module: &mut ModuleDecl,
    family: &str,
) -> Result<(), DeclarationError> {
    if !matches!(family, "map" | "set") {
        return Ok(());
    }
    let index = module.implementations.len() - 1;
    let implementation = &module.implementations[index];
    let key = implementation.generic_params[0].as_type();
    let requirements: Vec<_> = [
        (RegistrationTrait::Hash, "hash"),
        (RegistrationTrait::PartialEq, "eq"),
    ]
    .into_iter()
    .map(|(protocol, name)| {
        let interface = key::applied(ids, protocol, vec![])?;
        Ok(NativeCallableRequirement {
            receiver: key.clone(),
            member: identities::method(ids, &interface.declaration, name)?,
            interface,
            arguments: vec![],
        })
    })
    .collect::<Result<_, DeclarationError>>()?;
    let owner = identities::implementation(ids, index)?;
    let methods: Vec<_> = implementation
        .methods
        .iter()
        .filter(|method| {
            matches!(
                method.name.as_str(),
                "new"
                    | "from"
                    | "get"
                    | "contains"
                    | "contains_key"
                    | "insert"
                    | "insert_fluent"
                    | "remove"
            )
        })
        .map(|method| identities::method(ids, &owner, &method.name))
        .collect::<Result<_, _>>()?;
    for method in methods {
        module
            .callable_requirements
            .insert(method, requirements.clone());
    }

    Ok(())
}

pub(super) fn declare(
    ids: &DefinitionContext,
    module: &mut ModuleDecl,
) -> Result<(), DeclarationError> {
    for access in [ReferenceAccess::Shared, ReferenceAccess::Mutable] {
        define_type(
            ids,
            module,
            access.name(),
            NativeTypeConstructor::Reference(access),
            &["T"],
            false,
        )?;
    }
    let owner = identities::definition(ids, DefinitionKind::AssociatedType, "Vec")?;
    module.types.push(TypeDef {
        name: "Vec".into(),
        kind: TypeDefKind::NativeStorage(NativeStorageLayout::Sequence { element: 0 }),
        generic_params: parameters(&owner, &["T"]),
        bounds: vec![],
        fields: vec![],
        variants: vec![],
    });
    define_type(
        ids,
        module,
        "HashMap",
        NativeTypeConstructor::Map,
        &["K", "V"],
        true,
    )?;
    define_type(
        ids,
        module,
        "HashSet",
        NativeTypeConstructor::Set,
        &["T"],
        true,
    )?;
    define_type(
        ids,
        module,
        "CollectionCursor",
        NativeTypeConstructor::Iter,
        &["T"],
        false,
    )?;
    for (family, names, protocols) in [
        (
            "list",
            &["T"][..],
            &[
                RegistrationTrait::List,
                RegistrationTrait::MutableList,
                RegistrationTrait::Index,
                RegistrationTrait::Iterable,
            ][..],
        ),
        (
            "map",
            &["K", "V"][..],
            &[
                RegistrationTrait::Map,
                RegistrationTrait::MutableMap,
                RegistrationTrait::Iterable,
            ][..],
        ),
        (
            "set",
            &["T"][..],
            &[
                RegistrationTrait::Set,
                RegistrationTrait::MutableSet,
                RegistrationTrait::Iterable,
            ][..],
        ),
        ("cursor", &["T"][..], &[RegistrationTrait::Iterator][..]),
    ] {
        // Legacy host Map/Set descriptors have exact readable implementations.
        // Ordinary trait matching never weakens access to obtain these methods.
        let readable = protocols.iter().filter(|kind| {
            matches!(family, "map" | "set")
                && matches!(
                    kind,
                    RegistrationTrait::Map | RegistrationTrait::Set | RegistrationTrait::Iterable
                )
        });
        for (kind, access) in protocols
            .iter()
            .map(|kind| (kind, CollectionAccess::Mutable))
            .chain(readable.map(|kind| (kind, CollectionAccess::ReadOnly)))
        {
            let owner = identities::implementation(ids, module.implementations.len())?;
            let params = parameters(&owner, names);
            let items: Vec<_> = params.iter().map(GenericParam::as_type).collect();
            let item = if family == "map" {
                Ty::Tuple(items.clone())
            } else {
                items[0].clone()
            };
            let receiver = storage(ids, family, &items)?;
            let receiver = if access == CollectionAccess::ReadOnly {
                receiver.read_only_view().expect("host map/set descriptor")
            } else {
                receiver
            };
            let mut applied = if kind.iteration() {
                applied_item(ids, *kind, item.clone())?
            } else if *kind == RegistrationTrait::Index {
                key::applied(ids, *kind, vec![Ty::Builtin(BuiltinType::U64)])?
            } else {
                key::applied(ids, *kind, items.clone())?
            };
            if *kind == RegistrationTrait::Index {
                applied.associated_types.insert(
                    identities::associated(ids, &applied.declaration, "Output")?,
                    item.clone(),
                );
            }
            if *kind == RegistrationTrait::Iterable {
                applied.associated_types.insert(
                    identities::associated(ids, &applied.declaration, "Iter")?,
                    Ty::Iter(Box::new(item)),
                );
            }
            let bounds = if matches!(family, "map" | "set") {
                hash_bounds(ids, items[0].clone())?
            } else {
                vec![]
            };
            implement(
                ids,
                module,
                *kind,
                ImplDecl {
                    generic_params: params,
                    bounds,
                    trait_type: Some(applied),
                    for_type: receiver,
                    methods: vec![],
                },
                family,
            )?;
        }
        if family != "cursor" {
            inherent(ids, module, family, names)?;
        }
    }

    Ok(())
}

fn storage(ids: &DefinitionContext, family: &str, items: &[Ty]) -> Result<Ty, DeclarationError> {
    Ok(match family {
        "list" => vec_type(ids, items[0].clone())?,
        "map" => Ty::Map {
            key: Box::new(items[0].clone()),
            value: Box::new(items[1].clone()),
            access: CollectionAccess::Mutable,
        },
        "set" => Ty::Set(Box::new(items[0].clone()), CollectionAccess::Mutable),
        "cursor" => Ty::Iter(Box::new(items[0].clone())),
        _ => unreachable!("foundation family"),
    })
}

fn inherent(
    ids: &DefinitionContext,
    module: &mut ModuleDecl,
    family: &str,
    names: &[&str],
) -> Result<(), DeclarationError> {
    let owner = identities::implementation(ids, module.implementations.len())?;
    let parameters = parameters(&owner, names);
    let items: Vec<_> = parameters.iter().map(GenericParam::as_type).collect();
    let receiver = storage(ids, family, &items)?;
    let bounds = if matches!(family, "map" | "set") {
        hash_bounds(ids, items[0].clone())?
    } else {
        vec![]
    };
    let mut new = method("new", vec![], receiver.clone());
    new.implementation = CallableImplementation::Native(binding(ids, family, "new")?);
    let mut methods: Vec<FnDecl> = vec![new];
    if matches!(family, "list" | "set") {
        let mut from = method(
            "from",
            vec![Ty::Array(Box::new(items[0].clone()))],
            receiver.clone(),
        );
        from.params[0].name = "array".into();
        from.implementation = CallableImplementation::Native(binding(ids, family, "from")?);
        methods.push(from);
    }
    // Concrete mutators preserve fluent returns; capability methods return unit.
    for kind in [
        RegistrationTrait::MutableList,
        RegistrationTrait::MutableMap,
        RegistrationTrait::MutableSet,
    ] {
        if !matches!(
            (family, kind),
            ("list", RegistrationTrait::MutableList)
                | ("map", RegistrationTrait::MutableMap)
                | ("set", RegistrationTrait::MutableSet)
        ) {
            continue;
        }
        let contract = module
            .traits
            .iter()
            .find(|contract| contract.name == kind.name())
            .expect("mutable contract")
            .clone();
        let contract_owner = key::identity(ids, kind)?;
        let mut substitution = TypeSubstitution::for_owner(&contract_owner, &items);
        substitution.bind_receiver(&contract_owner, &receiver);
        for mut method in contract.methods {
            if method.return_type != unit()
                || matches!(
                    method.implementation,
                    CallableImplementation::NativeDefault(_)
                )
            {
                continue;
            }
            for param in &mut method.params {
                param.ty = substitution
                    .apply(&param.ty, &Default::default())
                    .expect("foundation parameter");
            }
            method.return_type = receiver.clone();
            method.implementation = CallableImplementation::Native(binding(
                ids,
                family,
                &format!("{}_fluent", method.name),
            )?);
            methods.push(method);
        }
    }
    for method in &mut methods {
        method.generic_params = parameters.clone();
        method.bounds = bounds.clone();
    }
    module.implementations.push(ImplDecl {
        generic_params: parameters,
        bounds,
        trait_type: None,
        for_type: receiver,
        methods,
    });
    declare_key_calls(ids, module, family)?;

    Ok(())
}
