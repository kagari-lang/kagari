//! Base implementations use ordinary native declarations and selected callbacks.
use crate::{
    declaration::ModuleDecl,
    language::{Protocol, catalog::contracts, primitive},
    native_import::callables::NativeCallableRequirement,
    scalar::BuiltinType,
    standard::surface::StandardEnum,
    types::{AbiType, GenericParameterAbi, NominalAbiType},
};
use kagari_common::{
    collection::CollectionAccess,
    identity::{DefinitionKind, associated_type_id},
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
    for target in SCALARS {
        let mut interface = primitive::applied(Protocol::FromStr, vec![]);
        interface.associated_types.insert(
            associated_type_id(&interface.declaration, "Err"),
            contracts::enum_type(StandardEnum::ParseError, vec![]),
        );
        implement(
            module,
            interface,
            AbiType::Builtin(target),
            vec![],
            "$foundation_from_str",
        );
        if target.number_type().is_some() {
            for (kind, binding) in [
                (Protocol::Sum, "$foundation_sum"),
                (Protocol::Product, "$foundation_product"),
            ] {
                let item = AbiType::Builtin(target);
                implement(
                    module,
                    primitive::applied(kind, vec![item.clone()]),
                    item.clone(),
                    vec![],
                    binding,
                );
                iteration_calls(module, item);
            }
        }
    }
    let parameter = GenericParameterAbi {
        owner: module.implementation_id(module.implementations.len()),
        position: 0,
    };
    let item = parameter.as_type();
    implement(
        module,
        primitive::applied(Protocol::FromIterator, vec![item.clone()]),
        AbiType::Array(Box::new(item.clone()), CollectionAccess::Mutable),
        vec![parameter],
        "$foundation_list_from_iter",
    );
    iteration_calls(module, item);
}

fn implement(
    module: &mut ModuleDecl,
    interface: NominalAbiType,
    receiver: AbiType,
    parameters: Vec<GenericParameterAbi>,
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

fn iteration_calls(module: &mut ModuleDecl, item: AbiType) {
    let owner = module.implementation_id(module.implementations.len() - 1);
    let method = &module.implementations.last().unwrap().methods[0];
    let source = method.params[0].ty.clone();
    let iterable = contracts::applied_item(Protocol::Iterable, item.clone());
    let cursor = AbiType::Projection {
        receiver: Box::new(source.clone()),
        member: associated_type_id(&iterable.declaration, "Iter"),
        interface: Box::new(iterable.clone()),
        arguments: vec![],
    };
    let iterator = contracts::applied_item(Protocol::Iterator, item);
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
