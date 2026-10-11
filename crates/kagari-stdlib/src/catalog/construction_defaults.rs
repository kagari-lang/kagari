//! Base implementations use ordinary native declarations and selected callbacks.
use crate::catalog::{contracts, defaults};
use crate::catalog::{key, key::RegistrationTrait};
use kagari_common::identity::{DefinitionKind, associated_type_id};
use kagari_types::{
    collection::CollectionAccess,
    conversion::checked_conversion_fallible,
    declaration::{module::ModuleDecl, requirement::NativeCallableRequirement},
    scalar::BuiltinType,
    ty::{GenericParam, NominalTy, Ty},
};

const SCALARS: [BuiltinType; 13] = [
    BuiltinType::Bool,
    BuiltinType::I8,
    BuiltinType::I16,
    BuiltinType::I32,
    BuiltinType::I64,
    BuiltinType::ISize,
    BuiltinType::U8,
    BuiltinType::U16,
    BuiltinType::U32,
    BuiltinType::U64,
    BuiltinType::USize,
    BuiltinType::F32,
    BuiltinType::F64,
];

pub(super) fn declare(module: &mut ModuleDecl) {
    checked_conversions(module);
    for target in SCALARS {
        let mut interface = key::applied(RegistrationTrait::FromStr, vec![]);
        interface.associated_types.insert(
            associated_type_id(&interface.declaration, "Err"),
            contracts::enum_type("ParseError", vec![]),
        );
        implement(
            module,
            interface,
            Ty::Builtin(target),
            vec![],
            "$foundation_from_str",
        );
        if target.number_type().is_some() {
            for (kind, binding) in [
                (RegistrationTrait::Sum, "$foundation_sum"),
                (RegistrationTrait::Product, "$foundation_product"),
            ] {
                let item = Ty::Builtin(target);
                implement(
                    module,
                    key::applied(kind, vec![item.clone()]),
                    item.clone(),
                    vec![],
                    binding,
                );
                iteration_calls(module, item);
            }
        }
    }
    let parameter = GenericParam {
        owner: module.implementation_id(module.implementations.len()),
        position: 0,
    };
    let item = parameter.as_type();
    implement(
        module,
        key::applied(RegistrationTrait::FromIterator, vec![item.clone()]),
        contracts::vec_type(item.clone()),
        vec![parameter],
        "$foundation_list_from_iter",
    );
    iteration_calls(module, item);
    hash_constructors(module);
}

fn checked_conversions(module: &mut ModuleDecl) {
    for source in SCALARS {
        for target in SCALARS {
            let Some(fallible) = checked_conversion_fallible(source, target) else {
                continue;
            };
            let mut interface = key::applied(RegistrationTrait::TryFrom, vec![Ty::Builtin(source)]);
            interface.associated_types.insert(
                associated_type_id(&interface.declaration, "Error"),
                contracts::enum_type(
                    if fallible {
                        "TryFromIntError"
                    } else {
                        "Infallible"
                    },
                    vec![],
                ),
            );
            implement(
                module,
                interface,
                Ty::Builtin(target),
                vec![],
                "$foundation_try_from",
            );
        }
    }
}

fn implement(
    module: &mut ModuleDecl,
    interface: NominalTy,
    receiver: Ty,
    parameters: Vec<GenericParam>,
    binding: &str,
) {
    let contract = module
        .traits
        .iter()
        .find(|contract| {
            module.definition(DefinitionKind::Trait, &contract.name) == interface.declaration
        })
        .expect("foundation contract")
        .clone();
    let method = contract.methods[0].name.as_str();
    let binding = module.definition(DefinitionKind::Function, binding);
    module
        .implement_trait(
            &contract,
            interface,
            receiver,
            parameters,
            &[(method, binding)],
        )
        .expect("foundation implementation");
}

fn iteration_calls(module: &mut ModuleDecl, item: Ty) {
    let owner = module.implementation_id(module.implementations.len() - 1);
    let method = &module.implementations.last().unwrap().methods[0];
    let source = method.params[0].ty.clone();
    let iterable = contracts::applied_item(RegistrationTrait::Iterable, item.clone());
    let cursor = Ty::Projection {
        receiver: Box::new(source.clone()),
        member: associated_type_id(&iterable.declaration, "Iter"),
        interface: Box::new(iterable.clone()),
        arguments: vec![],
    };
    let iterator = contracts::applied_item(RegistrationTrait::Iterator, item);
    module.callable_requirements.insert(
        ModuleDecl::method_id(&owner, &method.name),
        vec![
            NativeCallableRequirement {
                receiver: source,
                member: ModuleDecl::method_id(&iterable.declaration, "iter"),
                interface: iterable,
                arguments: vec![],
            },
            NativeCallableRequirement {
                receiver: cursor,
                member: ModuleDecl::method_id(&iterator.declaration, "next"),
                interface: iterator,
                arguments: vec![],
            },
        ],
    );
}

fn hash_constructors(module: &mut ModuleDecl) {
    for map in [false, true] {
        let owner = module.implementation_id(module.implementations.len());
        let parameters: Vec<_> = (0..if map { 2 } else { 1 })
            .map(|position| GenericParam {
                owner: owner.clone(),
                position,
            })
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
            module,
            key::applied(RegistrationTrait::FromIterator, vec![item.clone()]),
            receiver.clone(),
            parameters,
            if map {
                "$foundation_map_from_iter"
            } else {
                "$foundation_set_from_iter"
            },
        );
        module.implementations.last_mut().unwrap().bounds = defaults::hash_bounds(key_type.clone());
        iteration_calls(module, item);
        let method = ModuleDecl::method_id(&owner, "from_iter");
        let iteration = module.callable_requirements.remove(&method).unwrap();
        let mut requirements: Vec<_> = [
            (RegistrationTrait::Hash, "hash"),
            (RegistrationTrait::PartialEq, "eq"),
        ]
        .into_iter()
        .map(|(protocol, name)| {
            let interface = key::applied(protocol, vec![]);
            NativeCallableRequirement {
                receiver: key_type.clone(),
                member: ModuleDecl::method_id(&interface.declaration, name),
                interface,
                arguments: vec![],
            }
        })
        .collect();
        let interface = key::applied(
            if map {
                RegistrationTrait::MutableMap
            } else {
                RegistrationTrait::MutableSet
            },
            value.map_or_else(
                || vec![key_type.clone()],
                |value| vec![key_type.clone(), value],
            ),
        );
        requirements.push(NativeCallableRequirement {
            receiver,
            member: ModuleDecl::method_id(&interface.declaration, "insert"),
            interface,
            arguments: vec![],
        });
        requirements.extend(iteration);
        module.callable_requirements.insert(method, requirements);
    }
}
