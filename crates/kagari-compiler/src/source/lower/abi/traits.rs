//! Trait declaration order paired with canonical checked member contracts.

use super::{abi_parameters, checked_bounds, const_abi_value, constraint_abi, method_abi};
use kagari_common::identity::DefinitionKind;
use kagari_contract::types::TraitContract;
use kagari_hir::{
    compile_input::program::CheckedModule, hir::item::behavior::TraitDef as HirTraitDef,
    resolver::resolved::ResolvedName, types::semantic::lower_type,
};
use kagari_types::{
    declaration::{AssociatedConstDef, AssociatedTypeDef, TraitDef},
    ty::NominalTy,
};

pub(super) fn lower(module: &CheckedModule<'_>, header: &HirTraitDef) -> TraitContract {
    let environment = module.environment();
    let definitions = environment.definitions();
    let catalog = environment.aggregates();
    let declaration = environment
        .definition(ResolvedName::Trait(header.id))
        .expect("admitted trait identity");
    let signature = catalog
        .trait_(&declaration)
        .expect("admitted trait contract");
    let member_id = |kind, name: &str| {
        definitions
            .lookup_child(declaration, kind, name, 0)
            .expect("admitted associated member")
    };
    let abi = TraitDef {
        conversion_adapter: signature.conversion_adapter.clone(),
        associated_consts: header
            .associated_consts
            .iter()
            .map(|member| {
                let declaration = member_id(DefinitionKind::Const, &member.name);
                let constant = &signature.associated_consts[&declaration];
                AssociatedConstDef {
                    declaration,
                    ty: lower_type(&constant.ty),
                    default_value: member.initializer.map(|id| {
                        const_abi_value(
                            environment
                                .local_constant(id)
                                .expect("admitted default constant")
                                .value,
                            definitions,
                        )
                    }),
                }
            })
            .collect(),
        supertraits: signature
            .supertraits
            .iter()
            .cloned()
            .map(|parent| constraint_abi(catalog, definitions, parent))
            .collect(),
        associated_types: header
            .associated_types
            .iter()
            .map(|member| {
                let declaration = member_id(DefinitionKind::AssociatedType, &member.name);
                let inputs = signature.associated_type_parameters.get(&declaration);
                let bounds: Vec<_> = signature.associated_types[&declaration]
                    .iter()
                    .cloned()
                    .map(|constraint| constraint_abi(catalog, definitions, constraint))
                    .collect();
                AssociatedTypeDef {
                    declaration,
                    generic_params: inputs
                        .map(|inputs| abi_parameters(&inputs.parameters))
                        .unwrap_or_default(),
                    parameter_bounds: inputs
                        .map(|inputs| checked_bounds(catalog, definitions, &inputs.bounds))
                        .unwrap_or_default(),
                    bounds: NominalTy::normalize_bounds(bounds, definitions, &Default::default())
                        .expect("checked associated constraint identities"),
                }
            })
            .collect(),
        name: header.name.clone(),
        generic_params: abi_parameters(&signature.generic_params),
        bounds: checked_bounds(catalog, definitions, &signature.bounds),
        methods: header
            .methods
            .iter()
            .map(|method| {
                method_abi(
                    module,
                    &module.headers().functions[method.function.index()],
                    &signature.generic_params,
                    &signature.bounds,
                )
                .expect("admitted method contract")
            })
            .collect(),
    };
    TraitContract { declaration, abi }
}
