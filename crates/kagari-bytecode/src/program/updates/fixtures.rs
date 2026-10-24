//! Explicit source-free test producers. Admission never repairs an absent certificate.
use crate::{
    module::BytecodeModule,
    program::{
        BytecodeProgram,
        updates::{self, EvidenceInputs},
    },
};
use kagari_abi::representation::ValueType;
use kagari_common::identity::{DefinitionKind, map::DefinitionContext};
use kagari_contract::{
    types::{
        ConcreteFunctionIdentity,
        named::{NamedContract, NamedDeclaration},
    },
    updates::{BodyClassification, SourceRecord, SourceToken, StaticInitializer},
};
use kagari_types::{
    callable::CallableImplementation,
    declaration::{FnDecl, Param},
    scalar::BuiltinType,
    ty::{GenericParam, Ty},
    visibility::Visibility,
};
use std::collections::HashMap;

/// Complete old hand-authored scalar fixtures without changing their executable.
/// Non-scalar signatures must already be explicit; physical carriers cannot prove them.
pub(crate) fn certify(
    mut program: BytecodeProgram,
    context: &DefinitionContext,
) -> BytecodeProgram {
    let mut classifications = HashMap::new();
    for module in &mut program.modules {
        prepare(module, context, &mut classifications);
    }
    let mut sources = Vec::new();
    let mut initializers = Vec::new();
    for module in &program.modules {
        for declaration in &module.named_declarations {
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
    let definitions = context.snapshot();
    initializers.sort_by_key(|initializer| {
        definitions
            .resolve(initializer.declaration)
            .unwrap()
            .to_path()
    });
    program.update_evidence = updates::build(
        &program.modules,
        EvidenceInputs {
            sources,
            initializers,
            selections: vec![],
            classifications,
        },
        &definitions,
        &Default::default(),
    )
    .expect("source-free fixture supplies true executable evidence");
    program
}

fn prepare(
    module: &mut BytecodeModule,
    context: &DefinitionContext,
    classifications: &mut HashMap<ConcreteFunctionIdentity, BodyClassification>,
) {
    let root = context.intern_root(&module.identity).unwrap();
    for function in &mut module.functions {
        let identity = function
            .identity
            .get_or_insert_with(|| ConcreteFunctionIdentity {
                declaration: context
                    .intern_child(root, DefinitionKind::Function, &function.name, 0)
                    .unwrap(),
                arguments: vec![],
            })
            .clone();
        module.function_table[function.id.index()].identity = Some(identity.clone());
        let semantic = &mut function.metadata.semantic;
        for (index, physical) in function.metadata.params.iter().enumerate() {
            semantic.params.entry(index).or_insert_with(|| {
                scalar(*physical).expect("fixture must supply non-scalar parameter type")
            });
        }
        if semantic.result.is_none() {
            semantic.result = Some(
                scalar(function.metadata.return_type)
                    .expect("fixture must supply non-scalar result type"),
            );
        }

        let initializer = module.module_slots.iter().find(|slot| {
            slot.contract.supplier == module.identity && slot.initializer == function.id
        });
        if let Some(slot) = initializer {
            classifications.insert(
                identity,
                BodyClassification::StaticInitializer(slot.contract.declaration),
            );
            continue;
        }
        if !module
            .named_declarations
            .iter()
            .any(|declaration| declaration.declaration == identity.declaration)
        {
            let name = context
                .snapshot()
                .resolve(identity.declaration)
                .unwrap()
                .last()
                .unwrap()
                .name
                .to_owned();
            let parameters = semantic.generic.as_ref().map_or_else(
                || {
                    (0..identity.arguments.len())
                        .map(|position| GenericParam {
                            owner: identity.declaration,
                            position,
                        })
                        .collect()
                },
                |body| body.parameters.clone(),
            );
            module.named_declarations.push(NamedDeclaration {
                declaration: identity.declaration,
                visibility: Visibility::Private,
                ambiguous_local_body: None,
                contract: NamedContract::Function(FnDecl {
                    name,
                    implementation: CallableImplementation::Script,
                    host_policy: None,
                    method_policy: Default::default(),
                    generic_params: parameters,
                    bounds: semantic
                        .generic
                        .as_ref()
                        .map_or_else(Vec::new, |body| body.bounds.clone()),
                    params: semantic
                        .params
                        .values()
                        .enumerate()
                        .map(|(index, ty)| Param {
                            name: format!("arg{index}"),
                            mutable: false,
                            ty: ty.clone(),
                        })
                        .collect(),
                    return_type: semantic.result.clone().unwrap(),
                }),
            });
        }
        classifications.insert(
            identity.clone(),
            BodyClassification::Named(identity.declaration),
        );
    }
}

fn scalar(value: ValueType) -> Option<Ty> {
    Some(match value {
        ValueType::Never => Ty::Builtin(BuiltinType::Never),
        ValueType::Unit => Ty::Builtin(BuiltinType::Unit),
        ValueType::Bool => Ty::Builtin(BuiltinType::Bool),
        ValueType::I32 => Ty::Builtin(BuiltinType::I32),
        ValueType::I64 => Ty::Builtin(BuiltinType::I64),
        ValueType::U64 => Ty::Builtin(BuiltinType::U64),
        ValueType::F32 => Ty::Builtin(BuiltinType::F32),
        ValueType::F64 => Ty::Builtin(BuiltinType::F64),
        ValueType::Str => Ty::Builtin(BuiltinType::String),
        ValueType::Char => Ty::Builtin(BuiltinType::Char),
        _ => return None,
    })
}
