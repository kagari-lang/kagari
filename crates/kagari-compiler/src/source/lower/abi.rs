//! Source-independent declaration products from admitted semantic contracts.

#[cfg(test)]
mod host_tests;
#[cfg(test)]
mod tests;

mod named;
mod traits;
mod type_definitions;

use crate::source::lower::statics::static_contract;
use kagari_common::identity::table::DefinitionTable;
use kagari_contract::types::{
    InterfaceTable, ModuleContract, PublicItem, inherent::InherentTable, materialization,
    named::FunctionOrigin,
};
use kagari_hir::{
    aggregates::{AggregateCatalog, traits::MethodDefault},
    compile_input::program::CheckedModule,
    hir::item::{
        Item,
        behavior::Impl,
        function::{Function, FunctionKind},
    },
    resolver::resolved::ResolvedName,
    semantic::signatures::FunctionImplementation,
    typeck::{GenericBounds, scalar::ScalarValue},
    types::{
        GenericParameterType, NominalType, TypeId,
        semantic::{lower_nominal_type, lower_type},
    },
};
use kagari_types::{
    callable::{CallableImplementation, MethodPolicy},
    declaration::{AssociatedTypeFamily, ConstDef, FnDecl, NativeDeclaration, Param},
    scalar::IntegerType,
    ty::{GenericBound, GenericParam, NominalTy, Ty},
    visibility::Visibility,
};

// Versioned scalar encoding; float bits and UTF-8 byte length are explicit.
fn const_abi_value(value: &ScalarValue, definitions: &DefinitionTable) -> String {
    match value {
        ScalarValue::Unit => "const-v1:unit".to_owned(),
        ScalarValue::Bool(value) => format!("const-v1:bool:{}", u8::from(*value)),
        ScalarValue::Integer {
            value,
            ty: IntegerType::I32,
        } => format!("const-v1:i32:{value}"),
        ScalarValue::Integer { value, ty } => {
            format!(
                "const-v2:{}:{value}",
                TypeId::Builtin(ty.builtin_type()).display_name(definitions)
            )
        }
        ScalarValue::F32(value) => format!("const-v1:f32:{:08x}", value.to_bits()),
        ScalarValue::F64(value) => format!("const-v2:f64:{:016x}", value.to_bits()),
        ScalarValue::String(value) => format!("const-v1:str:{}:{value}", value.len()),
        ScalarValue::Char(value) => format!("const-v1:char:{:08x}", u32::from(*value)),
    }
}

pub(crate) fn collect_module_abi(module: &CheckedModule<'_>) -> ModuleContract {
    let headers = module.headers();
    let environment = module.environment();
    let definitions = environment.definitions();
    let catalog = environment.aggregates();
    let mut public_items = Vec::new();
    let mut private_types = Vec::new();
    let mut trait_contracts = Vec::new();
    for item in headers.items {
        match *item {
            Item::Function(id) => {
                let function = &headers.functions[id.index()];
                if function.visibility == Visibility::Public && function.kind == FunctionKind::User
                {
                    public_items.push(PublicItem::Function(
                        function_abi(module, function).expect("admitted function"),
                    ));
                }
            }
            Item::Const(id) => {
                let item = &headers.constants[id.index()];
                if item.visibility == Visibility::Public {
                    let constant = environment.local_constant(id).expect("admitted constant");
                    public_items.push(PublicItem::Const(ConstDef {
                        name: item.name.clone(),
                        ty: abi_type(module, constant.ty),
                        value: const_abi_value(constant.value, definitions),
                    }));
                }
            }
            Item::Struct(_) | Item::Enum(_) | Item::NativeType(_) => {
                let (visibility, declaration) = type_definitions::lower(module, *item);
                if visibility == Visibility::Public {
                    public_items.push(PublicItem::Type(declaration.definition));
                } else {
                    private_types.push(declaration);
                }
            }
            Item::Trait(id) => {
                let item = &headers.traits[id.index()];
                let contract = traits::lower(module, item);
                if item.visibility == Visibility::Public {
                    public_items.push(PublicItem::Trait(contract.abi));
                } else {
                    trait_contracts.push(contract);
                }
            }
            Item::Static(id) => {
                if headers.statics[id.index()].visibility == Visibility::Public {
                    public_items.push(PublicItem::Static(static_contract(module, id)));
                }
            }
            Item::Module(_) | Item::Impl(_) | Item::TypeAlias(_) => {}
        }
    }
    for header in headers.implementations {
        let input = module
            .implementation(header.id)
            .expect("admitted implementation");
        let Some(_) = input.interface else {
            let methods = header
                .methods
                .iter()
                .filter_map(|method| {
                    let function = &headers.functions[method.function.index()];
                    (function.visibility == Visibility::Public)
                        .then(|| {
                            inherent_method_abi(
                                module,
                                function,
                                &input.generic_params,
                                &input.bounds,
                            )
                        })
                        .flatten()
                })
                .collect::<Vec<_>>();
            if !methods.is_empty() {
                public_items.push(PublicItem::InherentTable(Box::new(InherentTable {
                    declaration: input.declaration,
                    name: input.receiver.display_name(definitions),
                    generic_params: abi_parameters(&input.generic_params),
                    bounds: checked_bounds(catalog, definitions, &input.bounds),
                    for_type: abi_type(module, input.receiver),
                    methods,
                })));
            }
            continue;
        };
        public_items.push(PublicItem::InterfaceTable(Box::new(implementation_table(
            module, header,
        ))));
    }

    let native_declarations = native_declarations(module).collect();
    ModuleContract {
        named_declarations: named::collect(module),
        function_origins: Vec::new(),
        trait_references: Vec::new(),
        native_declarations,
        public_items,
        private_types,
        trait_contracts,
    }
}

fn implementation_table(module: &CheckedModule<'_>, header: &Impl) -> InterfaceTable {
    let environment = module.environment();
    let definitions = environment.definitions();
    let catalog = environment.aggregates();
    let input = module
        .implementation(header.id)
        .expect("checked implementation");
    let interface = input.interface.expect("checked trait implementation");
    let signature = catalog
        .implementation_signature(&input.declaration)
        .expect("admitted trait implementation");
    let trait_type = TypeId::Trait(interface.clone());
    InterfaceTable {
        method_origins: implementation_origins(module, header),
        associated_type_families: signature
            .associated_type_families
            .iter()
            .map(|(id, family)| AssociatedTypeFamily {
                declaration: *id,
                generic_params: abi_parameters(&family.inputs.parameters),
                bounds: checked_bounds(catalog, definitions, &family.inputs.bounds),
                value: abi_type(module, &family.value),
            })
            .collect(),
        associated_consts: header
            .associated_consts
            .iter()
            .map(|member| {
                let constant = environment
                    .local_constant(member.initializer.expect("impl initializer"))
                    .expect("admitted constant");
                ConstDef {
                    name: member.name.clone(),
                    ty: lower_type(constant.ty),
                    value: const_abi_value(constant.value, definitions),
                }
            })
            .collect(),
        bridge: None,
        declaration: input.declaration,
        name: format!(
            "{} as {}",
            input.receiver.display_name(definitions),
            trait_type.display_name(definitions)
        ),
        generic_params: abi_parameters(&input.generic_params),
        bounds: checked_bounds(catalog, definitions, &input.bounds),
        trait_type: lower_type(
            &trait_type.map_children(|ty| catalog.canonical_value_type(definitions, ty)),
        ),
        for_type: abi_type(module, input.receiver),
        methods: implementation_methods_abi(module, header),
    }
}

fn implementation_methods_abi(module: &CheckedModule<'_>, item: &Impl) -> Vec<FnDecl> {
    let owner = module
        .implementation(item.id)
        .expect("admitted implementation");
    let mut result = item
        .methods
        .iter()
        .filter_map(|method| {
            module
                .headers()
                .functions
                .iter()
                .find(|function| function.id == method.function)
                .and_then(|function| {
                    method_abi(module, function, &owner.generic_params, &owner.bounds)
                })
        })
        .collect::<Vec<_>>();
    let implementation = module
        .environment()
        .aggregates()
        .implementation_signature(
            module
                .environment()
                .declarations()
                .impl_identity(item.id)
                .expect("impl identity"),
        )
        .expect("impl signature");
    let defaults = module
        .environment()
        .aggregates()
        .implementation_methods(
            module.environment().declarations().definitions(),
            implementation,
        )
        .into_iter()
        .filter_map(|target| {
            module
                .environment()
                .aggregates()
                .default_method(module.environment().declarations().definitions(), &target)
                .map(|(implementation, method)| (target, implementation, method))
        })
        .collect::<Vec<_>>();
    for (target, implementation, method) in defaults {
        let contract = module
            .environment()
            .aggregates()
            .trait_(&method.owner)
            .expect("default trait contract");
        let method_params = &method.generic_params[contract.generic_params.len()..];
        let own = method_params.iter().enumerate().map(|(position, param)| {
            (
                param.clone(),
                TypeId::Generic(GenericParameterType {
                    owner: target,
                    position,
                    name: param.name.clone(),
                }),
            )
        });
        let substitution = contract
            .generic_params
            .iter()
            .cloned()
            .zip(implementation.trait_type.arguments.iter().cloned())
            .chain(own)
            .collect();
        let apply = |ty: &TypeId| {
            ty.with_associated_types(&implementation.trait_type, &TypeId::SelfType(method.owner))
                .with_self(&method.owner, &implementation.for_type)
                .instantiate(&substitution)
        };
        let normalize = |ty: &TypeId| abi_type(module, &apply(ty));
        let bounds = method
            .bounds
            .iter()
            .map(|(ty, constraints)| GenericBound {
                ty: normalize(ty),
                constraints: constraints
                    .iter()
                    .filter(|constraint| {
                        !contract
                            .bounds
                            .get(ty)
                            .is_some_and(|inherited| inherited.contains(constraint))
                    })
                    .map(|constraint| {
                        let TypeId::Trait(ty) = module.environment().aggregates().normalize_type(
                            module.environment().declarations().definitions(),
                            &apply(&TypeId::Trait(constraint.clone())),
                        ) else {
                            unreachable!("trait bound");
                        };
                        constraint_abi(
                            module.environment().aggregates(),
                            module.environment().declarations().definitions(),
                            ty,
                        )
                    })
                    .collect(),
            })
            .collect();
        result.push(FnDecl {
            host_policy: match &method.default {
                Some(MethodDefault::Native(contract)) => contract.host_policy.clone(),
                _ => None,
            },
            method_policy: MethodPolicy {
                override_allowed: true,
                ..method.policy
            },
            name: method.name.clone(),
            implementation: match method.default.clone().expect("selected default method") {
                MethodDefault::Script => CallableImplementation::Script,
                MethodDefault::Native(_) => CallableImplementation::Native(method.id),
            },
            generic_params: method_params
                .iter()
                .enumerate()
                .map(|(position, _)| GenericParam {
                    owner: target,
                    position,
                })
                .collect(),
            bounds: canonical_bounds(module.environment().declarations().definitions(), bounds),
            params: method
                .params
                .iter()
                .map(|param| Param {
                    name: param.name.clone(),
                    ty: normalize(&param.ty),
                    mutable: param.writeability.is_var(),
                })
                .collect(),
            return_type: normalize(&method.return_type),
        });
    }
    result
}

fn function_abi(module: &CheckedModule<'_>, function: &Function) -> Option<FnDecl> {
    let environment = module.environment();
    let typed = environment.signatures().function(function.id)?;
    let declaration = environment.definition(ResolvedName::Function(function.id))?;
    Some(FnDecl {
        host_policy: match &typed.implementation {
            FunctionImplementation::Native(contract) => contract.host_policy.clone(),
            _ => None,
        },
        method_policy: environment
            .aggregates()
            .trait_method(&declaration)
            .map_or_else(
                || MethodPolicy {
                    receiver_access: match &typed.implementation {
                        FunctionImplementation::Native(contract) => contract.receiver_access,
                        _ => None,
                    },
                    ..Default::default()
                },
                |method| method.policy,
            ),
        name: typed.name.clone(),
        implementation: match typed.implementation.clone() {
            FunctionImplementation::Required => CallableImplementation::Required,
            FunctionImplementation::Script => CallableImplementation::Script,
            FunctionImplementation::Native(_) => CallableImplementation::Native(declaration),
        },
        generic_params: abi_parameters(&typed.generic_params),
        bounds: checked_bounds(
            environment.aggregates(),
            environment.definitions(),
            &typed.bounds,
        ),
        params: typed
            .params
            .iter()
            .map(|param| Param {
                name: param.name.clone(),
                ty: abi_type(module, &param.ty),
                mutable: param.writeability.is_var(),
            })
            .collect(),
        return_type: abi_type(module, &typed.return_type),
    })
}

/// Executable templates are projections of checked declarations, never installed records.
fn declared_native_functions<'a>(
    module: &'a CheckedModule<'_>,
) -> impl Iterator<Item = NativeDeclaration> + 'a {
    module.headers().functions.iter().filter_map(|function| {
        let typed = module.environment().signatures().function(function.id)?;
        let FunctionImplementation::Native(contract) = &typed.implementation else {
            return None;
        };
        let declaration = module
            .environment()
            .definition(ResolvedName::Function(function.id))?;
        let mut function = function_abi(module, function)?;
        function.method_policy.receiver_access = contract.receiver_access;
        Some(NativeDeclaration {
            declaration,
            function,
            concrete_result: contract.concrete_result.clone(),
            callable_requirements: contract.callable_requirements.clone(),
        })
    })
}

/// Retain ordinary applied Native declarations beside inherited Script materializations.
pub(crate) fn native_declarations<'a>(
    module: &'a CheckedModule<'_>,
) -> impl Iterator<Item = NativeDeclaration> + 'a {
    let mut result = declared_native_functions(module).collect::<Vec<_>>();
    let environment = module.environment();
    for header in module.headers().implementations {
        let implementation = module.implementation(header.id).expect("checked impl");
        let Some(_) = implementation.interface else {
            continue;
        };
        let table = implementation_table(module, header);
        for origin in &table.method_origins {
            let method = environment
                .aggregates()
                .trait_method(&origin.authored)
                .expect("checked method");
            let Some(MethodDefault::Native(contract)) = &method.default else {
                continue;
            };
            let source = NativeDeclaration {
                declaration: method.id,
                function: FnDecl {
                    name: method.name.clone(),
                    implementation: CallableImplementation::Native(method.id),
                    host_policy: contract.host_policy.clone(),
                    method_policy: method.policy,
                    generic_params: abi_parameters(&method.generic_params),
                    bounds: checked_bounds(
                        environment.aggregates(),
                        environment.definitions(),
                        &method.bounds,
                    ),
                    params: method
                        .params
                        .iter()
                        .map(|param| Param {
                            name: param.name.clone(),
                            mutable: param.writeability.is_var(),
                            ty: abi_type(module, &param.ty),
                        })
                        .collect(),
                    return_type: abi_type(module, &method.return_type),
                },
                concrete_result: contract.concrete_result.clone(),
                callable_requirements: contract.callable_requirements.clone(),
            };
            result.push(
                materialization::native(
                    &source,
                    &table,
                    origin,
                    environment.definitions(),
                    &Default::default(),
                )
                .expect("checked Native materialization"),
            );
        }
    }
    result.into_iter()
}

fn implementation_origins(module: &CheckedModule<'_>, header: &Impl) -> Vec<FunctionOrigin> {
    let environment = module.environment();
    let signature = environment
        .aggregates()
        .implementation_signature(
            environment
                .declarations()
                .impl_identity(header.id)
                .expect("checked impl identity"),
        )
        .expect("checked impl");
    environment
        .aggregates()
        .implementation_methods(environment.definitions(), signature)
        .into_iter()
        .filter_map(|placement| {
            environment
                .aggregates()
                .default_method(environment.definitions(), &placement)
                .map(|(_, method)| FunctionOrigin {
                    placement,
                    authored: method.id,
                })
        })
        .collect()
}

fn abi_type(module: &CheckedModule<'_>, ty: &TypeId) -> Ty {
    let environment = module.environment();
    lower_type(
        &environment.aggregates().canonical_value_type(
            environment.definitions(),
            &environment
                .aggregates()
                .normalize_type(environment.definitions(), ty),
        ),
    )
}

fn inherent_method_abi(
    module: &CheckedModule<'_>,
    function: &Function,
    outer: &[GenericParameterType],
    bounds: &GenericBounds,
) -> Option<FnDecl> {
    method_abi(module, function, outer, bounds)
}

fn method_abi(
    module: &CheckedModule<'_>,
    function: &Function,
    outer: &[GenericParameterType],
    bounds: &GenericBounds,
) -> Option<FnDecl> {
    let mut method = function_abi(module, function)?;
    let inherited = abi_parameters(outer);
    method
        .generic_params
        .retain(|parameter| !inherited.contains(parameter));
    let environment = module.environment();
    let guaranteed = checked_bounds(environment.aggregates(), environment.definitions(), bounds);
    for bound in &mut method.bounds {
        bound.constraints.retain(|constraint| {
            !guaranteed
                .iter()
                .any(|outer| outer.ty == bound.ty && outer.constraints.contains(constraint))
        });
    }
    method.bounds.retain(|bound| !bound.constraints.is_empty());
    Some(method)
}

fn abi_parameters(params: &[GenericParameterType]) -> Vec<GenericParam> {
    params
        .iter()
        .map(|param| GenericParam {
            owner: param.owner,
            position: param.position,
        })
        .collect()
}

fn constraint_abi(
    catalog: &AggregateCatalog,
    definitions: &DefinitionTable,
    target: NominalType,
) -> NominalTy {
    lower_nominal_type(&catalog.canonical_interface_application(definitions, &target))
}

pub(super) fn checked_bounds(
    catalog: &AggregateCatalog,
    definitions: &DefinitionTable,
    bounds: &GenericBounds,
) -> Vec<GenericBound> {
    canonical_bounds(
        definitions,
        bounds
            .iter()
            .map(|(param, targets)| GenericBound {
                ty: lower_type(&catalog.canonical_value_type(definitions, param)),
                constraints: targets
                    .iter()
                    .cloned()
                    .map(|target| constraint_abi(catalog, definitions, target))
                    .collect(),
            })
            .collect(),
    )
}

fn canonical_bounds(definitions: &DefinitionTable, bounds: Vec<GenericBound>) -> Vec<GenericBound> {
    GenericBound::normalize(bounds, definitions, &Default::default())
        .expect("checked ABI bound identities and type limits")
        .into_iter()
        .filter(|bound| !bound.constraints.is_empty())
        .collect()
}
