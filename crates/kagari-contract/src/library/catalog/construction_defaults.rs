//! Base implementations use ordinary native declarations and selected callbacks.
use crate::library::catalog::key::{self, RegistrationTrait};
use crate::{
    declaration::ModuleDecl,
    library::catalog::contracts,
    native_import::callables::NativeCallableRequirement,
    scalar::BuiltinType,
    standard::surface::StandardEnum,
    types::{GenericParam, NominalTy, Ty},
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
        let mut interface = key::applied(RegistrationTrait::FromStr, vec![]);
        interface.associated_types.insert(
            associated_type_id(&interface.declaration, "Err"),
            contracts::enum_type(StandardEnum::ParseError, vec![]),
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
        Ty::Array(Box::new(item.clone()), CollectionAccess::Mutable),
        vec![parameter],
        "$foundation_list_from_iter",
    );
    iteration_calls(module, item);
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
