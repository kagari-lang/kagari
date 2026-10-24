//! Source-free common free-function contracts, shared with VM boundary fixtures.
use kagari_abi::representation::ValueType;
use kagari_bytecode::{
    module::BytecodeModule,
    program::{
        BytecodeProgram, ModuleRef,
        updates::{self, EvidenceInputs},
    },
};
use kagari_common::{
    cancellation::CancellationToken,
    identity::{
        DefinitionKind, map::DefinitionContext, mapping::DefinitionMappingError,
        metadata::DefinitionMetadata, table::DefinitionTable,
    },
};
use kagari_contract::{
    exports::ProgramExports,
    native_import::NativeImport,
    types::{
        ConcreteFunctionIdentity, PublicItem,
        named::{NamedContract, NamedDeclaration},
    },
    updates::{
        BodyClassification, Dependency, ProtectedRoot, RootKind, SourceRecord, SourceToken,
        UnknownDependency, UpdateEvidence,
    },
};
use kagari_types::{
    callable::{CallableImplementation, Signature},
    declaration::{FnDecl, Param, TypeDeclaration, TypeDefKind, module::ModuleDecl},
    scalar::BuiltinType,
    ty::Ty,
    visibility::Visibility,
};
use std::collections::HashMap;

/// Source-free monomorphic authoring input, never accepted by runtime verification.
/// Non-scalar signatures must be explicit. Generic authored source templates cannot
/// be recovered from executable instances and must use the compiler's producer.
#[allow(dead_code)] // Shared with consumers that only need native contracts.
pub struct ProgramFixture {
    pub root: ModuleRef,
    pub modules: Vec<BytecodeModule>,
    pub exports: ProgramExports,
}

impl From<BytecodeProgram> for ProgramFixture {
    fn from(program: BytecodeProgram) -> Self {
        Self {
            root: program.root,
            modules: program.modules,
            exports: program.exports,
        }
    }
}

#[allow(dead_code)]
impl ProgramFixture {
    pub fn build(mut self, context: &DefinitionContext) -> BytecodeProgram {
        let update_evidence = evidence(&mut self.modules, context);
        BytecodeProgram {
            root: self.root,
            modules: self.modules,
            exports: self.exports,
            update_evidence,
        }
    }

    pub fn metadata(
        self,
        definitions: DefinitionTable,
        cancel: &CancellationToken,
    ) -> Result<DefinitionMetadata<BytecodeProgram>, DefinitionMappingError> {
        let context = DefinitionContext::new()?;
        // Import the authoring input before assigning missing fixture identities.
        // The temporary empty certificate never reaches verification or a loader.
        let input = BytecodeProgram {
            root: self.root,
            modules: self.modules,
            exports: self.exports,
            update_evidence: UpdateEvidence {
                sources: vec![],
                bodies: vec![],
                roots: vec![],
                targets: vec![],
                selections: vec![],
            },
        };
        let input = DefinitionMetadata::checked(definitions, input, cancel)?
            .import_into(&context, cancel)?
            .into_records();
        let program = Self {
            root: input.root,
            modules: input.modules,
            exports: input.exports,
        }
        .build(&context);
        DefinitionMetadata::checked(context.snapshot(), program, cancel)
    }
}

/// Rebuild after explicit fixture edits, not as part of admission/verification.
#[allow(dead_code)]
pub fn certify(mut program: BytecodeProgram, context: &DefinitionContext) -> BytecodeProgram {
    program.update_evidence = evidence(&mut program.modules, context);
    program
}

fn evidence(modules: &mut [BytecodeModule], context: &DefinitionContext) -> UpdateEvidence {
    let mut classifications = HashMap::new();
    for module in modules.iter_mut() {
        prepare(module, context, &mut classifications);
    }
    let mut sources = Vec::new();
    let mut roots = Vec::new();
    for module in modules.iter() {
        for declaration in &module.named_declarations {
            assert!(
                !matches!(&declaration.contract, NamedContract::Function(function) if !function.generic_params.is_empty())
                    && !matches!(&declaration.contract, NamedContract::Implementation(_)),
                "fixture must supply genuine generic/implementation source evidence through its producer"
            );
            if !matches!(declaration.contract, NamedContract::Alias(_)) {
                sources.push(SourceRecord {
                    declaration: declaration.declaration,
                    generic: None,
                    tokens: vec![],
                    inputs: vec![],
                    calls: vec![],
                    dependencies: if matches!(&declaration.contract, NamedContract::Function(function) if matches!(function.implementation, CallableImplementation::Native(_))) {
                        // Declared effects do not prove the absence of script callbacks.
                        vec![Dependency::Unknown(UnknownDependency { edge: 0 })]
                    } else {
                        vec![]
                    },
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
                // These fixtures author bytecode, not Kagari source. Retain the
                // authored recipe; build() independently extracts its canonical body.
                tokens: vec![SourceToken {
                    kind: 0,
                    text: format!(
                        "{:?}",
                        module.functions[slot.initializer.index()].instructions
                    ),
                }],
                inputs: vec![],
                calls: vec![],
                dependencies: vec![],
            });
            roots.push(ProtectedRoot {
                declaration: ConcreteFunctionIdentity {
                    declaration: slot.contract.declaration,
                    arguments: vec![],
                },
                kind: RootKind::StaticInitializer,
                dependencies: vec![Dependency::Source(slot.contract.declaration)],
                selections: vec![],
            });
        }
    }
    let definitions = context.snapshot();
    roots.sort_by_key(|root| {
        definitions
            .resolve(root.declaration.declaration)
            .unwrap()
            .to_path()
    });
    updates::build(
        modules,
        EvidenceInputs {
            sources,
            roots,
            selections: vec![],
            classifications,
        },
        &definitions,
        &Default::default(),
    )
    .expect("fixture supplies canonical executable evidence")
}

fn prepare(
    module: &mut BytecodeModule,
    context: &DefinitionContext,
    classifications: &mut HashMap<ConcreteFunctionIdentity, BodyClassification>,
) {
    let root = context.intern_root(&module.identity).unwrap();
    for item in &module.public_items {
        let (kind, name, contract) = match item {
            PublicItem::Function(function) => (
                DefinitionKind::Function,
                &function.name,
                NamedContract::Function(function.clone()),
            ),
            PublicItem::Const(constant) => (
                DefinitionKind::Const,
                &constant.name,
                NamedContract::Constant(constant.clone()),
            ),
            _ => continue,
        };
        let declaration = context.intern_child(root, kind, name, 0).unwrap();
        if !module
            .named_declarations
            .iter()
            .any(|record| record.declaration == declaration)
        {
            module.named_declarations.push(NamedDeclaration {
                declaration,
                visibility: Visibility::Public,
                ambiguous_local_body: None,
                contract,
            });
        }
    }
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
                scalar(*physical).expect("fixture must declare non-scalar parameter types")
            });
        }
        if semantic.result.is_none() {
            semantic.result = Some(
                scalar(function.metadata.return_type)
                    .expect("fixture must declare its non-scalar result type"),
            );
        }
        if let Some(slot) = module.module_slots.iter().find(|slot| {
            slot.contract.supplier == module.identity && slot.initializer == function.id
        }) {
            classifications.insert(
                identity,
                BodyClassification::StaticInitializer(slot.contract.declaration),
            );
            continue;
        }
        if !module
            .named_declarations
            .iter()
            .any(|record| record.declaration == identity.declaration)
        {
            assert!(
                identity.arguments.is_empty() && semantic.generic.is_none(),
                "generic fixtures must supply their authored declaration, not infer it from an instance"
            );
            module.named_declarations.push(NamedDeclaration {
                declaration: identity.declaration,
                visibility: Visibility::Private,
                ambiguous_local_body: None,
                contract: NamedContract::Function(FnDecl {
                    name: context
                        .snapshot()
                        .resolve(identity.declaration)
                        .unwrap()
                        .last()
                        .unwrap()
                        .name
                        .to_owned(),
                    implementation: CallableImplementation::Script,
                    host_policy: None,
                    method_policy: Default::default(),
                    generic_params: vec![],
                    bounds: vec![],
                    params: (0..function.parameter_count as usize)
                        .map(|index| Param {
                            name: format!("arg{index}"),
                            mutable: false,
                            ty: semantic.params[&index].clone(),
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

/// Carry every free-function contract, including unused entries, without source analysis.
/// External schemas retain their nominal owners. Generic proofs, GC layouts and trait impls
/// remain at their dedicated fixtures.
pub fn free_function_program(
    declarations: &DefinitionMetadata<ModuleDecl>,
) -> DefinitionMetadata<BytecodeProgram> {
    let module = declarations.records();
    assert!(
        module
            .types
            .iter()
            .all(|ty| matches!(ty.kind, TypeDefKind::Host(_)))
    );
    assert!(module.traits.is_empty());
    assert!(module.implementations.is_empty());
    assert!(module.paths.is_empty());
    let native_declarations = module
        .native_declarations(declarations.definitions())
        .unwrap();
    let native_imports = native_declarations
        .iter()
        .map(|declaration| {
            let function = &declaration.function;
            assert!(function.generic_params.is_empty() && function.bounds.is_empty());
            assert!(declaration.callable_requirements.is_empty());
            let CallableImplementation::Native(binding) = function.implementation else {
                panic!("fixture requires an ordinary native entry")
            };
            NativeImport {
                receiver_access: function.method_policy.receiver_access,
                result_adapter: None,
                generic: None,
                instance: ConcreteFunctionIdentity {
                    declaration: declaration.declaration,
                    arguments: vec![],
                },
                binding,
                signature: Signature {
                    params: function
                        .params
                        .iter()
                        .map(|parameter| parameter.ty.clone())
                        .collect(),
                    result: function.return_type.clone(),
                },
                requirements: vec![],
                callables: vec![],
            }
        })
        .collect();
    let external_types = module
        .types
        .iter()
        .map(|ty| TypeDeclaration {
            declaration: module
                .definition(DefinitionKind::Struct, &ty.name, declarations.definitions())
                .unwrap(),
            definition: ty.clone(),
        })
        .collect();
    let program = ProgramFixture {
        exports: Default::default(),
        root: ModuleRef::new(0),
        modules: vec![BytecodeModule {
            identity: module.identity.clone(),
            source_name: "native-contract-fixture".into(),
            external_types,
            public_items: native_declarations
                .iter()
                .filter(|declaration| !module.private_functions.contains(&declaration.declaration))
                .map(|declaration| PublicItem::Function(declaration.function.clone()))
                .chain(module.types.iter().cloned().map(PublicItem::Type))
                .collect(),
            named_declarations: native_declarations
                .iter()
                .map(|declaration| NamedDeclaration {
                    declaration: declaration.declaration,
                    visibility: if module.private_functions.contains(&declaration.declaration) {
                        Visibility::Private
                    } else {
                        Visibility::Public
                    },
                    ambiguous_local_body: None,
                    contract: NamedContract::Function(declaration.function.clone()),
                })
                .collect(),
            native_declarations,
            native_imports,
            ..Default::default()
        }],
    };
    program
        .metadata(declarations.definitions().clone(), &Default::default())
        .unwrap()
}
