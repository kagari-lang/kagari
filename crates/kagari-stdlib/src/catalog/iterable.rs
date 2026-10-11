//! Iterable owns the finite lazy/terminal surface; cursors expose only next.
use crate::catalog::{contracts, key, key::RegistrationTrait};
use kagari_common::identity::{DefinitionKind, associated_type_id};
use kagari_types::{
    callable::{CallableImplementation, NativeDefaultApplication},
    declaration::{
        FnDecl, Param, TypeDef, TypeDefKind, module::ModuleDecl, native::NativeStorageLayout,
        requirement::NativeCallableRequirement,
    },
    scalar::BuiltinType,
    ty::{
        Constraint, GenericBound, GenericParam, NominalTy, Ty,
        substitution::{TypeSubstitution, resolve_associated_outputs},
    },
};
use std::collections::BTreeMap;

pub(super) fn adapter_type(module: &ModuleDecl, name: &str, arguments: Vec<Ty>) -> Ty {
    Ty::NativeObject(NominalTy {
        declaration: module.definition(DefinitionKind::AssociatedType, name),
        arguments,
        associated_types: BTreeMap::new(),
    })
}

pub(super) fn declare(module: &mut ModuleDecl) {
    for (name, arity) in [
        ("MapIterator", 2),
        ("FilterIterator", 1),
        ("TakeIterator", 1),
    ] {
        let owner = module.definition(DefinitionKind::AssociatedType, name);
        module.documentation.insert(owner.clone(), format!("Lazy {name} cursor. Aliases share progress; the source and selected next target retain their original generation."));
        module.types.push(TypeDef {
            name: name.into(),
            kind: TypeDefKind::NativeStorage(NativeStorageLayout::Opaque),
            generic_params: (0..arity)
                .map(|position| GenericParam {
                    owner: owner.clone(),
                    position,
                })
                .collect(),
            bounds: vec![],
            fields: vec![],
            variants: vec![],
        });
        let owner = module.implementation_id(module.implementations.len());
        let parameters: Vec<_> = (0..arity)
            .map(|position| GenericParam {
                owner: owner.clone(),
                position,
            })
            .collect();
        let items: Vec<_> = parameters.iter().map(GenericParam::as_type).collect();
        let contract = module
            .traits
            .iter()
            .find(|t| t.name == "Iterator")
            .unwrap()
            .clone();
        let binding = module.definition(
            DefinitionKind::Function,
            &format!("$foundation_{name}_next"),
        );
        module.documentation.insert(ModuleDecl::method_id(&owner, "next"), "Advance the retained source lazily. Recursive advancement of this cursor traps; completed source and callback effects survive failure.".into());
        module
            .implement_trait(
                &contract,
                contracts::applied_item(RegistrationTrait::Iterator, items[arity - 1].clone()),
                adapter_type(module, name, items),
                parameters,
                &[("next", binding)],
            )
            .expect("adapter Iterator");
    }
    let owner = key::identity(RegistrationTrait::Iterable);
    let receiver = Ty::SelfType(owner.clone());
    let item = Ty::Projection {
        receiver: Box::new(receiver.clone()),
        interface: Box::new(key::applied(RegistrationTrait::Iterable, vec![])),
        member: associated_type_id(&owner, "Item"),
        arguments: vec![],
    };
    for name in ["map", "filter", "take"] {
        let parameter = GenericParam {
            owner: ModuleDecl::method_id(&owner, name),
            position: 0,
        };
        let output = if name == "map" {
            parameter.as_type()
        } else {
            item.clone()
        };
        let adapter = match name {
            "map" => "MapIterator",
            "filter" => "FilterIterator",
            _ => "TakeIterator",
        };
        let arguments = if name == "map" {
            vec![item.clone(), output.clone()]
        } else {
            vec![item.clone()]
        };
        let mut method = contracts::method(
            name,
            vec![receiver.clone()],
            adapter_type(module, adapter, arguments),
        );
        if name == "map" {
            method.generic_params.push(parameter);
        }
        method.params.push(Param {
            name: if name == "take" { "limit" } else { "callback" }.into(),
            ty: if name == "take" {
                Ty::Builtin(BuiltinType::USize)
            } else {
                Ty::Function {
                    params: vec![item.clone()],
                    result: Box::new(if name == "filter" {
                        contracts::boolean()
                    } else {
                        output
                    }),
                }
            },
            mutable: false,
        });
        default(module, &item, &mut method);
        module.documentation.insert(ModuleDecl::method_id(&owner, name), match name {
            "map" => "Lazily transform each element with the callback. Obtain the source cursor once; copies share progress.",
            "filter" => "Lazily return elements accepted by the predicate, preserving source order and callback effects.",
            _ => "Lazily return at most limit elements. A zero limit never advances the source.",
        }.into());
        module
            .traits
            .iter_mut()
            .find(|t| t.name == "Iterable")
            .unwrap()
            .methods
            .push(method);
    }
}

fn default(module: &mut ModuleDecl, item: &Ty, method: &mut FnDecl) {
    let owner = key::identity(RegistrationTrait::Iterable);
    let id = module.definition(
        DefinitionKind::Function,
        &format!("__default_Iterable_{}", method.name),
    );
    let mut arguments: Vec<_> = [Ty::SelfType(owner.clone()), item.clone()]
        .into_iter()
        .chain(method.generic_params.iter().map(GenericParam::as_type))
        .collect();
    let iterable = key::applied(RegistrationTrait::Iterable, vec![]);
    arguments.push(Ty::Projection {
        receiver: Box::new(arguments[0].clone()),
        member: associated_type_id(&owner, "Iter"),
        interface: Box::new(iterable),
        arguments: vec![],
    });
    let parameters: Vec<_> = (0..arguments.len())
        .map(|position| GenericParam {
            owner: id.clone(),
            position,
        })
        .collect();
    let types: Vec<_> = parameters.iter().map(GenericParam::as_type).collect();
    let cursor = types.last().unwrap().clone();
    let mut substitution = TypeSubstitution::default();
    substitution.bind_receiver(&owner, &types[0]);
    // Associated outputs are checked hidden parameters of the shared template.
    let cancel = Default::default();
    for (parameter, ty) in method.generic_params.iter().zip(&types[2..]) {
        substitution.bind(&parameter.owner, parameter.position, ty);
    }
    let mut iterable = contracts::applied_item(RegistrationTrait::Iterable, types[1].clone());
    iterable
        .associated_types
        .insert(associated_type_id(&owner, "Iter"), cursor.clone());
    let normalize = |ty: &Ty| {
        resolve_associated_outputs(
            &substitution
                .apply(ty, &cancel)
                .expect("Iterable substitution"),
            &iterable,
            &cancel,
        )
        .expect("Iterable outputs")
    };
    let iterator = contracts::applied_item(RegistrationTrait::Iterator, types[1].clone());
    let requirements = vec![
        NativeCallableRequirement {
            receiver: types[0].clone(),
            member: ModuleDecl::method_id(&owner, "iter"),
            interface: iterable.clone(),
            arguments: vec![],
        },
        NativeCallableRequirement {
            receiver: cursor.clone(),
            member: ModuleDecl::method_id(&iterator.declaration, "next"),
            interface: iterator.clone(),
            arguments: vec![],
        },
    ];
    let mut template = method.clone();
    template.name = id.path.last().unwrap().name.clone();
    template.generic_params = parameters;
    template.implementation = CallableImplementation::Native(id.clone());
    template.bounds = vec![
        GenericBound {
            ty: types[0].clone(),
            constraints: vec![Constraint::Trait(iterable.clone())],
        },
        GenericBound {
            ty: cursor,
            constraints: vec![Constraint::Trait(iterator)],
        },
    ];
    for parameter in &mut template.params {
        if parameter.name == "self" {
            parameter.name = "receiver".into();
        }
        parameter.ty = normalize(&parameter.ty);
    }
    template.return_type = normalize(&template.return_type);
    module.private_functions.insert(id.clone());
    module
        .callable_requirements
        .insert(id.clone(), requirements);
    module.functions.push(template);
    method.implementation = CallableImplementation::NativeDefault(NativeDefaultApplication {
        declaration: id,
        arguments,
    });
}
