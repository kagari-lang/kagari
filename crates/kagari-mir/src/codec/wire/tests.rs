use super::{ProgramRecord, function::MirModuleRecord};
use crate::{
    codec::{Header, MirCodecError, decode_program, encode_program, options},
    debug::{MirFunctionDebugMetadata, MirLexicalScope},
    function::{BasicBlock, MirFunction, MirModule, MirTemp},
    ids::{BlockId, InstanceId, TempId},
    instruction::{Constant, Instruction, MirValue, Terminator},
    program::{ProgramErrorKind, records::MirProgram, verify_program},
    verify::{MirVerificationErrorKind, verify_mir},
};
use bincode::Options;
use kagari_abi::representation::ValueType;
use kagari_common::identity::{
    DefinitionKind, ModuleIdentity, map::DefinitionContext, metadata::DefinitionMetadata,
    scoped::DefinitionReferences,
};
use kagari_contract::{
    exports::wire::ProgramExportsRecord,
    layout::{EnumLayout, EnumVariantLayout},
    numeric::NumericConversion,
    operations::BinaryOp,
    statics::{StaticContract, StaticDescriptor},
    types::{ConcreteFunctionIdentity, PublicItem},
};
use kagari_types::{
    declaration::{TypeDeclaration, TypeDef, TypeDefKind, VariantDef},
    scalar::BuiltinType,
    ty::{GenericParam, Ty},
    visibility::Visibility,
};
use std::collections::HashSet;

fn module(context: &DefinitionContext, name: &str) -> MirModule {
    let identity = ModuleIdentity::single_file(name);
    let root = context.intern_root(&identity).unwrap();
    let declaration = context
        .intern_child(root, DefinitionKind::Function, "main", 0)
        .unwrap();
    MirModule {
        native_targets: vec![],
        interface_instances: vec![],
        external_types: vec![],
        dependencies: vec![],
        structures: vec![],
        enumerations: vec![],
        identity,
        source_name: String::new(),
        module_slots: vec![],
        abi: Default::default(),
        functions: vec![MirFunction {
            semantic: Default::default(),
            id: InstanceId::new(0),
            instance: ConcreteFunctionIdentity {
                declaration,
                arguments: vec![],
            },
            name: "main".into(),
            params: vec![],
            return_type: ValueType::Unit,
            locals: vec![],
            temps: vec![],
            blocks: vec![BasicBlock {
                instructions: vec![],
                instruction_spans: vec![],
                instruction_scopes: vec![],
                terminator: Some(Terminator::Return(None)),
                terminator_span: Some(Default::default()),
                terminator_scope: Some(0),
            }],
            entry: BlockId::new(0),
            effects: Default::default(),
            debug: MirFunctionDebugMetadata {
                lexical_scopes: vec![MirLexicalScope {
                    parent: None,
                    local: None,
                }],
                ..Default::default()
            },
        }],
    }
}

// Test-only access to the real file records permits malformed semantic facts
// without exposing an unverified encoder through the production codec API.
fn forged(context: &DefinitionContext, modules: &[MirModule]) -> Vec<u8> {
    let cancel = Default::default();
    let mut modules = modules.to_vec();
    // Malformed extraction inputs are intentional on this unverified encoder route.
    // Successful fixtures carry real evidence; malformed ones still cross all
    // ordinary executable validation before the mandatory evidence boundary.
    let update_evidence = crate::program::updates::fixtures::evidence(&mut modules, context)
        .unwrap_or_else(|_| crate::program::updates::empty());
    let mut references = HashSet::new();
    modules
        .to_vec()
        .visit_definition_ids(
            &mut |id| {
                references.insert(*id);
                Ok(())
            },
            &cancel,
        )
        .unwrap();
    let encoding = context.snapshot().encode(references).unwrap();
    let record = ProgramRecord {
        root: modules[0].identity.clone(),
        modules: modules
            .iter()
            .map(|module| MirModuleRecord::encode(module, &encoding, &cancel).unwrap())
            .collect(),
        exports: ProgramExportsRecord::encode(&Default::default(), &encoding, &cancel).unwrap(),
        update_evidence: kagari_contract::updates::wire::UpdateEvidenceRecord::encode(
            &update_evidence,
            &encoding,
            &mut kagari_contract::updates::limits::Budget::new(Default::default(), &cancel)
                .unwrap(),
        )
        .unwrap(),
        definitions: encoding.table,
    };
    options().serialize(&(Header::current(), record)).unwrap()
}

#[test]
fn private_types_codec_checks_enum_payloads_without_exporting_the_declaration() {
    let context = DefinitionContext::new().unwrap();
    let mut module = module(&context, "private-enum");
    module.functions.clear();
    let root = context.intern_root(&module.identity).unwrap();
    let choice = context
        .intern_child(root, DefinitionKind::Enum, "Choice", 0)
        .unwrap();
    let variant = context
        .intern_child(choice, DefinitionKind::Variant, "Value", 0)
        .unwrap();
    let parameter = GenericParam {
        owner: choice,
        position: 0,
    };
    module.abi.private_types.push(TypeDeclaration {
        declaration: choice,
        definition: TypeDef {
            name: "Choice".into(),
            kind: TypeDefKind::Enum,
            generic_params: vec![parameter.clone()],
            bounds: vec![],
            fields: vec![],
            variants: vec![VariantDef {
                name: "Value".into(),
                reports_failure: false,
                payload: vec![parameter.as_type()],
            }],
        },
    });
    module.enumerations.push(EnumLayout {
        declaration: choice,
        arguments: vec![parameter.as_type()],
        variants: vec![EnumVariantLayout {
            declaration: variant,
            reports_failure: false,
            payload: vec![parameter.as_type()],
        }],
    });
    let bytes = forged(&context, &[module.clone()]);
    let decoded = decode_program(&bytes, &Default::default()).unwrap();
    let owner = &decoded.modules()[0];
    assert!(owner.abi.public_items.is_empty());
    assert!(owner.external_types.is_empty());
    assert_eq!(owner.abi.private_types.len(), 1);
    let definition = &owner.abi.private_types[0];
    assert_eq!(
        definition.definition.generic_params[0].owner,
        definition.declaration
    );
    assert_eq!(
        definition.definition.variants[0].payload[0],
        owner.enumerations[0].variants[0].payload[0]
    );
    assert_eq!(
        encode_program(&decoded, &Default::default()).unwrap(),
        bytes
    );
    for corruption in 0..4 {
        let mut invalid = module.clone();
        match corruption {
            0 => {
                invalid.abi.private_types[0].definition.variants[0].payload[0] =
                    Ty::Builtin(BuiltinType::Bool)
            }
            1 => invalid.abi.private_types[0].definition.variants[0].reports_failure = true,
            2 => invalid.abi.private_types[0].definition.variants[0].name = "Other".into(),
            3 => {
                invalid.enumerations[0].arguments[0] = Ty::Parameter {
                    owner: root,
                    position: 0,
                }
            }
            _ => unreachable!(),
        }
        assert!(
            decode_program(&forged(&context, &[invalid]), &Default::default()).is_err(),
            "corruption {corruption}"
        );
    }
}

#[test]
fn char_codec_roundtrips_scalar_endpoints_and_rejects_invalid_payloads() {
    let context = DefinitionContext::new().unwrap();
    let mut module = module(&context, "char");
    let function = &mut module.functions[0];
    function.return_type = ValueType::Char;
    function.semantic.result = Some(Ty::Builtin(BuiltinType::Char));
    function
        .semantic
        .registers
        .insert(0, Ty::Builtin(BuiltinType::Char));
    function.temps.push(MirTemp {
        ty: ValueType::Char,
    });
    let value = MirValue {
        temp: TempId::new(0),
        ty: ValueType::Char,
    };
    let block = &mut function.blocks[0];
    block.instructions.push(Instruction::LoadConst {
        dst: value,
        constant: Constant::Char('\0'),
    });
    block.instruction_spans.push(Default::default());
    block.instruction_scopes.push(0);
    block.terminator = Some(Terminator::Return(Some(value)));
    for scalar in ['\0', '\u{d7ff}', '\u{e000}', '🙂', '\u{10ffff}'] {
        module.functions[0].blocks[0].instructions[0] = Instruction::LoadConst {
            dst: value,
            constant: Constant::Char(scalar),
        };
        let bytes = forged(&context, &[module.clone()]);
        let decoded = decode_program(&bytes, &Default::default()).unwrap();
        assert!(matches!(
            &decoded.modules()[0].functions[0].blocks[0].instructions[0],
            Instruction::LoadConst { constant: Constant::Char(found), .. } if *found == scalar
        ));
        assert_eq!(
            bytes,
            encode_program(&decoded, &Default::default()).unwrap()
        );
    }
    // Serde char uses canonical UTF-8, not an integer with unchecked high bits.
    let bytes = forged(&context, &[module.clone()]);
    let needle = options().serialize(&Constant::Char('\u{10ffff}')).unwrap();
    let positions: Vec<_> = bytes
        .windows(needle.len())
        .enumerate()
        .filter_map(|(index, window)| (window == needle).then_some(index))
        .collect();
    // The executable and its canonical/actual evidence each retain the constant.
    assert_eq!(positions.len(), 3);
    for payload in [
        &[0xed, 0xa0, 0x80][..],         // U+D800 surrogate
        &[0xf4, 0x90, 0x80, 0x80],       // U+110000
        &[0xc0, 0x80],                   // overlong NUL
        &[0xf8, 0x80, 0x80, 0x80, 0x80], // oversized scalar
    ] {
        // Modules precede evidence in ProgramRecord: mutate the executable char.
        let mut invalid = bytes.clone();
        let start = positions[0] + 4; // appended enum tag, followed by UTF-8
        invalid.splice(start..positions[0] + needle.len(), payload.iter().copied());
        assert!(matches!(
            decode_program(&invalid, &Default::default()),
            Err(MirCodecError::Encoding(_))
        ));
    }
    for invalid_operation in [
        Instruction::Binary {
            dst: value,
            op: BinaryOp::Add,
            lhs: value,
            rhs: value,
        },
        Instruction::Convert {
            dst: value,
            src: value,
            conversion: NumericConversion {
                source: BuiltinType::Char,
                target: BuiltinType::U32,
            },
        },
        Instruction::Convert {
            dst: value,
            src: value,
            conversion: NumericConversion {
                source: BuiltinType::U32,
                target: BuiltinType::Char,
            },
        },
    ] {
        let mut invalid = module.clone();
        let block = &mut invalid.functions[0].blocks[0];
        block.instructions.push(invalid_operation);
        block.instruction_spans.push(Default::default());
        block.instruction_scopes.push(0);
        assert!(matches!(
            decode_program(&forged(&context, &[invalid]), &Default::default()),
            Err(MirCodecError::Verification(_))
        ));
    }
    // A valid scalar in the wrong physical domain must also reject in memory.
    module.functions[0].blocks[0].instructions[0] = Instruction::LoadConst {
        dst: value,
        constant: Constant::I64(65),
    };
    assert!(
        verify_mir(
            DefinitionMetadata::checked(context.snapshot(), module, &Default::default()).unwrap(),
            &Default::default(),
        )
        .is_err()
    );
}

#[test]
fn codec_preserves_static_identity_supplier_and_hidden_initializer_contract() {
    let context = DefinitionContext::new().unwrap();
    let mut owner = module(&context, "owner");
    let root = context.intern_root(&owner.identity).unwrap();
    let contract = StaticContract {
        declaration: context
            .intern_child(root, DefinitionKind::Static, "state", 0)
            .unwrap(),
        supplier: owner.identity.clone(),
        name: "state".into(),
        ty: Ty::Builtin(BuiltinType::Unit),
        mutable: false,
        visibility: Visibility::Public,
    };
    owner.functions[0].semantic.result = Some(contract.ty.clone());
    owner.module_slots.push(StaticDescriptor {
        contract: contract.clone(),
        initializer: InstanceId::new(0),
    });
    owner.abi.public_items.push(PublicItem::Static(contract));
    let mut importer = module(&context, "importer");
    importer.dependencies.push(owner.identity.clone());
    importer.module_slots = owner.module_slots.clone();
    owner.dependencies.push(importer.identity.clone());
    verify_mir(
        DefinitionMetadata::checked(context.snapshot(), owner.clone(), &Default::default())
            .unwrap(),
        &Default::default(),
    )
    .unwrap();
    let modules = vec![owner, importer];
    let decoded = decode_program(&forged(&context, &modules), &Default::default()).unwrap();
    let records = decoded.into_unverified();
    let descriptor = &records.records().modules[0].module_slots[0];
    assert_eq!(descriptor.contract.supplier, modules[0].identity);
    assert_eq!(
        records
            .definitions()
            .resolve(descriptor.contract.declaration)
            .unwrap()
            .to_path(),
        context
            .snapshot()
            .resolve(modules[0].module_slots[0].contract.declaration)
            .unwrap()
            .to_path()
    );
    assert_eq!(descriptor, &records.records().modules[1].module_slots[0]);
    for mutation in 0..4 {
        let mut invalid = modules.clone();
        match mutation {
            0 => invalid[0].module_slots[0].initializer = InstanceId::new(999),
            1 => invalid[0].functions[0].semantic.result = None,
            2 => invalid[1].dependencies.clear(),
            _ => invalid[1].module_slots[0].contract.mutable = true,
        }
        assert!(
            decode_program(&forged(&context, &invalid), &Default::default()).is_err(),
            "static contract mutation {mutation}"
        );
        if mutation < 2 {
            assert!(
                verify_mir(
                    DefinitionMetadata::checked(
                        context.snapshot(),
                        invalid[0].clone(),
                        &Default::default()
                    )
                    .unwrap(),
                    &Default::default()
                )
                .is_err()
            );
        }
    }
}

#[test]
fn codec_rejects_forged_control_flow_instead_of_accepting_serialized_proof() {
    let context = DefinitionContext::new().unwrap();
    let mut module = module(&context, "root");
    verify_program(
        crate::program::updates::fixtures::certify(
            MirProgram {
                update_evidence: crate::program::updates::empty(),
                root: module.identity.clone(),
                modules: vec![module.clone()],
                exports: Default::default(),
            },
            &context,
        ),
        &context,
        &Default::default(),
    )
    .unwrap();
    module.functions[0].blocks[0].terminator = Some(Terminator::Jump(BlockId::new(999)));
    assert!(matches!(
        decode_program(&forged(&context, &[module]), &Default::default()),
        Err(MirCodecError::Verification(error)) if matches!(error.kind,
            ProgramErrorKind::Verification(ref error)
                if error.kind == MirVerificationErrorKind::InvalidBlock(BlockId::new(999)))
    ));
}

#[test]
fn whole_program_analysis_budget_cannot_be_reset_by_splitting_modules() {
    let context = DefinitionContext::new().unwrap();
    let mut modules = (0..24)
        .map(|index| {
            let mut module = module(&context, &format!("member{index}"));
            let function = &mut module.functions[0];
            // Keep each canonical layout operand within its evidence byte limit;
            // grow point count instead to exhaust the aggregate analysis matrix.
            function.temps = vec![
                MirTemp {
                    ty: ValueType::Unit
                };
                512
            ];
            let block = &mut function.blocks[0];
            block.instructions = vec![
                Instruction::LoadConst {
                    dst: MirValue {
                        temp: TempId::new(0),
                        ty: ValueType::Unit
                    },
                    constant: Constant::Unit,
                };
                16_384
            ];
            block.instruction_spans = vec![Default::default(); 16_384];
            block.instruction_scopes = vec![0; 16_384];
            module
        })
        .collect::<Vec<_>>();
    verify_program(
        crate::program::updates::fixtures::certify(
            MirProgram {
                update_evidence: crate::program::updates::empty(),
                root: modules[0].identity.clone(),
                modules: vec![modules[0].clone()],
                exports: Default::default(),
            },
            &context,
        ),
        &context,
        &Default::default(),
    )
    .unwrap();
    modules[0].dependencies = modules
        .iter()
        .skip(1)
        .map(|module| module.identity.clone())
        .collect();
    let error = decode_program(&forged(&context, &modules), &Default::default()).unwrap_err();
    assert!(
        matches!(error, MirCodecError::Verification(error) if matches!(error.kind,
            ProgramErrorKind::Verification(ref error) if matches!(error.kind,
                MirVerificationErrorKind::Limit { resource: "MIR analysis state bytes", .. }
            )
        ))
    );
}
