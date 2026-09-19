use kagari_hir::{
    AnalyzedModule,
    hir::{self, FunctionKind, Item, Visibility},
};

// Versioned scalar encoding; float bits and UTF-8 byte length are explicit.
fn const_abi_value(value: &kagari_hir::typeck::ScalarValue) -> String {
    use kagari_hir::typeck::ScalarValue;
    match value {
        ScalarValue::Unit => "const-v1:unit".to_owned(),
        ScalarValue::Bool(value) => format!("const-v1:bool:{}", u8::from(*value)),
        ScalarValue::I32(value) => format!("const-v1:i32:{value}"),
        ScalarValue::F32(value) => format!("const-v1:f32:{:08x}", value.to_bits()),
        ScalarValue::String(value) => format!("const-v1:str:{}:{value}", value.len()),
    }
}

use crate::module::abi::{AbiType, ConstraintAbi, GenericBoundAbi, GenericParameterAbi};
use crate::module::{
    ConstAbi, FieldAbi, FunctionAbi, InterfaceTableAbi, ModuleAbi, ParameterAbi, PublicAbiItem,
    TraitAbi, TypeAbi, TypeAbiKind, VariantAbi,
};

pub(crate) fn collect_module_abi(module: &AnalyzedModule) -> ModuleAbi {
    let hir_module = &module.lowered.module;
    let mut public_items = Vec::new();

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
                    .map(AbiType::from_checked_type)
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
                            ty: AbiType::from_checked_type(
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
            Item::Enum(id) => {
                let Some(enum_item) = hir_module.enums.iter().find(|enum_item| {
                    enum_item.id == id && enum_item.visibility == Visibility::Public
                }) else {
                    continue;
                };
                public_items.push(PublicAbiItem::Type(TypeAbi {
                    name: enum_item.name.clone(),
                    kind: TypeAbiKind::Enum,
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
                                    crate::module::abi::AbiType::from_checked_type(
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
                let Some(trait_item) = hir_module.traits.iter().find(|trait_item| {
                    trait_item.id == id && trait_item.visibility == Visibility::Public
                }) else {
                    continue;
                };
                public_items.push(PublicAbiItem::Trait(TraitAbi {
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
                                .and_then(|function| function_abi(module, function))
                        })
                        .collect(),
                }));
            }
            Item::Module(_) | Item::Impl(_) => {}
        }
    }

    for impl_block in &hir_module.impls {
        let Some(reference) = &impl_block.trait_ref else {
            continue;
        };
        let trait_type = &module
            .typed
            .type_table
            .type_ref(reference.ty)
            .expect("checked impl trait reference")
            .ty;
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
        public_items.push(PublicAbiItem::InterfaceTable(InterfaceTableAbi {
            name,
            generic_params: generic_param_abi(module, &impl_block.generic_params),
            bounds: canonical_bounds(
                parameter_bounds(module, &impl_block.generic_params)
                    .into_iter()
                    .chain(impl_block.bounds.iter().map(|bound| {
                        let kagari_hir::types::TypeId::Generic(parameter) = &module
                            .typed
                            .type_table
                            .type_ref(bound.target_ref)
                            .expect("checked impl bound target")
                            .ty
                        else {
                            unreachable!("checked generic bound target")
                        };
                        GenericBoundAbi {
                            owner: parameter.owner.clone(),
                            position: parameter.position,
                            constraints: bound
                                .traits
                                .iter()
                                .map(|reference| {
                                    constraint_abi(
                                        module
                                            .typed
                                            .type_table
                                            .constraint(reference.ty)
                                            .expect("checked impl bound"),
                                    )
                                })
                                .collect(),
                        }
                    }))
                    .collect(),
            ),
            trait_type: AbiType::from_checked_type(trait_type),
            for_type: AbiType::from_checked_type(for_type),
            methods: impl_block
                .methods
                .iter()
                .filter_map(|method| {
                    hir_module
                        .functions
                        .iter()
                        .find(|function| function.id == method.function)
                        .and_then(|function| function_abi(module, function))
                })
                .collect(),
        }));
    }

    ModuleAbi { public_items }
}

fn function_abi(module: &AnalyzedModule, function: &hir::Function) -> Option<FunctionAbi> {
    let typed = module
        .typed
        .functions
        .iter()
        .find(|typed| typed.id == function.id)?;
    Some(FunctionAbi {
        name: typed.name.clone(),
        generic_params: generic_param_abi(module, &function.generic_params),
        bounds: checked_bounds(&typed.bounds),
        params: typed
            .params
            .iter()
            .map(|param| ParameterAbi {
                name: param.name.clone(),
                ty: AbiType::from_checked_type(&param.ty),
                mutable: param.writeability.is_var(),
            })
            .collect(),
        return_type: AbiType::from_checked_type(&typed.return_type),
    })
}

fn generic_param_abi(
    module: &AnalyzedModule,
    params: &[hir::GenericParam],
) -> Vec<GenericParameterAbi> {
    params
        .iter()
        .map(|param| {
            let kagari_hir::declarations::DeclarationId::GenericParameter { owner, position } =
                &module
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

fn constraint_abi(target: kagari_hir::typeck::ConstraintTarget) -> ConstraintAbi {
    match target {
        kagari_hir::typeck::ConstraintTarget::Standard(constraint) => {
            ConstraintAbi::Standard(constraint)
        }
        kagari_hir::typeck::ConstraintTarget::Trait(id) => ConstraintAbi::Trait(id),
    }
}

fn parameter_bounds(module: &AnalyzedModule, params: &[hir::GenericParam]) -> Vec<GenericBoundAbi> {
    let identities = generic_param_abi(module, params);
    let bounds = params
        .iter()
        .zip(identities)
        .map(|(param, id)| GenericBoundAbi {
            owner: id.owner,
            position: id.position,
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

fn checked_bounds(bounds: &kagari_hir::typeck::GenericBounds) -> Vec<GenericBoundAbi> {
    canonical_bounds(
        bounds
            .iter()
            .map(|(param, targets)| GenericBoundAbi {
                owner: param.owner.clone(),
                position: param.position,
                constraints: targets.iter().cloned().map(constraint_abi).collect(),
            })
            .collect(),
    )
}

fn canonical_bounds(mut bounds: Vec<GenericBoundAbi>) -> Vec<GenericBoundAbi> {
    bounds.sort_by(|a, b| (&a.owner, a.position).cmp(&(&b.owner, b.position)));
    let mut merged: Vec<GenericBoundAbi> = Vec::new();
    for bound in bounds {
        if let Some(previous) = merged.last_mut()
            && previous.owner == bound.owner
            && previous.position == bound.position
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
    bounds.sort_by(|a, b| (&a.owner, a.position).cmp(&(&b.owner, b.position)));
    bounds
}
