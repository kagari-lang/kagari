//! Closed element contracts are prepared once, independently of collection length.
use crate::{error::RuntimeError, module::LoadedModule};
use kagari_abi::{
    layout::{EnumLayout, StructLayout},
    types::AbiType,
};
use std::collections::HashSet;

#[derive(Debug)]
pub(crate) struct StorageType {
    pub(crate) ty: AbiType,
    pub(crate) owner: LoadedModule,
    structures: Vec<StructLayout>,
    enumerations: Vec<EnumLayout>,
}
impl StorageType {
    pub(crate) fn prepare(ty: AbiType, owner: &LoadedModule) -> Result<Self, RuntimeError> {
        let mut contract = Self {
            ty: ty.clone(),
            owner: owner.clone(),
            structures: Vec::new(),
            enumerations: Vec::new(),
        };
        if matches!(ty, AbiType::Builtin(_)) {
            return Ok(contract);
        }
        let mut pending = vec![ty];
        let mut visited = HashSet::new();
        while let Some(ty) = pending.pop() {
            if !visited.insert(ty.clone()) {
                continue;
            }
            match ty {
                AbiType::Struct(nominal) => {
                    let layout = owner
                        .members()
                        .find_map(|member| {
                            member
                                .bytecode
                                .structures
                                .iter()
                                .find(|layout| {
                                    layout.declaration == nominal.declaration
                                        && layout.arguments == nominal.arguments
                                })
                                .cloned()
                        })
                        .ok_or_else(|| {
                            RuntimeError::module_validation(
                                "sequence element struct layout unavailable",
                            )
                        })?;
                    pending.extend(layout.fields.iter().map(|field| field.ty.clone()));
                    pending.extend(nominal.arguments);
                    contract.structures.push(layout);
                }
                AbiType::Enum(nominal) => {
                    let layout = owner
                        .members()
                        .find_map(|member| {
                            member
                                .bytecode
                                .enumerations
                                .iter()
                                .find(|layout| {
                                    layout.declaration == nominal.declaration
                                        && layout.arguments == nominal.arguments
                                })
                                .cloned()
                        })
                        .ok_or_else(|| {
                            RuntimeError::module_validation(
                                "sequence element enum layout unavailable",
                            )
                        })?;
                    pending.extend(
                        layout
                            .variants
                            .iter()
                            .flat_map(|variant| variant.payload.iter().cloned()),
                    );
                    pending.extend(nominal.arguments);
                    contract.enumerations.push(layout);
                }
                AbiType::NativeObject(nominal) | AbiType::Trait(nominal) => {
                    pending.extend(nominal.arguments);
                    pending.extend(nominal.associated_types.into_values());
                }
                AbiType::Tuple(items) | AbiType::StandardEnum { args: items, .. } => {
                    pending.extend(items)
                }
                AbiType::Array(item, _)
                | AbiType::Set(item, _)
                | AbiType::Iter(item)
                | AbiType::Range(item, _) => pending.push(*item),
                AbiType::Map { key, value, .. } => pending.extend([*key, *value]),
                AbiType::Function { params, result } => {
                    pending.extend(params);
                    pending.push(*result);
                }
                AbiType::Builtin(_) => {}
                AbiType::Host(_)
                | AbiType::Projection { .. }
                | AbiType::SelfType(_)
                | AbiType::Parameter { .. } => {
                    return Err(RuntimeError::module_validation(
                        "sequence element is not a closed script-heap type",
                    ));
                }
            }
        }
        Ok(contract)
    }
    pub(crate) fn matches(&self, ty: &AbiType, owner: &LoadedModule) -> bool {
        self.ty == *ty
            && self.structures.iter().all(|layout| {
                owner
                    .members()
                    .any(|member| member.bytecode.structures.contains(layout))
            })
            && self.enumerations.iter().all(|layout| {
                owner
                    .members()
                    .any(|member| member.bytecode.enumerations.contains(layout))
            })
    }
}
