use std::collections::HashMap;

use kagari_hir::{
    AnalyzedModule,
    hir::{self, ConstId, FunctionKind, Item, Visibility},
    types::TypeId,
};

use crate::{
    lower::EvaluatedConst,
    module::{
        ConstAbi, FieldAbi, FunctionAbi, InterfaceTableAbi, ModuleAbi, ParameterAbi, PublicAbiItem,
        TraitAbi, TypeAbi, TypeAbiKind, VariantAbi,
    },
};

pub(crate) fn collect_module_abi(
    module: &AnalyzedModule,
    const_values: &HashMap<ConstId, EvaluatedConst>,
) -> ModuleAbi {
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
                if let Some(abi) = function_abi(&module.typed.functions, function) {
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
                    .unwrap_or_else(|| "<unknown>".to_owned());
                let value = const_values
                    .get(&id)
                    .map(const_abi_value)
                    .unwrap_or_else(|| "<unknown>".to_owned());
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
                            ty: display_type_ref(hir_module, field.ty),
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
                    generic_params: generic_param_abi(&trait_item.generic_params),
                    methods: trait_item
                        .methods
                        .iter()
                        .filter_map(|method| {
                            hir_module
                                .functions
                                .iter()
                                .find(|function| function.id == method.function)
                                .and_then(|function| {
                                    function_abi(&module.typed.functions, function)
                                })
                        })
                        .collect(),
                }));
            }
            Item::Module(_) | Item::Impl(_) => {}
        }
    }

    for impl_block in &hir_module.impls {
        let Some(trait_name) = &impl_block.trait_ref else {
            continue;
        };
        let for_type = impl_block
            .for_type
            .map(|ty| display_type_ref(hir_module, ty))
            .unwrap_or_else(|| "<missing>".to_owned());
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
                        .and_then(|function| function_abi(&module.typed.functions, function))
                })
                .collect(),
        }));
    }

    ModuleAbi { public_items }
}

fn function_abi(
    typed_functions: &[kagari_hir::typeck::TypedFunction],
    function: &hir::Function,
) -> Option<FunctionAbi> {
    let typed = typed_functions
        .iter()
        .find(|typed| typed.id == function.id)?;
    Some(FunctionAbi {
        name: typed.name.clone(),
        generic_params: generic_param_abi(&function.generic_params),
        bounds: trait_bound_abi(&function.bounds),
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

fn generic_param_abi(params: &[hir::GenericParam]) -> Vec<String> {
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
                        .map(|bound| bound.name.as_str())
                        .collect::<Vec<_>>()
                        .join(" + ")
                )
            }
        })
        .collect()
}

fn trait_bound_abi(bounds: &[hir::TraitBound]) -> Vec<String> {
    bounds
        .iter()
        .map(|bound| {
            format!(
                "{}: {}",
                bound.target,
                bound
                    .traits
                    .iter()
                    .map(|trait_ref| trait_ref.name.as_str())
                    .collect::<Vec<_>>()
                    .join(" + ")
            )
        })
        .collect()
}

fn display_type_ref(module: &hir::Module, ty: hir::TypeRefId) -> String {
    match &module.type_ref(ty).kind {
        hir::TypeKind::Named(name) => name.clone(),
        hir::TypeKind::Generic { name, args } => {
            let args = args
                .iter()
                .map(|arg| display_type_ref(module, *arg))
                .collect::<Vec<_>>()
                .join(", ");
            format!("{name}<{args}>")
        }
        hir::TypeKind::Tuple(elements) => {
            let elements = elements
                .iter()
                .map(|element| display_type_ref(module, *element))
                .collect::<Vec<_>>()
                .join(", ");
            format!("({elements})")
        }
        hir::TypeKind::Array(element) => format!("[{}]", display_type_ref(module, *element)),
    }
}

fn const_abi_value(value: &EvaluatedConst) -> String {
    match value {
        EvaluatedConst::Scalar(constant) => format!("{constant:?}"),
        EvaluatedConst::Tuple(elements) => format!(
            "({})",
            elements
                .iter()
                .map(const_abi_value)
                .collect::<Vec<_>>()
                .join(", ")
        ),
        EvaluatedConst::Array(elements) => format!(
            "[{}]",
            elements
                .iter()
                .map(const_abi_value)
                .collect::<Vec<_>>()
                .join(", ")
        ),
        EvaluatedConst::Struct { name, fields } => format!(
            "{name} {{ {} }}",
            fields
                .iter()
                .map(|field| format!("{}: {}", field.name, const_abi_value(&field.value)))
                .collect::<Vec<_>>()
                .join(", ")
        ),
    }
}
