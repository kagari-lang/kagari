use kagari_hir::{
    AnalyzedModule,
    hir::{self, FunctionKind, Item, Visibility},
    types::TypeId,
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
                    .map(TypeId::display_name)
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
                    fields: struct_item
                        .fields
                        .iter()
                        .map(|field| FieldAbi {
                            name: field.name.clone(),
                            ty: module
                                .typed
                                .type_table
                                .field_type(field.id)
                                .expect("checked field type must exist")
                                .display_name(),
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
        let trait_name = module
            .typed
            .type_table
            .type_ref(reference.ty)
            .expect("checked impl trait reference")
            .ty
            .display_name();
        let for_type = impl_block
            .for_type
            .and_then(|ty| module.typed.type_table.type_ref(ty))
            .expect("checked impl target must exist")
            .ty
            .display_name();
        let name = format!("{for_type} as {trait_name}");
        public_items.push(PublicAbiItem::InterfaceTable(InterfaceTableAbi {
            name,
            trait_name: trait_name.clone(),
            for_type,
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
        bounds: trait_bound_abi(module, &function.bounds),
        params: typed
            .params
            .iter()
            .map(|param| ParameterAbi {
                name: param.name.clone(),
                ty: param.ty.display_name(),
                mutable: param.writeability.is_var(),
            })
            .collect(),
        return_type: typed.return_type.display_name(),
    })
}

fn generic_param_abi(module: &AnalyzedModule, params: &[hir::GenericParam]) -> Vec<String> {
    params
        .iter()
        .map(|param| {
            if param.bounds.is_empty() {
                param.name.clone()
            } else {
                format!(
                    "{}: {}",
                    param.name,
                    param
                        .bounds
                        .iter()
                        .map(|bound| constraint_name(module, bound))
                        .collect::<Vec<_>>()
                        .join(" + ")
                )
            }
        })
        .collect()
}

fn constraint_name<'a>(module: &'a AnalyzedModule, reference: &hir::TraitRef) -> &'a str {
    match module
        .typed
        .type_table
        .constraint(reference.ty)
        .expect("checked constraint must exist")
    {
        kagari_hir::typeck::ConstraintTarget::Standard(constraint) => {
            kagari_hir::builtin::surface::standard_constraint_name(constraint)
        }
        kagari_hir::typeck::ConstraintTarget::Trait(id) => {
            &module
                .lowered
                .module
                .traits
                .iter()
                .find(|item| item.id == id)
                .expect("checked trait must exist")
                .name
        }
    }
}

fn trait_bound_abi(module: &AnalyzedModule, bounds: &[hir::TraitBound]) -> Vec<String> {
    bounds
        .iter()
        .map(|bound| {
            format!(
                "{}: {}",
                module
                    .typed
                    .type_table
                    .type_ref(bound.target_ref)
                    .expect("checked bound target must exist")
                    .ty
                    .display_name(),
                bound
                    .traits
                    .iter()
                    .map(|trait_ref| constraint_name(module, trait_ref))
                    .collect::<Vec<_>>()
                    .join(" + ")
            )
        })
        .collect()
}
