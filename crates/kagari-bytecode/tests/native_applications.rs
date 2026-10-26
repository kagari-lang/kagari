//! Source-free native nominal evidence and its dependency/binder boundaries.
use kagari_bytecode::{
    artifact::{ArtifactValidationError, KbcArtifact},
    instruction::{BytecodeInstruction, CallTarget, NativeImportId, Slot},
    module::{
        BytecodeFunction, BytecodeModule, CallableTarget, FunctionMetadata, FunctionRecord,
        InstructionOrigin, InterfaceTableRecord, RootSlotLayout,
    },
    program::{
        BytecodeProgram, ModuleRef,
        native::NativeApplicationOccurrence,
        updates::{self, EvidenceInputs},
        verified::VerifiedBytecodeProgram,
    },
};
use kagari_common::identity::{DefinitionKind, ModuleIdentity, map::DefinitionContext};
use kagari_contract::{
    callable::interface::dynamic_interface_valid,
    callable::{
        construction::evidence::{
            ConstructionRequirement, ConstructionSelection, ConstructionWitness,
        },
        generic::GenericBody,
        shared::SharedCall,
    },
    native_import::{NativeImport, result::NativeResultAdapter},
    representation::semantic_representation,
    types::{
        ConcreteFunctionIdentity, InterfaceTable, PublicItem,
        named::{ImplementationContract, NamedContract, NamedDeclaration},
    },
    types::{
        applications::native::NativeApplications,
        proofs::{ProofCatalog, implementation::Implementation},
        type_contract,
    },
    updates::{BodyClassification, SourceRecord, UpdateEvidence},
};
use kagari_types::{
    callable::{CallableImplementation, Signature},
    declaration::{
        FnDecl, NativeDeclaration, TraitDef, TypeDef, TypeDefKind, native::NativeStorageLayout,
    },
    scalar::BuiltinType,
    ty::{GenericBound, GenericParam, NominalTy, Ty},
    visibility::Visibility,
};

fn fixture(open: bool) -> (DefinitionContext, BytecodeProgram) {
    let context = DefinitionContext::new().unwrap();
    let identity = ModuleIdentity::single_file("bounded.kgr");
    let root = context.intern_root(&identity).unwrap();
    let id = |kind, name| context.intern_child(root, kind, name, 0).unwrap();
    let native = id(DefinitionKind::AssociatedType, "Bag");
    let marker = id(DefinitionKind::Trait, "Marker");
    let entry = id(DefinitionKind::Function, "from_value");
    let binding = id(DefinitionKind::Function, "rust_from_value");
    let parameter = GenericParam {
        owner: native,
        position: 0,
    };
    let argument = GenericParam {
        owner: entry,
        position: 0,
    };
    let interface = NominalTy {
        declaration: marker,
        arguments: vec![],
        associated_types: Default::default(),
    };
    let bounds = vec![GenericBound {
        ty: argument.as_type(),
        constraints: vec![interface.clone()],
    }];
    let result = Ty::NativeObject(NominalTy {
        declaration: native,
        arguments: vec![if open {
            argument.as_type()
        } else {
            Ty::Builtin(BuiltinType::I32)
        }],
        associated_types: Default::default(),
    });
    let function = FnDecl {
        name: "from_value".into(),
        implementation: CallableImplementation::Native(binding),
        host_policy: None,
        method_policy: Default::default(),
        generic_params: if open { vec![argument.clone()] } else { vec![] },
        bounds: if open { bounds.clone() } else { vec![] },
        params: vec![],
        return_type: result.clone(),
    };
    let import = NativeImport {
        receiver_access: None,
        result_adapter: None,
        generic: open.then(|| GenericBody {
            parameters: vec![argument.clone()],
            bounds: bounds.clone(),
        }),
        instance: ConcreteFunctionIdentity {
            declaration: entry,
            arguments: if open {
                vec![argument.as_type()]
            } else {
                vec![]
            },
        },
        binding,
        signature: Signature {
            params: vec![],
            result,
        },
        requirements: if open { bounds } else { vec![] },
        callables: vec![],
    };
    let mut module = BytecodeModule {
        identity,
        public_items: vec![
            PublicItem::Type(TypeDef {
                name: "Bag".into(),
                kind: TypeDefKind::NativeStorage(NativeStorageLayout::Opaque),
                generic_params: vec![parameter.clone()],
                bounds: vec![GenericBound {
                    ty: parameter.as_type(),
                    constraints: vec![interface.clone()],
                }],
                fields: vec![],
                variants: vec![],
            }),
            PublicItem::Trait(TraitDef {
                name: "Marker".into(),
                generic_params: vec![],
                bounds: vec![],
                supertraits: vec![],
                methods: vec![],
                associated_types: vec![],
                associated_consts: vec![],
                conversion_adapter: None,
            }),
            PublicItem::InterfaceTable(Box::new(InterfaceTable {
                declaration: id(DefinitionKind::Impl, ""),
                name: "marker_i32".into(),
                generic_params: vec![],
                bounds: vec![],
                for_type: Ty::Builtin(BuiltinType::I32),
                trait_type: Ty::Trait(interface),
                methods: vec![],
                associated_consts: vec![],
                associated_type_families: vec![],
                bridge: None,
            })),
            PublicItem::Function(function.clone()),
        ],
        named_declarations: vec![NamedDeclaration {
            declaration: entry,
            visibility: Visibility::Public,
            ambiguous_local_body: None,
            contract: NamedContract::Function(function.clone()),
        }],
        native_declarations: vec![NativeDeclaration {
            declaration: entry,
            function,
            concrete_result: None,
            callable_requirements: vec![],
        }],
        native_imports: vec![import],
        ..Default::default()
    };
    table_inventory(&mut module);
    let mut program = BytecodeProgram {
        root: ModuleRef::new(0),
        modules: vec![module],
        exports: Default::default(),
        update_evidence: UpdateEvidence {
            source_manifest: None,
            sources: vec![],
            bodies: vec![],
            initializers: vec![],
            targets: vec![],
            selections: vec![],
        },
    };
    certify(&mut program, &context);
    (context, program)
}

fn certify(program: &mut BytecodeProgram, context: &DefinitionContext) {
    let sources = program
        .modules
        .iter()
        .flat_map(|module| &module.named_declarations)
        .map(|record| SourceRecord {
            declaration: record.declaration,
            generic: None,
            tokens: vec![],
            inputs: vec![],
            calls: vec![],
            bodies: vec![],
        })
        .collect();
    program.update_evidence = updates::build(
        &program.modules,
        EvidenceInputs {
            sources,
            initializers: vec![],
            selections: vec![],
            classifications: program
                .modules
                .iter()
                .flat_map(|module| &module.functions)
                .map(|function| {
                    let identity = function.identity.clone().unwrap();
                    (
                        identity.clone(),
                        BodyClassification::Named(identity.declaration),
                    )
                })
                .collect(),
        },
        &context.snapshot(),
        &Default::default(),
    )
    .unwrap();
}

fn table_inventory(module: &mut BytecodeModule) {
    module.interface_tables.clear();
    module
        .named_declarations
        .retain(|record| !matches!(record.contract, NamedContract::Implementation(_)));
    for item in &module.public_items {
        let PublicItem::InterfaceTable(table) = item else {
            continue;
        };
        module.interface_tables.push(InterfaceTableRecord {
            view: None,
            arguments: vec![],
            declaration: table.declaration,
            methods: vec![],
            parents: vec![],
        });
        module.named_declarations.push(NamedDeclaration {
            declaration: table.declaration,
            visibility: Visibility::Private,
            ambiguous_local_body: None,
            contract: NamedContract::Implementation(ImplementationContract {
                bridge: None,
                generic_params: table.generic_params.clone(),
                bounds: table.bounds.clone(),
                receiver: table.for_type.clone(),
                trait_type: Some(table.trait_type.clone()),
            }),
        });
    }
}

#[test]
fn native_application_witness_roundtrips_source_free_and_is_not_constructor_evidence() {
    let (context, program) = fixture(false);
    let verified = VerifiedBytecodeProgram::new(program, &context, &Default::default()).unwrap();
    let evidence = verified.native_applications();
    assert_eq!(evidence.len(), 1);
    assert!(evidence[0].is_closed());
    assert_eq!(evidence[0].carrier(), ModuleRef::new(0));
    assert_eq!(
        evidence[0].occurrence(),
        NativeApplicationOccurrence::Import(NativeImportId::new(0))
    );
    assert_eq!(
        evidence[0].nominal().arguments,
        [Ty::Builtin(BuiltinType::I32)]
    );
    // Admission from an ordinary application does not invent a callable `new` entry.
    assert_eq!(verified.program().modules[0].native_imports.len(), 1);
    let artifact =
        KbcArtifact::from_verified(verified, Default::default(), &Default::default()).unwrap();
    let bytes = artifact.records().to_bytes(artifact.definitions()).unwrap();
    let foreign = DefinitionContext::new().unwrap();
    let decoded = KbcArtifact::from_bytes(&bytes)
        .unwrap()
        .import_into(&foreign, &Default::default())
        .unwrap()
        .into_records();
    let verified = decoded
        .into_verified(&Default::default(), &foreign, &Default::default())
        .unwrap()
        .into_bytecode();
    assert_eq!(verified.native_applications().len(), 1);
    assert!(verified.native_applications()[0].is_closed());
    let Ty::NativeObject(original) = &artifact.records().program.modules[0].native_imports[0]
        .signature
        .result
    else {
        unreachable!()
    };
    assert_ne!(
        verified.native_applications()[0].nominal().declaration,
        original.declaration
    );
    let mut editable = verified.into_unverified().into_records();
    editable.modules[0].native_imports.clear();
    certify(&mut editable, &foreign);
    let verified = VerifiedBytecodeProgram::new(editable, &foreign, &Default::default()).unwrap();
    assert!(
        verified.native_applications().is_empty(),
        "a declaration signature alone is not an occurrence"
    );
}

fn reverify_source_free(
    artifact: &KbcArtifact,
    program: BytecodeProgram,
    context: &DefinitionContext,
) -> Result<VerifiedBytecodeProgram, ArtifactValidationError> {
    let mut forged = artifact.clone();
    forged.program = program;
    assert!(forged.source.is_none());
    assert!(forged.portable_mir.is_none());
    let bytes = forged.to_bytes(&context.snapshot()).unwrap();
    let destination = DefinitionContext::new().unwrap();
    KbcArtifact::from_bytes(&bytes)
        .unwrap()
        .import_into(&destination, &Default::default())
        .unwrap()
        .into_records()
        .into_verified(&Default::default(), &destination, &Default::default())
        .map(|artifact| artifact.into_bytecode())
}

#[test]
fn native_application_witness_rejects_missing_forged_and_wrong_scope_proofs() {
    let (context, program) = fixture(false);
    let artifact = KbcArtifact::from_verified(
        VerifiedBytecodeProgram::new(program.clone(), &context, &Default::default()).unwrap(),
        Default::default(),
        &Default::default(),
    )
    .unwrap();
    assert!(artifact.records().source.is_none());
    assert!(artifact.records().portable_mir.is_none());
    for mutation in 0..4 {
        let mut corrupt = program.clone();
        match mutation {
            0 => {
                corrupt.modules[0].public_items.remove(2);
            }
            1 => {
                let PublicItem::InterfaceTable(table) = &mut corrupt.modules[0].public_items[2]
                else {
                    unreachable!()
                };
                table.bounds.push(GenericBound {
                    ty: table.for_type.clone(),
                    constraints: vec![match &table.trait_type {
                        Ty::Trait(n) => n.clone(),
                        _ => unreachable!(),
                    }],
                });
            }
            2 => {
                let PublicItem::Type(definition) = &mut corrupt.modules[0].public_items[0] else {
                    unreachable!()
                };
                definition.bounds[0].ty = Ty::Builtin(BuiltinType::Bool);
            }
            3 => {
                // A sibling's valid table is not in the import carrier's closure.
                let mut carrier = BytecodeModule {
                    identity: ModuleIdentity::single_file("carrier.kgr"),
                    dependencies: vec![ModuleRef::new(0)],
                    ..Default::default()
                };
                carrier.native_imports = std::mem::take(&mut corrupt.modules[0].native_imports);
                let mut sibling = BytecodeModule {
                    identity: ModuleIdentity::single_file("proof.kgr"),
                    dependencies: vec![ModuleRef::new(0)],
                    ..Default::default()
                };
                let PublicItem::InterfaceTable(mut table) =
                    corrupt.modules[0].public_items.remove(2)
                else {
                    unreachable!()
                };
                let root = context.intern_root(&sibling.identity).unwrap();
                table.declaration = context
                    .intern_child(root, DefinitionKind::Impl, "", 0)
                    .unwrap();
                sibling.public_items.push(PublicItem::InterfaceTable(table));
                corrupt.modules.push(carrier);
                corrupt.modules.push(sibling);
                corrupt.modules.push(BytecodeModule {
                    identity: ModuleIdentity::single_file("root.kgr"),
                    dependencies: vec![ModuleRef::new(1), ModuleRef::new(2)],
                    ..Default::default()
                });
                corrupt.root = ModuleRef::new(3);
            }
            _ => unreachable!(),
        }
        for module in &mut corrupt.modules {
            table_inventory(module);
        }
        certify(&mut corrupt, &context);
        // Neither serialized summaries nor a foreign identity import can carry
        // the old seal across edits to executable proof resources.
        let error = reverify_source_free(artifact.records(), corrupt, &context).unwrap_err();
        assert!(
            matches!(
                error,
                kagari_bytecode::artifact::ArtifactValidationError::Bytecode(
                    kagari_bytecode::verifier::BytecodeVerificationError::InvalidHostInterface(_)
                )
            ),
            "mutation {mutation}: {error:?}"
        );
    }
}

fn add_shared_caller(program: &mut BytecodeProgram, context: &DefinitionContext) {
    let module = &mut program.modules[0];
    let import = &module.native_imports[0];
    let root = context.snapshot().lookup_root(&module.identity).unwrap();
    let declaration = context
        .intern_child(root, DefinitionKind::Function, "closed_call", 0)
        .unwrap();
    let Ty::NativeObject(mut nominal) = import.signature.result.clone() else {
        unreachable!()
    };
    nominal.arguments = vec![Ty::Builtin(BuiltinType::I32)];
    let result = Ty::NativeObject(nominal);
    let physical = semantic_representation(&result);
    let marker = import.requirements[0].constraints[0].clone();
    let call = SharedCall {
        receiver_access: None,
        constructions: vec![ConstructionWitness {
            requirement: ConstructionRequirement {
                receiver: Ty::Builtin(BuiltinType::I32),
                interface: marker,
            },
            selection: ConstructionSelection::Declared {
                implementation: ConcreteFunctionIdentity {
                    declaration: module.interface_tables[0].declaration,
                    arguments: vec![],
                },
                operations: vec![],
                constructions: vec![],
            },
        }],
        instance: import.instance.clone(),
        implementation: CallableImplementation::Native(import.binding),
        arguments: vec![Ty::Builtin(BuiltinType::I32)],
        signature: Signature {
            params: vec![],
            result: result.clone(),
        },
        operations: vec![],
    };
    let instructions = vec![
        BytecodeInstruction::Call {
            dst: Some(Slot::new(0)),
            callee: CallTarget::Shared {
                module: ModuleRef::new(0),
                target: CallableTarget::Native(NativeImportId::new(0)),
                contract: Box::new(call),
            },
            args: vec![],
        },
        BytecodeInstruction::Return(Some(Slot::new(0))),
    ];
    let mut metadata = FunctionMetadata {
        return_type: physical,
        slots: vec![physical],
        roots: RootSlotLayout::from_types(&[physical]),
        origins: (0..instructions.len())
            .map(|instruction| InstructionOrigin {
                instruction,
                span: Default::default(),
                line: None,
                column: None,
            })
            .collect(),
        ..Default::default()
    };
    metadata.semantic.result = Some(result.clone());
    metadata.semantic.slots.insert(0, result.clone());
    let identity = ConcreteFunctionIdentity {
        declaration,
        arguments: vec![],
    };
    let function = BytecodeFunction {
        identity: Some(identity.clone()),
        name: "closed_call".into(),
        slot_count: 1,
        metadata,
        instructions,
        ..Default::default()
    };
    module.function_table.push(FunctionRecord {
        id: function.id,
        identity: Some(identity),
        name: function.name.clone(),
        params: vec![],
        return_type: physical,
        effects: function.metadata.effects,
    });
    module.functions.push(function);
    module.types.push(physical);
    module.named_declarations.push(NamedDeclaration {
        declaration,
        visibility: Visibility::Private,
        ambiguous_local_body: None,
        contract: NamedContract::Function(FnDecl {
            name: "closed_call".into(),
            implementation: CallableImplementation::Script,
            host_policy: None,
            method_policy: Default::default(),
            generic_params: vec![],
            bounds: vec![],
            params: vec![],
            return_type: result,
        }),
    });
    certify(program, context);
}

#[test]
fn native_application_contract_checks_nested_types_concrete_adapters_and_static_only_bounds() {
    let (context, mut program) = fixture(false);
    let module = &mut program.modules[0];
    let marker = module
        .public_items
        .iter()
        .find_map(|item| match item {
            PublicItem::InterfaceTable(table) => match &table.trait_type {
                Ty::Trait(n) => Some(n.clone()),
                _ => None,
            },
            _ => None,
        })
        .unwrap();
    let static_method = FnDecl {
        name: "make".into(),
        implementation: CallableImplementation::Required,
        host_policy: None,
        method_policy: Default::default(),
        generic_params: vec![],
        bounds: vec![],
        params: vec![],
        return_type: Ty::Builtin(BuiltinType::I32),
    };
    context
        .intern_child(marker.declaration, DefinitionKind::Method, "make", 0)
        .unwrap();
    let PublicItem::Trait(trait_def) = &mut module.public_items[1] else {
        unreachable!()
    };
    trait_def.methods.push(static_method.clone());
    let PublicItem::InterfaceTable(table) = &mut module.public_items[2] else {
        unreachable!()
    };
    context
        .intern_child(table.declaration, DefinitionKind::Method, "make", 0)
        .unwrap();
    table.methods.push(FnDecl {
        implementation: CallableImplementation::Script,
        ..static_method
    });
    let PublicItem::Trait(trait_def) = &module.public_items[1] else {
        unreachable!()
    };
    let PublicItem::InterfaceTable(table) = &module.public_items[2] else {
        unreachable!()
    };
    let cancel = Default::default();
    let catalog = ProofCatalog::new(
        vec![Implementation::Interface(table)],
        [],
        [(marker.declaration, trait_def)],
        [],
        &context.snapshot(),
        &cancel,
    )
    .unwrap();
    assert!(
        !dynamic_interface_valid(
            &[],
            &marker,
            &|id| catalog.trait_contract(id),
            catalog.definitions(),
            &cancel
        )
        .unwrap()
    );
    let check = NativeApplications::new(
        &catalog,
        |id| {
            type_contract(
                &module.identity,
                &module.public_items,
                id,
                catalog.definitions(),
            )
        },
        &cancel,
    );
    let import = &module.native_imports[0];
    assert_eq!(
        check.import(import).unwrap().len(),
        1,
        "static constraints need no dynamic construction"
    );
    let Ty::NativeObject(nominal) = &import.signature.result else {
        unreachable!()
    };
    let mut invalid = nominal.clone();
    invalid.arguments = vec![Ty::Builtin(BuiltinType::Bool)];
    assert!(
        check
            .types(
                [&Ty::Array(Box::new(Ty::NativeObject(invalid.clone())))],
                None
            )
            .is_err()
    );
    let mut adapted = import.clone();
    adapted.signature.result = Ty::Builtin(BuiltinType::Unit);
    adapted.result_adapter = Some(NativeResultAdapter {
        receiver: Ty::NativeObject(invalid),
        implementation: ConcreteFunctionIdentity {
            declaration: table.declaration,
            arguments: vec![],
        },
    });
    assert!(
        check.import(&adapted).is_err(),
        "the exposed result cannot conceal a bad concrete result"
    );
    for mutation in 0..3 {
        let mut invalid = nominal.clone();
        match mutation {
            0 => invalid.arguments.clear(),
            1 => {
                invalid
                    .associated_types
                    .insert(marker.declaration, Ty::Builtin(BuiltinType::I32));
            }
            2 => {
                invalid.arguments = vec![
                    GenericParam {
                        owner: import.instance.declaration,
                        position: 0,
                    }
                    .as_type(),
                ]
            }
            _ => unreachable!(),
        }
        assert!(
            check.types([&Ty::NativeObject(invalid)], None).is_err(),
            "mutation {mutation}"
        );
    }
    cancel.cancel();
    assert_eq!(
        check.import(import),
        Err(kagari_types::ty::substitution::TypeTransformError::Cancelled)
    );
}

#[test]
fn native_shared_call_issues_closed_evidence_but_still_requires_construction_evidence() {
    let (context, mut program) = fixture(true);
    add_shared_caller(&mut program, &context);
    let verified =
        VerifiedBytecodeProgram::new(program.clone(), &context, &Default::default()).unwrap();
    assert_eq!(verified.native_applications().len(), 2);
    let witness = verified
        .native_applications()
        .iter()
        .find(|w| w.is_closed())
        .unwrap();
    assert!(matches!(
        witness.occurrence(),
        NativeApplicationOccurrence::SharedCall { offset: 0, .. }
    ));
    assert_eq!(witness.nominal().arguments, [Ty::Builtin(BuiltinType::I32)]);
    let BytecodeInstruction::Call {
        callee: CallTarget::Shared { contract, .. },
        ..
    } = &mut program.modules[0].functions[0].instructions[0]
    else {
        unreachable!()
    };
    contract.constructions.clear();
    certify(&mut program, &context);
    assert!(VerifiedBytecodeProgram::new(program, &context, &Default::default()).is_err());
}

#[test]
fn native_adapter_source_free_admission_checks_concrete_result_bounds() {
    let (context, mut program) = fixture(false);
    let module = &mut program.modules[0];
    let PublicItem::InterfaceTable(proof) = &module.public_items[2] else {
        unreachable!()
    };
    let interface = proof.trait_type.clone();
    let mut adapter = proof.as_ref().clone();
    let root = context.snapshot().lookup_root(&module.identity).unwrap();
    adapter.declaration = context
        .intern_child(root, DefinitionKind::Impl, "", 1)
        .unwrap();
    adapter.name = "marker_bag".into();
    let import = &mut module.native_imports[0];
    let concrete = import.signature.result.clone();
    adapter.for_type = concrete.clone();
    import.signature.result = interface.clone();
    import.result_adapter = Some(NativeResultAdapter {
        receiver: concrete.clone(),
        implementation: ConcreteFunctionIdentity {
            declaration: adapter.declaration,
            arguments: vec![],
        },
    });
    module.native_declarations[0].concrete_result = Some(concrete);
    module.native_declarations[0].function.return_type = interface.clone();
    let PublicItem::Function(function) = &mut module.public_items[3] else {
        unreachable!()
    };
    function.return_type = interface.clone();
    let NamedContract::Function(function) = &mut module.named_declarations[0].contract else {
        unreachable!()
    };
    function.return_type = interface;
    module
        .public_items
        .push(PublicItem::InterfaceTable(Box::new(adapter)));
    table_inventory(module);
    certify(&mut program, &context);
    let verified =
        VerifiedBytecodeProgram::new(program.clone(), &context, &Default::default()).unwrap();
    assert_eq!(verified.native_applications().len(), 1);
    let artifact =
        KbcArtifact::from_verified(verified, Default::default(), &Default::default()).unwrap();
    assert_eq!(
        reverify_source_free(artifact.records(), program.clone(), &context)
            .unwrap()
            .native_applications()
            .len(),
        1
    );
    // Keep the exposed Marker interface and its Bag implementation valid. Only
    // the concrete Bag<i32> declaration's i32: Marker proof disappears.
    program.modules[0].public_items.remove(2);
    table_inventory(&mut program.modules[0]);
    certify(&mut program, &context);
    assert!(matches!(
        reverify_source_free(artifact.records(), program, &context),
        Err(ArtifactValidationError::Bytecode(
            kagari_bytecode::verifier::BytecodeVerificationError::InvalidHostInterface(_)
        ))
    ));
}

#[test]
fn native_application_witness_preserves_open_binder_without_granting_closed_admission() {
    let (context, program) = fixture(true);
    let verified =
        VerifiedBytecodeProgram::new(program.clone(), &context, &Default::default()).unwrap();
    let evidence = &verified.native_applications()[0];
    assert!(!evidence.is_closed());
    assert_eq!(
        evidence.binder(),
        program.modules[0].native_imports[0].generic.as_ref()
    );
    // Even a concrete-looking result in an open import remains contextual.
    let mut concrete_body = program.clone();
    let Ty::NativeObject(mut concrete) = concrete_body.modules[0].native_imports[0]
        .signature
        .result
        .clone()
    else {
        unreachable!()
    };
    concrete.arguments = vec![Ty::Builtin(BuiltinType::I32)];
    let result = Ty::NativeObject(concrete);
    let module = &mut concrete_body.modules[0];
    module.native_imports[0].signature.result = result.clone();
    module.native_declarations[0].function.return_type = result.clone();
    let PublicItem::Function(function) = &mut module.public_items[3] else {
        unreachable!()
    };
    function.return_type = result.clone();
    let NamedContract::Function(function) = &mut module.named_declarations[0].contract else {
        unreachable!()
    };
    function.return_type = result;
    certify(&mut concrete_body, &context);
    let concrete_body =
        VerifiedBytecodeProgram::new(concrete_body, &context, &Default::default()).unwrap();
    assert!(
        concrete_body.native_applications()[0]
            .nominal()
            .arguments
            .iter()
            .all(Ty::is_concrete)
    );
    assert!(!concrete_body.native_applications()[0].is_closed());

    let artifact =
        KbcArtifact::from_verified(verified, Default::default(), &Default::default()).unwrap();
    let mut missing = program;
    missing.modules[0].native_imports[0].requirements.clear();
    missing.modules[0].native_imports[0]
        .generic
        .as_mut()
        .unwrap()
        .bounds
        .clear();
    // Even forging both the import and declaration's authored assumptions cannot
    // justify the independently declared nominal bound.
    missing.modules[0].native_declarations[0]
        .function
        .bounds
        .clear();
    let PublicItem::Function(function) = &mut missing.modules[0].public_items[3] else {
        unreachable!()
    };
    function.bounds.clear();
    let NamedContract::Function(function) = &mut missing.modules[0].named_declarations[0].contract
    else {
        unreachable!()
    };
    function.bounds.clear();
    certify(&mut missing, &context);
    assert!(matches!(
        reverify_source_free(artifact.records(), missing, &context),
        Err(ArtifactValidationError::Bytecode(
            kagari_bytecode::verifier::BytecodeVerificationError::InvalidHostInterface(_)
        ))
    ));
}
