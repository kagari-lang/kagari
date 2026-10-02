use crate::source::types::{lower_native_kind, raise_type};
use kagari_hir::{
    AnalyzedModule,
    aggregates::traits::MethodDefault,
    declarations::DeclarationId,
    hir::item::{
        Item,
        behavior::{GenericParam, Impl},
        function::{Function, FunctionKind},
        storage::Visibility,
    },
    native::NativeBinding,
    resolver::resolved::ResolvedName,
    typeck::{FunctionImplementation, GenericBounds, scalar::ScalarValue, table::ConstraintTarget},
    types::{
        GenericParameterType, TypeId,
        abi::{lower_nominal_type, lower_type},
    },
};

use kagari_abi::{
    callable::{CallableImplementation, NativeDefaultApplication},
    types::{
        AbiType, AssociatedConstAbi, AssociatedTypeAbi, AssociatedTypeFamilyAbi, ConstAbi,
        ConstraintAbi, FieldAbi, FunctionAbi, GenericBoundAbi, GenericParameterAbi,
        InterfaceTableAbi, ModuleAbi, NativeDeclaration, ParameterAbi, PublicAbiItem, TraitAbi,
        TraitContract, TypeAbi, TypeAbiKind, VariantAbi,
    },
};
use kagari_common::identity;

#[cfg(test)]
mod tests;

// Versioned scalar encoding; float bits and UTF-8 byte length are explicit.
fn const_abi_value(value: &ScalarValue) -> String {
    match value {
        ScalarValue::Unit => "const-v1:unit".to_owned(),
        ScalarValue::Bool(value) => format!("const-v1:bool:{}", u8::from(*value)),
        ScalarValue::I32(value) => format!("const-v1:i32:{value}"),
        ScalarValue::Integer { value, ty } => {
            format!("const-v2:{}:{value}", TypeId::Builtin(*ty).display_name())
        }
        ScalarValue::F32(value) => format!("const-v1:f32:{:08x}", value.to_bits()),
        ScalarValue::F64(value) => format!("const-v2:f64:{:016x}", value.to_bits()),
        ScalarValue::String(value) => format!("const-v1:str:{}:{value}", value.len()),
    }
}

pub(crate) fn collect_module_abi(module: &AnalyzedModule) -> ModuleAbi {
    let hir_module = &module.lowered.module;
    let mut public_items = Vec::new();
    let mut trait_contracts = Vec::new();

    for item in &hir_module.items {
        match *item {
            Item::Function(id) => {
                let Some(function) = hir_module.functions.iter().find(|function| {
                    function.id == id
                        && function.visibility == Visibility::Public
                        && function.kind == FunctionKind::User
                }) else {
                    continue;
                };
                if let Some(abi) = function_abi(module, function) {
                    public_items.push(PublicAbiItem::Function(abi));
                }
            }
            Item::Const(id) => {
                let Some(const_item) = hir_module.consts.iter().find(|const_item| {
                    const_item.id == id && const_item.visibility == Visibility::Public
                }) else {
                    continue;
                };
                let ty = module
                    .typed
                    .consts
                    .get(&id)
                    .map(|ty| abi_type(module, ty))
                    .expect("checked const fact must exist");
                let value = module
                    .typed
                    .const_values
                    .get(&id)
                    .map(const_abi_value)
                    .expect("checked const fact must exist");
                public_items.push(PublicAbiItem::Const(ConstAbi {
                    name: const_item.name.clone(),
                    ty,
                    value,
                }));
            }
            Item::Struct(id) => {
                let Some(struct_item) = hir_module.structs.iter().find(|struct_item| {
                    struct_item.id == id && struct_item.visibility == Visibility::Public
                }) else {
                    continue;
                };
                public_items.push(PublicAbiItem::Type(TypeAbi {
                    name: struct_item.name.clone(),
                    kind: TypeAbiKind::Struct,
                    generic_params: generic_param_abi(module, &struct_item.generic_params),
                    bounds: parameter_bounds(module, &struct_item.generic_params),
                    fields: struct_item
                        .fields
                        .iter()
                        .map(|field| FieldAbi {
                            name: field.name.clone(),
                            ty: abi_type(
                                module,
                                &module
                                    .typed
                                    .type_table
                                    .field_type(field.id)
                                    .expect("checked field type must exist"),
                            ),
                            mutable: field.writeability.is_var(),
                        })
                        .collect(),
                    variants: Vec::new(),
                }));
            }
            Item::OpaqueType(id) => {
                let Some(item) = hir_module
                    .opaque_types
                    .iter()
                    .find(|item| item.id == id && item.visibility == Visibility::Public)
                else {
                    continue;
                };
                let representation = module
                    .declarations
                    .native_type(id)
                    .expect("checked native type representation");
                public_items.push(PublicAbiItem::Type(TypeAbi {
                    name: item.name.clone(),
                    kind: lower_native_kind(representation),
                    generic_params: generic_param_abi(module, &item.generic_params),
                    bounds: parameter_bounds(module, &item.generic_params),
                    fields: Vec::new(),
                    variants: Vec::new(),
                }));
            }
            Item::Enum(id) => {
                let Some(enum_item) = hir_module.enums.iter().find(|enum_item| {
                    enum_item.id == id && enum_item.visibility == Visibility::Public
                }) else {
                    continue;
                };
                public_items.push(PublicAbiItem::Type(TypeAbi {
                    name: enum_item.name.clone(),
                    kind: module
                        .declarations
                        .native_enum(id)
                        .map(lower_native_kind)
                        .unwrap_or(TypeAbiKind::Enum),
                    generic_params: generic_param_abi(module, &enum_item.generic_params),
                    bounds: parameter_bounds(module, &enum_item.generic_params),
                    fields: Vec::new(),
                    variants: enum_item
                        .variants
                        .iter()
                        .map(|variant| VariantAbi {
                            name: variant.name.clone(),
                            payload: variant
                                .payload
                                .iter()
                                .map(|ty| {
                                    abi_type(
                                        module,
                                        &module
                                            .typed
                                            .type_table
                                            .type_ref(*ty)
                                            .expect("checked enum payload type must exist")
                                            .ty,
                                    )
                                })
                                .collect(),
                        })
                        .collect(),
                }));
            }
            Item::Trait(id) => {
                let Some(trait_item) = hir_module
                    .traits
                    .iter()
                    .find(|trait_item| trait_item.id == id)
                else {
                    continue;
                };
                let abi = TraitAbi {
                    associated_consts: trait_item
                        .associated_consts
                        .iter()
                        .map(|member| AssociatedConstAbi {
                            declaration: identity::associated_const_id(
                                match &module
                                    .declarations
                                    .target(ResolvedName::Trait(id))
                                    .expect("trait declaration")
                                    .id
                                {
                                    DeclarationId::Definition(owner) => owner,
                                    _ => unreachable!("nominal trait"),
                                },
                                &member.name,
                            ),
                            ty: lower_type(
                                &module
                                    .typed
                                    .type_table
                                    .type_ref(member.ty)
                                    .expect("constant signature")
                                    .ty,
                            ),
                            default_value: member
                                .initializer
                                .and_then(|id| module.typed.const_values.get(&id))
                                .map(const_abi_value),
                        })
                        .collect(),
                    supertraits: trait_item
                        .supertraits
                        .iter()
                        .filter_map(|reference| {
                            let ConstraintTarget::Trait(parent) =
                                module.typed.type_table.constraint(reference.ty)?
                            else {
                                return None;
                            };
                            Some(lower_nominal_type(&parent))
                        })
                        .collect(),
                    associated_types: trait_item
                        .associated_types
                        .iter()
                        .map(|member| {
                            let DeclarationId::Definition(owner) = &module
                                .declarations
                                .target(ResolvedName::Trait(id))
                                .expect("trait identity")
                                .id
                            else {
                                unreachable!("nominal trait")
                            };
                            AssociatedTypeAbi {
                                generic_params: generic_param_abi(module, &member.generic_params),
                                parameter_bounds: module
                                    .typed
                                    .type_table
                                    .associated_type_parameters(&identity::associated_type_id(
                                        owner,
                                        &member.name,
                                    ))
                                    .map(|inputs| checked_bounds(&inputs.bounds))
                                    .unwrap_or_default(),
                                declaration: identity::associated_type_id(owner, &member.name),
                                bounds: {
                                    let mut bounds = member
                                        .bounds
                                        .iter()
                                        .filter_map(|bound| {
                                            module.typed.type_table.constraint(bound.ty)
                                        })
                                        .map(constraint_abi)
                                        .collect::<Vec<_>>();
                                    bounds.sort();
                                    bounds.dedup();
                                    bounds
                                },
                            }
                        })
                        .collect(),
                    name: trait_item.name.clone(),
                    generic_params: generic_param_abi(module, &trait_item.generic_params),
                    bounds: parameter_bounds(module, &trait_item.generic_params),
                    methods: trait_item
                        .methods
                        .iter()
                        .filter_map(|method| {
                            hir_module
                                .functions
                                .iter()
                                .find(|function| function.id == method.function)
                                .and_then(|function| {
                                    method_abi(module, function, &trait_item.generic_params)
                                })
                        })
                        .collect(),
                };
                if trait_item.visibility == Visibility::Public {
                    public_items.push(PublicAbiItem::Trait(abi));
                } else {
                    trait_contracts.push(TraitContract {
                        declaration: match &module
                            .declarations
                            .target(ResolvedName::Trait(id))
                            .expect("checked trait declaration identity")
                            .id
                        {
                            DeclarationId::Definition(id) => id.clone(),
                            _ => unreachable!("nominal trait declaration"),
                        },
                        abi,
                    });
                }
            }
            Item::Module(_) | Item::Impl(_) => {}
        }
    }

    for impl_block in &hir_module.impls {
        let Some(reference) = &impl_block.trait_ref else {
            continue;
        };
        let trait_type = match module
            .typed
            .type_table
            .constraint(reference.ty)
            .expect("checked impl trait")
        {
            ConstraintTarget::Trait(ty) => TypeId::Trait(ty),
            _ => unreachable!("user trait impl"),
        };
        let for_type = &impl_block
            .for_type
            .and_then(|ty| module.typed.type_table.type_ref(ty))
            .expect("checked impl target must exist")
            .ty;
        let name = format!(
            "{} as {}",
            for_type.display_name(),
            trait_type.display_name()
        );
        public_items.push(PublicAbiItem::InterfaceTable(Box::new(InterfaceTableAbi {
            associated_type_families: module
                .aggregates
                .implementation_signature(
                    module
                        .declarations
                        .impl_identity(impl_block.id)
                        .expect("impl identity"),
                )
                .expect("impl signature")
                .associated_type_families
                .iter()
                .map(|(id, family)| AssociatedTypeFamilyAbi {
                    declaration: id.clone(),
                    generic_params: family
                        .inputs
                        .parameters
                        .iter()
                        .map(|param| GenericParameterAbi {
                            owner: param.owner.clone(),
                            position: param.position,
                        })
                        .collect(),
                    bounds: checked_bounds(&family.inputs.bounds),
                    value: abi_type(module, &family.value),
                })
                .collect(),
            associated_consts: impl_block
                .associated_consts
                .iter()
                .map(|member| ConstAbi {
                    name: member.name.clone(),
                    ty: lower_type(
                        &module
                            .typed
                            .type_table
                            .type_ref(member.ty)
                            .expect("constant signature")
                            .ty,
                    ),
                    value: const_abi_value(
                        module
                            .typed
                            .const_values
                            .get(&member.initializer.expect("impl initializer"))
                            .expect("checked constant"),
                    ),
                })
                .collect(),
            host_bridge: false,
            declaration: module
                .declarations
                .impl_identity(impl_block.id)
                .expect("checked impl declaration identity")
                .clone(),
            name,
            generic_params: generic_param_abi(module, &impl_block.generic_params),
            bounds: checked_bounds(
                &module
                    .aggregates
                    .implementation_signature(
                        module
                            .declarations
                            .impl_identity(impl_block.id)
                            .expect("impl identity"),
                    )
                    .expect("checked implementation")
                    .bounds,
            ),
            trait_type: lower_type(&trait_type),
            for_type: lower_type(for_type),
            methods: implementation_methods_abi(module, impl_block),
        })));
    }

    let native_declarations = hir_module
        .functions
        .iter()
        .filter_map(|function| {
            let typed = module
                .typed
                .functions
                .iter()
                .find(|typed| typed.id == function.id)?;
            if !matches!(
                typed.implementation,
                FunctionImplementation::Native(NativeBinding::Entry(_))
            ) {
                return None;
            }
            let DeclarationId::Definition(declaration) = &module
                .declarations
                .target(ResolvedName::Function(function.id))?
                .id
            else {
                return None;
            };
            // Registration owns the executable template, including unreduced
            // projections and their selected requirements. HIR may normalize
            // them for checking, but mixing that view with the original
            // requirements changes the sealed contract used by portable linking.
            if let Some(registered) = module
                .lowered
                .registered_native_declarations()
                .iter()
                .find(|item| &item.declaration == declaration)
            {
                return Some(registered.clone());
            }
            let mut abi = function_abi(module, function)?;
            // Native method applications bind the owner parameters as well as the
            // method parameters; public trait method contracts keep their own scope.
            abi.generic_params = typed
                .generic_params
                .iter()
                .map(|parameter| GenericParameterAbi {
                    owner: parameter.owner.clone(),
                    position: parameter.position,
                })
                .collect();
            Some(NativeDeclaration {
                concrete_result: None,
                callable_requirements: vec![],
                declaration: declaration.clone(),
                function: abi,
            })
        })
        .collect();
    ModuleAbi {
        native_declarations,
        public_items,
        trait_contracts,
    }
}

fn implementation_methods_abi(module: &AnalyzedModule, item: &Impl) -> Vec<FunctionAbi> {
    let mut result = item
        .methods
        .iter()
        .filter_map(|method| {
            module
                .lowered
                .module
                .functions
                .iter()
                .find(|function| function.id == method.function)
                .and_then(|function| method_abi(module, function, &item.generic_params))
        })
        .collect::<Vec<_>>();
    let implementation = module
        .aggregates
        .implementation_signature(
            module
                .declarations
                .impl_identity(item.id)
                .expect("impl identity"),
        )
        .expect("impl signature");
    let defaults = module
        .aggregates
        .implementation_methods(implementation)
        .into_iter()
        .filter_map(|target| {
            module
                .aggregates
                .default_method(&target)
                .map(|(implementation, method)| (target, implementation, method))
        })
        .collect::<Vec<_>>();
    for (target, implementation, method) in defaults {
        let contract = module
            .aggregates
            .trait_(&method.owner)
            .expect("default trait contract");
        let method_params = &method.generic_params[contract.generic_params.len()..];
        let own = method_params.iter().enumerate().map(|(position, param)| {
            (
                param.clone(),
                TypeId::Generic(GenericParameterType {
                    owner: target.clone(),
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
        let normalize = |ty: &TypeId| {
            abi_type(
                module,
                &ty.with_associated_types(&implementation.trait_type)
                    .with_self(&method.owner, &implementation.for_type)
                    .instantiate(&substitution),
            )
        };
        let bounds = method
            .bounds
            .iter()
            .map(|(ty, constraints)| GenericBoundAbi {
                ty: normalize(ty),
                constraints: constraints
                    .iter()
                    .filter(|constraint| {
                        !contract
                            .bounds
                            .get(ty)
                            .is_some_and(|inherited| inherited.contains(constraint))
                    })
                    .map(|constraint| match constraint {
                        ConstraintTarget::Standard(value) => ConstraintAbi::Standard(*value),
                        ConstraintTarget::Trait(ty) => {
                            let AbiType::Trait(ty) = normalize(&TypeId::Trait(ty.clone())) else {
                                unreachable!("trait bound");
                            };
                            ConstraintAbi::Trait(ty)
                        }
                    })
                    .collect(),
            })
            .collect();
        result.push(FunctionAbi {
            method_policy: method.policy,
            name: method.name.clone(),
            implementation: match method.default.clone().expect("selected default method") {
                MethodDefault::Script => CallableImplementation::Script,
                MethodDefault::Native(NativeBinding::Default(application)) => {
                    CallableImplementation::NativeDefault(NativeDefaultApplication {
                        declaration: application.declaration,
                        arguments: application
                            .arguments
                            .iter()
                            .map(|ty| normalize(&raise_type(ty)))
                            .collect(),
                    })
                }
                MethodDefault::Native(binding) => native_implementation_abi(module, binding),
            },
            generic_params: method_params
                .iter()
                .enumerate()
                .map(|(position, _)| GenericParameterAbi {
                    owner: target.clone(),
                    position,
                })
                .collect(),
            bounds: canonical_bounds(bounds),
            params: method
                .params
                .iter()
                .map(|param| ParameterAbi {
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

fn function_abi(module: &AnalyzedModule, function: &Function) -> Option<FunctionAbi> {
    let typed = module
        .typed
        .functions
        .iter()
        .find(|typed| typed.id == function.id)?;
    Some(FunctionAbi {
        method_policy: module
            .declarations
            .target(ResolvedName::Function(function.id))
            .and_then(|declaration| match &declaration.id {
                DeclarationId::Definition(id) => module.aggregates.trait_method(id),
                _ => None,
            })
            .map_or_else(Default::default, |method| method.policy),
        name: typed.name.clone(),
        implementation: match typed.implementation.clone() {
            FunctionImplementation::Required => CallableImplementation::Required,
            FunctionImplementation::Script => CallableImplementation::Script,
            FunctionImplementation::Native(binding) => native_implementation_abi(module, binding),
        },
        generic_params: generic_param_abi(module, &function.generic_params),
        bounds: checked_bounds(&typed.bounds),
        params: typed
            .params
            .iter()
            .map(|param| ParameterAbi {
                name: param.name.clone(),
                ty: abi_type(module, &param.ty),
                mutable: param.writeability.is_var(),
            })
            .collect(),
        return_type: abi_type(module, &typed.return_type),
    })
}

fn native_implementation_abi(
    module: &AnalyzedModule,
    binding: NativeBinding,
) -> CallableImplementation {
    let binding = match binding {
        NativeBinding::Default(application) => {
            return CallableImplementation::NativeDefault(application);
        }
        NativeBinding::Entry(binding) => binding,
        NativeBinding::Host(id) => module
            .names
            .hosts
            .function(id)
            .expect("checked host callable")
            .id
            .clone(),
    };
    CallableImplementation::Native(binding)
}

fn abi_type(module: &AnalyzedModule, ty: &TypeId) -> AbiType {
    lower_type(&module.aggregates.normalize_type(ty))
}

fn method_abi(
    module: &AnalyzedModule,
    function: &Function,
    outer: &[GenericParam],
) -> Option<FunctionAbi> {
    let mut method = function_abi(module, function)?;
    let inherited = generic_param_abi(module, outer);
    method
        .generic_params
        .retain(|parameter| !inherited.contains(parameter));
    let mut guaranteed = parameter_bounds(module, outer);
    if let Some(owner) = inherited.first().map(|p| &p.owner)
        && let Some(implementation) = module
            .aggregates
            .implementations()
            .find(|item| &item.id == owner)
    {
        guaranteed.extend(checked_bounds(&implementation.bounds));
    }
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

fn generic_param_abi(module: &AnalyzedModule, params: &[GenericParam]) -> Vec<GenericParameterAbi> {
    params
        .iter()
        .map(|param| {
            let DeclarationId::GenericParameter { owner, position } = &module
                .declarations
                .generic_parameter(param.id)
                .expect("checked generic declaration")
                .id
            else {
                unreachable!("generic declaration identity")
            };
            GenericParameterAbi {
                owner: owner.clone(),
                position: *position,
            }
        })
        .collect()
}

fn constraint_abi(target: ConstraintTarget) -> ConstraintAbi {
    match target {
        ConstraintTarget::Standard(constraint) => ConstraintAbi::Standard(constraint),
        ConstraintTarget::Trait(ty) => ConstraintAbi::Trait(lower_nominal_type(&ty)),
    }
}

fn parameter_bounds(module: &AnalyzedModule, params: &[GenericParam]) -> Vec<GenericBoundAbi> {
    let identities = generic_param_abi(module, params);
    let bounds = params
        .iter()
        .zip(identities)
        .map(|(param, id)| GenericBoundAbi {
            ty: AbiType::Parameter {
                owner: id.owner,
                position: id.position,
            },
            constraints: param
                .bounds
                .iter()
                .map(|bound| {
                    constraint_abi(
                        module
                            .typed
                            .type_table
                            .constraint(bound.ty)
                            .expect("checked constraint"),
                    )
                })
                .collect(),
        })
        .collect();
    canonical_bounds(bounds)
}

pub(super) fn checked_bounds(bounds: &GenericBounds) -> Vec<GenericBoundAbi> {
    canonical_bounds(
        bounds
            .iter()
            .map(|(param, targets)| GenericBoundAbi {
                ty: lower_type(param),
                constraints: targets.iter().cloned().map(constraint_abi).collect(),
            })
            .collect(),
    )
}

fn canonical_bounds(mut bounds: Vec<GenericBoundAbi>) -> Vec<GenericBoundAbi> {
    bounds.sort_by(|a, b| a.ty.cmp(&b.ty));
    let mut merged: Vec<GenericBoundAbi> = Vec::new();
    for bound in bounds {
        if let Some(previous) = merged.last_mut()
            && previous.ty == bound.ty
        {
            previous.constraints.extend(bound.constraints);
        } else {
            merged.push(bound);
        }
    }
    let mut bounds = merged;
    for bound in &mut bounds {
        bound.constraints.sort();
        bound.constraints.dedup();
    }
    bounds.retain(|bound| !bound.constraints.is_empty());
    bounds.sort_by(|a, b| a.ty.cmp(&b.ty));
    bounds
}
