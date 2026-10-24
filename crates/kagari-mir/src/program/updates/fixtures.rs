//! Explicit evidence production for hand-authored source-free codec fixtures.
use crate::{
    function::MirModule,
    program::{
        records::MirProgram,
        updates::{self, EvidenceInputs},
    },
};
use kagari_abi::representation::ValueType;
use kagari_common::identity::{map::DefinitionContext, mapping::DefinitionMappingError};
use kagari_contract::{
    types::named::{NamedContract, NamedDeclaration},
    updates::{
        BodyClassification, SourceRecord, SourceToken, StaticInitializer, UpdateEvidence,
        limits::UpdateError,
    },
};
use kagari_types::{
    callable::CallableImplementation,
    declaration::{FnDecl, Param},
    scalar::BuiltinType,
    ty::Ty,
    visibility::Visibility,
};

use std::collections::{HashMap, btree_map::Entry};

pub(crate) fn evidence(
    modules: &mut [MirModule],
    context: &DefinitionContext,
) -> Result<UpdateEvidence, UpdateError> {
    let mut classifications = HashMap::new();
    let mut sources = Vec::new();
    let mut initializers = Vec::new();
    for module in modules.iter_mut() {
        for function in &mut module.functions {
            for (index, parameter) in function.params.iter().enumerate() {
                if let Entry::Vacant(entry) = function.semantic.params.entry(index) {
                    entry.insert(scalar(parameter.ty).ok_or(UpdateError::InvalidEvidence)?);
                }
            }
            let initializer = module.module_slots.iter().find(|slot| {
                slot.contract.supplier == module.identity && slot.initializer == function.id
            });
            // Initializer result types are explicit authority, including in malformed
            // codec fixtures; a physical carrier must not repair missing evidence.
            if function.semantic.result.is_none() && initializer.is_none() {
                function.semantic.result = scalar(function.return_type);
            }
            let classification = if let Some(slot) = initializer {
                BodyClassification::StaticInitializer(slot.contract.declaration)
            } else {
                if !module
                    .abi
                    .named_declarations
                    .iter()
                    .any(|record| record.declaration == function.instance.declaration)
                {
                    let name = context
                        .snapshot()
                        .resolve(function.instance.declaration)
                        .map_err(DefinitionMappingError::from)?
                        .last()
                        .ok_or(UpdateError::InvalidEvidence)?
                        .name
                        .to_owned();
                    module.abi.named_declarations.push(NamedDeclaration {
                        declaration: function.instance.declaration,
                        visibility: Visibility::Private,
                        ambiguous_local_body: None,
                        contract: NamedContract::Function(FnDecl {
                            name,
                            implementation: CallableImplementation::Script,
                            host_policy: None,
                            method_policy: Default::default(),
                            generic_params: function
                                .semantic
                                .generic
                                .as_ref()
                                .map_or_else(Vec::new, |body| body.parameters.clone()),
                            bounds: function
                                .semantic
                                .generic
                                .as_ref()
                                .map_or_else(Vec::new, |body| body.bounds.clone()),
                            params: function
                                .semantic
                                .params
                                .values()
                                .enumerate()
                                .map(|(index, ty)| Param {
                                    name: format!("arg{index}"),
                                    mutable: false,
                                    ty: ty.clone(),
                                })
                                .collect(),
                            return_type: function
                                .semantic
                                .result
                                .clone()
                                .ok_or(UpdateError::InvalidEvidence)?,
                        }),
                    });
                }
                BodyClassification::Named(function.instance.declaration)
            };
            classifications.insert(function.instance.clone(), classification);
        }
        for declaration in &module.abi.named_declarations {
            if !matches!(declaration.contract, NamedContract::Alias(_)) {
                sources.push(SourceRecord {
                    declaration: declaration.declaration,
                    generic: None,
                    tokens: vec![],
                    inputs: vec![],
                    calls: vec![],
                    bodies: vec![],
                });
            }
        }
        for slot in module
            .module_slots
            .iter()
            .filter(|slot| slot.contract.supplier == module.identity)
        {
            sources.push(SourceRecord {
                declaration: slot.contract.declaration,
                generic: None,
                tokens: vec![SourceToken {
                    kind: 0,
                    text: "source-free initializer recipe".into(),
                }],
                inputs: vec![],
                calls: vec![],
                bodies: vec![],
            });
            initializers.push(StaticInitializer {
                declaration: slot.contract.declaration,
            });
        }
    }
    initializers.sort_by_key(|initializer| {
        context
            .snapshot()
            .resolve(initializer.declaration)
            .unwrap()
            .to_path()
    });
    updates::build(
        modules,
        EvidenceInputs {
            sources,
            initializers,
            selections: vec![],
            classifications,
        },
        &context.snapshot(),
        &Default::default(),
    )
}

pub(crate) fn certify(mut program: MirProgram, context: &DefinitionContext) -> MirProgram {
    program.update_evidence = evidence(&mut program.modules, context)
        .expect("source-free fixture supplies true executable evidence");
    program
}

fn scalar(value: ValueType) -> Option<Ty> {
    Some(Ty::Builtin(match value {
        ValueType::Never => BuiltinType::Never,
        ValueType::Unit => BuiltinType::Unit,
        ValueType::Bool => BuiltinType::Bool,
        ValueType::I32 => BuiltinType::I32,
        ValueType::I64 => BuiltinType::I64,
        ValueType::U64 => BuiltinType::U64,
        ValueType::F32 => BuiltinType::F32,
        ValueType::F64 => BuiltinType::F64,
        ValueType::Str => BuiltinType::String,
        ValueType::Char => BuiltinType::Char,
        _ => return None,
    }))
}
