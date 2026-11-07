//! Inventory authored declarations before instance reachability removes templates.

use super::{
    abi_parameters, abi_type, checked_bounds, const_abi_value, constraint_abi, function_abi,
};
use kagari_contract::types::named::{
    AliasContract, ImplementationContract, NamedContract, NamedDeclaration,
};
use kagari_hir::{
    compile_input::program::CheckedModule, hir::item::Item, resolver::resolved::ResolvedName,
};
use kagari_types::{declaration::ConstDef, ty::Ty, visibility::Visibility};

pub(super) fn collect(module: &CheckedModule<'_>) -> Vec<NamedDeclaration> {
    let headers = module.headers();
    let environment = module.environment();
    let definitions = environment.definitions();
    let catalog = environment.aggregates();
    let declarations = environment.declarations();
    let definition = |name| {
        environment
            .definition(name)
            .expect("admitted authored declaration")
    };
    let mut records = Vec::new();
    for function in headers.functions {
        records.push(NamedDeclaration {
            declaration: definition(ResolvedName::Function(function.id)),
            visibility: function.visibility,
            ambiguous_local_body: declarations
                .ambiguous_local_owner(ResolvedName::Function(function.id))
                .map(|owner| {
                    let item = match declarations
                        .definition_target(&owner)
                        .expect("authored local owner")
                    {
                        ResolvedName::Function(id) => Item::Function(id),
                        ResolvedName::Const(id) => Item::Const(id),
                        ResolvedName::Static(id) => Item::Static(id),
                        _ => unreachable!("local callable owner"),
                    };
                    let span = module
                        .declaration_span(item)
                        .expect("admitted local owner span");
                    module.source().text()[span.start..span.end].to_owned()
                }),
            contract: NamedContract::Function(
                function_abi(module, function).expect("admitted function contract"),
            ),
        });
    }
    for item in headers.constants {
        let constant = environment
            .local_constant(item.id)
            .expect("admitted constant");
        records.push(NamedDeclaration {
            declaration: definition(ResolvedName::Const(item.id)),
            visibility: item.visibility,
            ambiguous_local_body: None,
            contract: NamedContract::Constant(ConstDef {
                name: item.name.clone(),
                ty: abi_type(module, constant.ty),
                value: const_abi_value(constant.value, definitions),
            }),
        });
    }
    for alias in headers.aliases {
        let declaration = definition(ResolvedName::TypeAlias(alias.id));
        let template = declarations
            .imported_types()
            .alias(&declaration)
            .expect("admitted normalized alias");
        records.push(NamedDeclaration {
            declaration,
            visibility: alias.visibility,
            ambiguous_local_body: None,
            contract: NamedContract::Alias(AliasContract {
                name: alias.name.clone(),
                generic_params: abi_parameters(&template.parameters),
                bounds: checked_bounds(catalog, definitions, &template.bounds),
                target: abi_type(module, &template.target),
            }),
        });
    }
    for header in headers.implementations {
        let implementation = module
            .implementation(header.id)
            .expect("admitted implementation");
        records.push(NamedDeclaration {
            declaration: implementation.declaration,
            visibility: Visibility::Private,
            ambiguous_local_body: None,
            contract: NamedContract::Implementation(ImplementationContract {
                bridge: None,
                generic_params: abi_parameters(&implementation.generic_params),
                bounds: checked_bounds(catalog, definitions, &implementation.bounds),
                receiver: abi_type(module, implementation.receiver),
                trait_type: implementation.interface.map(|interface| {
                    Ty::Trait(constraint_abi(catalog, definitions, interface.clone()))
                }),
            }),
        });
    }
    records
}
