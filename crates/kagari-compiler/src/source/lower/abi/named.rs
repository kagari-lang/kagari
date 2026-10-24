//! Inventory authored declarations before instance reachability removes templates.

use super::{
    abi_type, canonical_bounds, checked_bounds, const_abi_value, constraint_abi, function_abi,
    generic_param_abi, parameter_bounds,
};
use kagari_contract::types::named::{
    AliasContract, ImplementationContract, NamedContract, NamedDeclaration,
};
use kagari_hir::{
    AnalyzedModule, declarations::DeclarationId, hir::item::Item, resolver::resolved::ResolvedName,
};
use kagari_types::{
    declaration::ConstDef,
    ty::{GenericBound, GenericParam, Ty},
    visibility::Visibility,
};

pub(super) fn collect(module: &AnalyzedModule) -> Vec<NamedDeclaration> {
    let hir = &module.lowered.module;
    let definition = |name| {
        let DeclarationId::Definition(id) = module
            .declarations
            .target(name)
            .expect("checked authored declaration")
            .id
        else {
            unreachable!("named definition")
        };
        id
    };
    let mut records = Vec::new();
    for function in &hir.functions {
        let mut signature = function_abi(module, function).expect("checked function contract");
        let typed = module
            .typed
            .functions
            .iter()
            .find(|typed| typed.id == function.id)
            .expect("checked function");
        signature.generic_params = typed
            .generic_params
            .iter()
            .map(|parameter| GenericParam {
                owner: parameter.owner,
                position: parameter.position,
            })
            .collect();
        records.push(NamedDeclaration {
            declaration: definition(ResolvedName::Function(function.id)),
            visibility: function.visibility,
            ambiguous_local_body: module
                .declarations
                .ambiguous_local_owner(ResolvedName::Function(function.id))
                .map(|owner| {
                    let item = match module
                        .declarations
                        .definition_target(&owner)
                        .expect("authored local owner")
                    {
                        ResolvedName::Function(id) => Item::Function(id),
                        ResolvedName::Const(id) => Item::Const(id),
                        ResolvedName::Static(id) => Item::Static(id),
                        _ => unreachable!("local callable owner"),
                    };
                    let span = module.lowered.source_map.item_span(item);
                    module.lowered.source.text()[span.start..span.end].to_owned()
                }),
            contract: NamedContract::Function(signature),
        });
    }
    for item in &hir.consts {
        records.push(NamedDeclaration {
            declaration: definition(ResolvedName::Const(item.id)),
            visibility: item.visibility,
            ambiguous_local_body: None,
            contract: NamedContract::Constant(ConstDef {
                name: item.name.clone(),
                ty: abi_type(
                    module,
                    module
                        .typed
                        .consts
                        .get(&item.id)
                        .expect("checked constant type"),
                ),
                value: const_abi_value(
                    module
                        .typed
                        .const_values
                        .get(&item.id)
                        .expect("checked constant value"),
                    module.declarations.definitions(),
                ),
            }),
        });
    }
    for alias in &hir.type_aliases {
        let declaration = definition(ResolvedName::TypeAlias(alias.id));
        let template = module
            .declarations
            .imported_types()
            .alias(&declaration)
            .expect("checked normalized alias");
        records.push(NamedDeclaration {
            declaration,
            visibility: alias.visibility,
            ambiguous_local_body: None,
            contract: NamedContract::Alias(AliasContract {
                name: alias.name.clone(),
                generic_params: generic_param_abi(module, &alias.generic_params),
                bounds: checked_bounds(
                    &module.aggregates,
                    module.declarations.definitions(),
                    &template.bounds,
                ),
                target: abi_type(module, &template.target),
            }),
        });
    }
    for implementation in &hir.impls {
        let declaration = *module
            .declarations
            .impl_identity(implementation.id)
            .expect("checked impl owner");
        let receiver = abi_type(
            module,
            &module
                .typed
                .type_table
                .type_ref(implementation.for_type.expect("checked receiver"))
                .expect("checked receiver type")
                .ty,
        );
        let trait_type = implementation.trait_ref.as_ref().map(|reference| {
            Ty::Trait(constraint_abi(
                &module.aggregates,
                module.declarations.definitions(),
                module
                    .typed
                    .type_table
                    .constraint(reference.ty)
                    .expect("checked implemented trait"),
            ))
        });
        let bounds =
            if let Some(signature) = module.aggregates.implementation_signature(&declaration) {
                checked_bounds(
                    &module.aggregates,
                    module.declarations.definitions(),
                    &signature.bounds,
                )
            } else {
                let mut bounds = parameter_bounds(module, &implementation.generic_params);
                for bound in &implementation.bounds {
                    bounds.push(GenericBound {
                        ty: abi_type(
                            module,
                            &module
                                .typed
                                .type_table
                                .type_ref(bound.target_ref)
                                .expect("checked impl bound target")
                                .ty,
                        ),
                        constraints: bound
                            .traits
                            .iter()
                            .map(|reference| {
                                constraint_abi(
                                    &module.aggregates,
                                    module.declarations.definitions(),
                                    module
                                        .typed
                                        .type_table
                                        .constraint(reference.ty)
                                        .expect("checked impl bound"),
                                )
                            })
                            .collect(),
                    });
                }
                canonical_bounds(module.declarations.definitions(), bounds)
            };
        records.push(NamedDeclaration {
            declaration,
            visibility: Visibility::Private,
            ambiguous_local_body: None,
            contract: NamedContract::Implementation(ImplementationContract {
                bridge: None,
                generic_params: generic_param_abi(module, &implementation.generic_params),
                bounds,
                receiver,
                trait_type,
            }),
        });
    }
    records
}
