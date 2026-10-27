//! Preserve concrete host access descriptors after validating their source view.
use crate::{
    hir::{
        ids::TypeRefId,
        item::{
            behavior::{TraitBound, TraitRef},
            function::FunctionKind,
        },
        ty::TypeKind,
    },
    lower::LoweredModule,
};
use kagari_common::identity::table::DefinitionTable;
use kagari_types::{
    declaration::module::{DeclarationError, ModuleDecl},
    ty::{GenericBound, NominalTy, Ty},
};

pub(super) fn attach(
    definition: &ModuleDecl,
    definitions: &DefinitionTable,
    lowered: &mut LoweredModule,
) -> Result<(), DeclarationError> {
    let invalid = || DeclarationError("registered concrete access correspondence".into());
    let mut pending = Vec::new();
    let mut functions = Vec::new();
    for declaration in &definition.types {
        if let Some(item) = lowered
            .module
            .enums
            .iter()
            .find(|item| item.name == declaration.name)
        {
            for variant in &declaration.variants {
                let syntax = item
                    .variants
                    .iter()
                    .find(|item| item.name == variant.name)
                    .ok_or_else(invalid)?;
                pending.extend(
                    syntax
                        .payload
                        .iter()
                        .copied()
                        .zip(variant.payload.iter().cloned()),
                );
            }
        } else if let Some(item) = lowered
            .module
            .opaque_types
            .iter()
            .find(|item| item.name == declaration.name)
        {
            bounds(&mut pending, &item.bounds, &declaration.bounds);
        }
    }
    for (item, implementation) in lowered.module.impls.iter().zip(&definition.implementations) {
        pending.push((
            item.for_type.ok_or_else(invalid)?,
            implementation.for_type.clone(),
        ));
        bounds(&mut pending, &item.bounds, &implementation.bounds);
        if let (Some(reference), Some(interface)) = (&item.trait_ref, &implementation.trait_type) {
            let mut header = interface.clone();
            header.associated_types.clear();
            pending.push((reference.ty, Ty::Trait(header)));
            for (member, ty) in &interface.associated_types {
                let name = definitions
                    .resolve(*member)
                    .map_err(|_| invalid())?
                    .last()
                    .ok_or_else(invalid)?
                    .name;
                let syntax = item
                    .associated_types
                    .iter()
                    .find(|item| item.name == name)
                    .ok_or_else(invalid)?;
                pending.push((syntax.ty.ok_or_else(invalid)?, ty.clone()));
            }
        }
        for method in &implementation.methods {
            let member = item
                .methods
                .iter()
                .find(|member| member.name == method.name)
                .ok_or_else(invalid)?;
            functions.push((member.function, method));
        }
    }
    for contract in &definition.traits {
        let item = lowered
            .module
            .traits
            .iter()
            .find(|item| item.name == contract.name)
            .ok_or_else(invalid)?;
        pending.extend(
            item.supertraits
                .iter()
                .zip(&contract.supertraits)
                .map(|(syntax, ty)| (syntax.ty, Ty::Trait(ty.clone()))),
        );
        for declaration in &contract.associated_types {
            let name = definitions
                .resolve(declaration.declaration)
                .map_err(|_| invalid())?
                .last()
                .ok_or_else(invalid)?
                .name;
            let syntax = item
                .associated_types
                .iter()
                .find(|item| item.name == name)
                .ok_or_else(invalid)?;
            constraints(&mut pending, &syntax.bounds, &declaration.bounds);
        }
        for method in &contract.methods {
            let member = item
                .methods
                .iter()
                .find(|member| member.name == method.name)
                .ok_or_else(invalid)?;
            functions.push((member.function, method));
        }
    }
    for function in &definition.functions {
        let item = lowered
            .module
            .functions
            .iter()
            .find(|item| item.kind == FunctionKind::User && item.name == function.name)
            .ok_or_else(invalid)?;
        functions.push((item.id, function));
    }
    for (id, declaration) in functions {
        let function = lowered
            .module
            .functions
            .iter()
            .find(|function| function.id == id)
            .ok_or_else(invalid)?;
        bounds(&mut pending, &function.bounds, &declaration.bounds);
        pending.extend(
            function
                .params
                .iter()
                .zip(&declaration.params)
                .map(|(parameter, declared)| (parameter.ty, declared.ty.clone())),
        );
        if let Some(result) = function.return_type {
            pending.push((result, declaration.return_type.clone()));
        }
    }
    attach_occurrences(definitions, lowered, pending)
}

fn attach_occurrences(
    definitions: &DefinitionTable,
    lowered: &mut LoweredModule,
    mut pending: Vec<(TypeRefId, Ty)>,
) -> Result<(), DeclarationError> {
    let invalid = || DeclarationError("registered concrete access correspondence".into());
    while let Some((id, ty)) = pending.pop() {
        let syntax = &lowered.module.type_ref(id).kind;
        let mut children: Vec<(TypeRefId, Ty)> = Vec::new();
        match (syntax, ty) {
            (TypeKind::Generic { args, .. }, Ty::Iter(item) | Ty::Range(item, _)) => {
                children.extend(args.iter().copied().zip([*item]));
            }

            (
                TypeKind::Generic { args, bindings, .. },
                Ty::NativeObject(nominal) | Ty::Trait(nominal) | Ty::Enum(nominal),
            ) => {
                children.extend(args.iter().copied().zip(nominal.arguments));
                for (member, ty) in nominal.associated_types {
                    let name = definitions
                        .resolve(member)
                        .map_err(|_| invalid())?
                        .last()
                        .ok_or_else(invalid)?
                        .name;
                    let (_, id) = bindings
                        .iter()
                        .find(|(candidate, _)| candidate == name)
                        .ok_or_else(invalid)?;
                    children.push((*id, ty));
                }
            }
            (TypeKind::Tuple(elements), Ty::Tuple(types)) => {
                children.extend(elements.iter().copied().zip(types))
            }
            (TypeKind::Array(element), Ty::Array(ty)) => children.push((*element, *ty)),
            (
                TypeKind::Function { params, result },
                Ty::Function {
                    params: types,
                    result: ty,
                },
            ) => {
                children.extend(params.iter().copied().zip(types));
                children.push((*result, *ty));
            }
            (
                TypeKind::Projection {
                    receiver,
                    trait_ref,
                    arguments,
                    ..
                },
                Ty::Projection {
                    receiver: ty,
                    interface,
                    arguments: types,
                    ..
                },
            ) => {
                children.push((*receiver, *ty));
                children.push((*trait_ref, Ty::Trait(*interface)));
                children.extend(arguments.iter().copied().zip(types));
            }
            _ => {}
        }
        pending.extend(children);
    }
    Ok(())
}

// The authoritative renderer emits where predicates and constraints in this order.
fn bounds(
    pending: &mut Vec<(TypeRefId, Ty)>,
    syntax: &[TraitBound],
    declarations: &[GenericBound],
) {
    for (syntax, declaration) in syntax.iter().zip(declarations) {
        pending.push((syntax.target_ref, declaration.ty.clone()));
        constraints(pending, &syntax.traits, &declaration.constraints);
    }
}

fn constraints(
    pending: &mut Vec<(TypeRefId, Ty)>,
    syntax: &[TraitRef],
    declarations: &[NominalTy],
) {
    for (syntax, declaration) in syntax.iter().zip(declarations) {
        pending.push((syntax.ty, Ty::Trait(declaration.clone())));
    }
}
