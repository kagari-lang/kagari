//! Trait declaration order paired with canonical checked member contracts.

use super::{abi_parameters, checked_bounds, const_abi_value, constraint_abi, method_abi};
use kagari_common::identity::DefinitionKind;
use kagari_contract::types::TraitContract;
use kagari_hir::{
    aggregates::traits::{MethodDefault, MethodSignature},
    compile_input::program::CheckedModule,
    hir::item::behavior::TraitDef as HirTraitDef,
    resolver::resolved::ResolvedName,
    types::semantic::lower_type,
};
use kagari_types::{
    callable::CallableImplementation,
    declaration::{AssociatedConstDef, AssociatedTypeDef, FnDecl, Param, TraitDef},
    ty::NominalTy,
};

/// A trait header retains its own associated projections. Body normalization may
/// select a blanket output; that describes an application, not the declared ABI.
pub(super) fn declared_method(module: &CheckedModule<'_>, method: &MethodSignature) -> FnDecl {
    let environment = module.environment();
    let catalog = environment.aggregates();
    let definitions = environment.definitions();
    let value_type = |ty| lower_type(&catalog.canonical_value_type(definitions, ty));
    let (implementation, host_policy) = match &method.default {
        None => (CallableImplementation::Required, None),
        Some(MethodDefault::Script) => (CallableImplementation::Script, None),
        Some(MethodDefault::Native(contract)) => (
            CallableImplementation::Native(method.id),
            contract.host_policy.clone(),
        ),
    };
    FnDecl {
        name: method.name.clone(),
        implementation,
        host_policy,
        method_policy: method.policy,
        generic_params: abi_parameters(&method.generic_params),
        bounds: checked_bounds(catalog, definitions, &method.bounds),
        params: method
            .params
            .iter()
            .map(|param| Param {
                name: param.name.clone(),
                mutable: param.writeability.is_var(),
                ty: value_type(&param.ty),
            })
            .collect(),
        return_type: value_type(&method.return_type),
    }
}

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
