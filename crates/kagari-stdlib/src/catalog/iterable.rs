//! Iterable owns the finite lazy/terminal surface; cursors expose only next.
use crate::catalog::{
    contracts, identities, iterable_defaults,
    key::{self, RegistrationTrait},
};
use kagari_common::identity::{DefinitionKind, map::DefinitionContext};
use kagari_types::{
    declaration::{
        Param, TypeDef, TypeDefKind,
        module::{DeclarationError, ImplDecl, ModuleDecl},
        native::NativeStorageLayout,
    },
    scalar::BuiltinType,
    ty::{Constraint, GenericBound, GenericParam, NominalTy, Ty},
};
use std::collections::BTreeMap;

pub(super) fn adapter_type(
    ids: &DefinitionContext,
    name: &str,
    arguments: Vec<Ty>,
) -> Result<Ty, DeclarationError> {
    Ok(Ty::NativeObject(NominalTy {
        declaration: identities::definition(ids, DefinitionKind::AssociatedType, name)?,
        arguments,
        associated_types: BTreeMap::new(),
    }))
}

pub(super) fn declare(
    ids: &DefinitionContext,
    module: &mut ModuleDecl,
) -> Result<(), DeclarationError> {
    for (name, arity) in [
        ("MapIterator", 2),
        ("FilterIterator", 1),
        ("TakeIterator", 1),
        ("FilterMapIterator", 2),
        ("SkipIterator", 1),
        ("EnumerateIterator", 1),
        ("ZipIterator", 2),
        ("ChainIterator", 1),
    ] {
        let owner = identities::definition(ids, DefinitionKind::AssociatedType, name)?;
        module.documentation.insert(owner, format!("Lazy {name} cursor. Aliases share progress; the source and selected next target retain their original generation."));
        module.types.push(TypeDef {
            name: name.into(),
            kind: TypeDefKind::NativeStorage(NativeStorageLayout::Opaque),
            generic_params: (0..arity)
                .map(|position| GenericParam { owner, position })
                .collect(),
            bounds: vec![],
            fields: vec![],
            variants: vec![],
        });
        let owner = identities::implementation(ids, module.implementations.len())?;
        let parameters: Vec<_> = (0..arity)
            .map(|position| GenericParam { owner, position })
            .collect();
        let items: Vec<_> = parameters.iter().map(GenericParam::as_type).collect();
        let contract = module
            .traits
            .iter()
            .find(|t| t.name == "Iterator")
            .unwrap()
            .clone();
        let binding = identities::definition(
            ids,
            DefinitionKind::Function,
            &format!("$foundation_{name}_next"),
        )?;
        module.documentation.insert(identities::method(ids, &owner, "next")?, "Advance the retained source lazily. Recursive advancement of this cursor traps; completed source and callback effects survive failure.".into());
        module.implement_trait(
            &contract,
            ImplDecl {
                generic_params: parameters,
                bounds: vec![],
                trait_type: Some(contracts::applied_item(
                    ids,
                    RegistrationTrait::Iterator,
                    match name {
                        "EnumerateIterator" => {
                            Ty::Tuple(vec![Ty::Builtin(BuiltinType::USize), items[0].clone()])
                        }
                        "ZipIterator" => Ty::Tuple(items.clone()),
                        _ => items[arity - 1].clone(),
                    },
                )?),
                for_type: adapter_type(ids, name, items)?,
                methods: vec![],
            },
            &[("next", binding)],
            ids,
        )?;
    }
    let owner = key::identity(ids, RegistrationTrait::Iterable)?;
    let receiver = Ty::SelfType(owner);
    let item = Ty::Projection {
        receiver: Box::new(receiver.clone()),
        interface: Box::new(key::applied(ids, RegistrationTrait::Iterable, vec![])?),
        member: identities::associated(ids, &owner, "Item")?,
        arguments: vec![],
    };
    for name in ["map", "filter", "take"] {
        let parameter = GenericParam {
            owner: identities::method(ids, &owner, name)?,
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
            adapter_type(ids, adapter, arguments)?,
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
        iterable_defaults::apply(ids, module, &item, &mut method)?;
        module.documentation.insert(identities::method(ids, &owner, name)?, match name {
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
    for (name, protocol, target) in [
        ("collect", RegistrationTrait::FromIterator, "from_iter"),
        ("sum", RegistrationTrait::Sum, "sum"),
        ("product", RegistrationTrait::Product, "product"),
    ] {
        let parameter = GenericParam {
            owner: identities::method(ids, &owner, name)?,
            position: 0,
        };
        let mut method = contracts::method(name, vec![receiver.clone()], parameter.as_type());
        method.bounds.push(GenericBound {
            ty: parameter.as_type(),
            constraints: vec![Constraint::Trait(key::applied(
                ids,
                protocol,
                vec![item.clone()],
            )?)],
        });
        method.generic_params.push(parameter);
        iterable_defaults::apply(ids, module, &item, &mut method)?;
        module.documentation.insert(identities::method(ids, &owner, name)?, format!("Consume through the requested destination's {target} implementation; preserve its checked construction and arithmetic semantics."));
        module
            .traits
            .iter_mut()
            .find(|t| t.name == "Iterable")
            .unwrap()
            .methods
            .push(method);
    }
    let parameter = GenericParam {
        owner: identities::method(ids, &owner, "fold")?,
        position: 0,
    };
    let accumulator = parameter.as_type();
    let mut method = contracts::method("fold", vec![receiver.clone()], accumulator.clone());
    method.generic_params.push(parameter);
    method.params.extend([
        Param {
            name: "initial".into(),
            ty: accumulator.clone(),
            mutable: false,
        },
        Param {
            name: "callback".into(),
            ty: Ty::Function {
                params: vec![accumulator.clone(), item.clone()],
                result: Box::new(accumulator),
            },
            mutable: false,
        },
    ]);
    iterable_defaults::apply(ids, module, &item, &mut method)?;
    module.documentation.insert(identities::method(ids, &owner, "fold")?, "Consume in source order, passing the accumulator and each item to the callback. Empty input returns initial; completed source/callback effects survive failure.".into());
    module
        .traits
        .iter_mut()
        .find(|t| t.name == "Iterable")
        .unwrap()
        .methods
        .push(method);
    for (name, adapter) in [
        ("filter_map", "FilterMapIterator"),
        ("skip", "SkipIterator"),
        ("enumerate", "EnumerateIterator"),
        ("zip", "ZipIterator"),
        ("chain", "ChainIterator"),
        ("count", ""),
        ("find", ""),
        ("any", ""),
        ("all", ""),
        ("for_each", ""),
    ] {
        let generic = GenericParam {
            owner: identities::method(ids, &owner, name)?,
            position: 0,
        };
        let parameter = generic.as_type();
        let paired = matches!(name, "zip" | "chain");
        let output = if name == "zip" {
            Ty::Projection {
                receiver: Box::new(parameter.clone()),
                interface: Box::new(key::applied(ids, RegistrationTrait::Iterable, vec![])?),
                member: identities::associated(ids, &owner, "Item")?,
                arguments: vec![],
            }
        } else if name == "filter_map" {
            parameter.clone()
        } else {
            item.clone()
        };
        let result = match name {
            "count" => Ty::Builtin(BuiltinType::USize),
            "find" => contracts::enum_type(ids, "Option", vec![item.clone()])?,
            "any" | "all" => contracts::boolean(),
            "for_each" => contracts::unit(),
            _ => adapter_type(
                ids,
                adapter,
                if matches!(name, "zip" | "filter_map") {
                    vec![item.clone(), output.clone()]
                } else {
                    vec![item.clone()]
                },
            )?,
        };
        let mut method = contracts::method(name, vec![receiver.clone()], result);
        if paired || name == "filter_map" {
            method.generic_params.push(generic);
        }
        if paired {
            method.bounds.push(GenericBound {
                ty: parameter.clone(),
                constraints: vec![Constraint::Trait(if name == "chain" {
                    contracts::applied_item(ids, RegistrationTrait::Iterable, item.clone())?
                } else {
                    key::applied(ids, RegistrationTrait::Iterable, vec![])?
                })],
            });
        }
        let argument = match name {
            "enumerate" | "count" => None,
            "zip" | "chain" => Some(("other", parameter)),
            "skip" => Some(("count", Ty::Builtin(BuiltinType::USize))),
            _ => Some((
                "callback",
                Ty::Function {
                    params: vec![item.clone()],
                    result: Box::new(match name {
                        "filter_map" => contracts::enum_type(ids, "Option", vec![output])?,
                        "for_each" => contracts::unit(),
                        _ => contracts::boolean(),
                    }),
                },
            )),
        };
        if let Some((name, ty)) = argument {
            method.params.push(Param {
                name: name.into(),
                ty,
                mutable: false,
            });
        }
        iterable_defaults::apply(ids, module, &item, &mut method)?;
        module.documentation.insert(identities::method(ids, &owner, name)?, match name {
            "filter_map" => "Lazily return the callback's Some values in source order, without fusing the source.",
            "skip" => "Discard only the initial count items; aliases share the remaining skip state.",
            "enumerate" => "Lazily pair each item with its checked usize position, starting at zero.",
            "zip" => "Lazily pull left then right and return pairs. A left item stays consumed if right returns None.",
            "chain" => "Lazily traverse the first cursor, then switch permanently to the second on its first None.",
            "count" => "Consume until the first None and return the checked usize count.",
            "find" => "Return the first matching item, stopping immediately; empty input returns None.",
            "any" => "Stop at the first true predicate; empty input returns false.",
            "all" => "Stop at the first false predicate; empty input returns true.",
            _ => "Run the action once per item in source order and return unit.",
        }.into());
        module
            .traits
            .iter_mut()
            .find(|t| t.name == "Iterable")
            .unwrap()
            .methods
            .push(method);
    }
    // An erased Iterator has a concrete boxed representation but no concrete
    // source table. Publish its identity Iterable table so generic member
    // witnesses use the same default entries without erasing other receivers.
    let implementation = identities::implementation(ids, module.implementations.len())?;
    let parameter = GenericParam {
        owner: implementation,
        position: 0,
    };
    let item = parameter.as_type();
    let receiver = Ty::Trait(contracts::applied_item(
        ids,
        RegistrationTrait::Iterator,
        item.clone(),
    )?);
    let mut iterable = contracts::applied_item(ids, RegistrationTrait::Iterable, item)?;
    iterable.associated_types.insert(
        identities::associated(ids, &owner, "Iter")?,
        receiver.clone(),
    );
    let contract = module
        .traits
        .iter()
        .find(|t| t.name == "Iterable")
        .unwrap()
        .clone();
    let binding = identities::definition(
        ids,
        DefinitionKind::Function,
        "$foundation_iterator_identity_iter",
    )?;
    module.implement_trait(
        &contract,
        ImplDecl {
            generic_params: vec![parameter],
            bounds: vec![],
            trait_type: Some(iterable),
            for_type: receiver,
            methods: vec![],
        },
        &[("iter", binding)],
        ids,
    )?;

    Ok(())
}
