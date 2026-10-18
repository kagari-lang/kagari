//! Base implementations use ordinary native declarations and selected callbacks.
use crate::catalog::{
    contracts, defaults, identities,
    key::{self, RegistrationTrait},
};
use kagari_common::identity::{DefinitionKind, map::DefinitionContext};
use kagari_types::{
    collection::CollectionAccess,
    conversion::checked_conversion_fallible,
    declaration::{
        module::{DeclarationError, ImplDecl, ModuleDecl},
        requirement::NativeCallableRequirement,
    },
    scalar::BuiltinType,
    ty::{GenericParam, NominalTy, Ty},
};

const SCALARS: [BuiltinType; 11] = [
    BuiltinType::Bool,
    BuiltinType::I8,
    BuiltinType::I16,
    BuiltinType::I32,
    BuiltinType::I64,
    BuiltinType::U8,
    BuiltinType::U16,
    BuiltinType::U32,
    BuiltinType::U64,
    BuiltinType::F32,
    BuiltinType::F64,
];

pub(super) fn declare(
    ids: &DefinitionContext,
    module: &mut ModuleDecl,
) -> Result<(), DeclarationError> {
    default_implementations(ids, module)?;
    checked_conversions(ids, module)?;
    for target in SCALARS {
        let mut interface = key::applied(ids, RegistrationTrait::FromStr, vec![])?;
        interface.associated_types.insert(
            identities::associated(ids, &interface.declaration, "Err")?,
            contracts::enum_type(ids, "ParseError", vec![])?,
        );
        implement(
            ids,
            module,
            interface,
            Ty::Builtin(target),
            vec![],
            "$foundation_from_str",
        )?;
        if target.number_type().is_some() {
            for (kind, binding) in [
                (RegistrationTrait::Sum, "$foundation_sum"),
                (RegistrationTrait::Product, "$foundation_product"),
            ] {
                let item = Ty::Builtin(target);
                implement(
                    ids,
                    module,
                    key::applied(ids, kind, vec![item.clone()])?,
                    item.clone(),
                    vec![],
                    binding,
                )?;
                iteration_calls(ids, module, item)?;
            }
        }
    }
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
    for (target, binding, description) in [
        (
            BuiltinType::Unit,
            "$foundation_default_Unit",
            "Return the Unit value ().",
        ),
        (
            BuiltinType::Bool,
            "$foundation_default_Bool",
            "Return false.",
        ),
        (
            BuiltinType::I8,
            "$foundation_default_I8",
            "Return zero of type i8.",
        ),
        (
            BuiltinType::I16,
            "$foundation_default_I16",
            "Return zero of type i16.",
        ),
        (
            BuiltinType::I32,
            "$foundation_default_I32",
            "Return zero of type i32.",
        ),
        (
            BuiltinType::I64,
            "$foundation_default_I64",
            "Return zero of type i64.",
        ),
        (
            BuiltinType::U8,
            "$foundation_default_U8",
            "Return zero of type u8.",
        ),
        (
            BuiltinType::U16,
            "$foundation_default_U16",
            "Return zero of type u16.",
        ),
        (
            BuiltinType::U32,
            "$foundation_default_U32",
            "Return zero of type u32.",
        ),
        (
            BuiltinType::U64,
            "$foundation_default_U64",
            "Return zero of type u64.",
        ),
        (
            BuiltinType::F32,
            "$foundation_default_F32",
            "Return positive zero of type f32.",
        ),
        (
            BuiltinType::F64,
            "$foundation_default_F64",
            "Return positive zero of type f64.",
        ),
        (
            BuiltinType::String,
            "$foundation_default_String",
            "Return an empty immutable String.",
        ),
    ] {
        implement_default(
            ids,
            module,
            Ty::Builtin(target),
            vec![],
            binding,
            description,
        )?;
    }
    for (name, binding, description) in [
        (
            "Vec",
            "$foundation_default_Vec",
            "Return a fresh empty Vec<T>. Every call creates independent mutable storage; T need not implement Default.",
        ),
        (
            "Option",
            "$foundation_default_Option",
            "Return Option<T>::None using the checked Option enum identity. T need not implement Default.",
        ),
    ] {
        let parameter = GenericParam {
            owner: identities::implementation(ids, module.implementations.len())?,
            position: 0,
        };
        let receiver = if name == "Vec" {
            contracts::vec_type(ids, parameter.as_type())?
        } else {
            contracts::option(ids, parameter.as_type())?
        };
        implement_default(ids, module, receiver, vec![parameter], binding, description)?;
    }
    Ok(())
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

fn checked_conversions(
    ids: &DefinitionContext,
    module: &mut ModuleDecl,
) -> Result<(), DeclarationError> {
    for source in SCALARS {
        for target in SCALARS {
            let Some(fallible) = checked_conversion_fallible(source, target) else {
                continue;
            };
            let mut interface =
                key::applied(ids, RegistrationTrait::TryFrom, vec![Ty::Builtin(source)])?;
            interface.associated_types.insert(
                identities::associated(ids, &interface.declaration, "Error")?,
                contracts::enum_type(
                    ids,
                    if fallible {
                        "TryFromIntError"
                    } else {
                        "Infallible"
                    },
                    vec![],
                )?,
            );
            implement(
                ids,
                module,
                interface,
                Ty::Builtin(target),
                vec![],
                "$foundation_try_from",
            )?;
        }
    }

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
