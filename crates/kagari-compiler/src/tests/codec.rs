use bincode::{DefaultOptions, Options};
use kagari_abi::{
    representation::ValueType,
    version::{KAGARI_RUNTIME_ABI_VERSION, KAGARI_RUNTIME_HELPER_ABI_VERSION},
};
use kagari_bytecode::{
    artifact::ArtifactValidationError,
    instruction::{BytecodeInstruction, ConstantOperand},
    program::verified::VerifiedBytecodeProgram,
};
use kagari_common::{
    cancellation::CancellationToken,
    identity::{ModuleIdentity, metadata::scope_record},
};
use kagari_mir::{
    codec::{MIR_FORMAT_VERSION, MIR_MAGIC, MirCodecError, decode_program, encode_program},
    function::{MirModule, MirTemp},
    ids::BlockId,
    instruction::{Constant, Instruction, Terminator},
    program::{ProgramErrorKind, VerifiedMirProgram, verify_program},
    verify::{MirVerificationErrorKind, verify_mir},
};
use std::sync::Arc;

use crate::{
    bytecode::lower_program_to_bytecode,
    native_input::{NativeInputError, verify_native_input},
    source::program::lower_program_to_mir,
    tests::common,
};

fn program(source: &str) -> VerifiedMirProgram {
    common::mir_ok(source)
}

fn forged(root: &ModuleIdentity, modules: &[MirModule]) -> Vec<u8> {
    let cancel = CancellationToken::default();
    let portable = scope_record(&modules.to_vec(), &cancel)
        .unwrap()
        .to_portable(&cancel)
        .unwrap();
    DefaultOptions::new()
        .with_fixint_encoding()
        .with_little_endian()
        .serialize(&(
            MIR_MAGIC,
            MIR_FORMAT_VERSION,
            KAGARI_RUNTIME_ABI_VERSION,
            KAGARI_RUNTIME_HELPER_ABI_VERSION,
            root,
            portable,
        ))
        .unwrap()
}

fn assert_same_bytecode(before: &VerifiedMirProgram, after: &VerifiedMirProgram) {
    assert_eq!(
        bincode::serialize(&lower_program_to_bytecode(before).unwrap()).unwrap(),
        bincode::serialize(&lower_program_to_bytecode(after).unwrap()).unwrap()
    );
}

#[test]
fn portable_program_codec_rebuilds_seals_and_preserves_canonical_lowering() {
    for source in [
        "fn main() -> i32 { if 1 + 2 == 3 { 42 } else { 0 } }",
        "fn main() -> u64 { 18446744073709551615u64 }",
        "fn main() -> f64 { -0.0 }",
        "fn main() -> i32 { var x = 0; while x < 3 { x += 1; } x }",
        "fn main() -> i32 { val a = [1, 2]; a[0] }",
    ] {
        let original = program(source);
        let bytes = encode_program(&original, &Default::default()).unwrap();
        let decoded = decode_program(&bytes, &Default::default()).unwrap();
        assert_eq!(
            bytes,
            encode_program(&decoded, &Default::default()).unwrap()
        );
        assert_same_bytecode(&original, &decoded);
        for (a, b) in original.modules().iter().zip(decoded.modules()) {
            for function in &a.functions {
                let facts = b.analysis(function.id).unwrap();
                assert_eq!(facts.block(function.entry).unwrap().start_offset(), 0);
                assert_eq!(
                    function.debug.source,
                    b.functions[function.id.index()].debug.source
                );
            }
        }
    }
}

#[test]
fn codec_retains_unicode_origins_without_retaining_source_objects() {
    let checked = common::program_ok("fn main() -> i32 {\r\n val text = \"雪😀\"; 7\r\n }");
    let weak = Arc::downgrade(&checked.root().lowered.source);
    let original = lower_program_to_mir(&checked, &Default::default()).unwrap();
    let bytes = encode_program(&original, &Default::default()).unwrap();
    drop(checked);
    assert!(weak.upgrade().is_none());
    let decoded = decode_program(&bytes, &Default::default()).unwrap();
    assert_same_bytecode(&original, &decoded);
}

#[test]
fn codec_rejects_forged_control_flow_instead_of_accepting_serialized_proof() {
    let original = program("fn main() {}");
    let root = original.root().clone();
    let mut raw = original.into_unverified();
    let slot = raw
        .iter()
        .position(|module| module.identity == root)
        .unwrap();
    raw[slot].functions[0].blocks[0].terminator = Some(Terminator::Jump(BlockId::new(999)));
    assert!(matches!(
        decode_program(&forged(&root, &raw), &Default::default()),
        Err(MirCodecError::Verification(_))
    ));
}

#[test]
fn codec_rejects_old_versions_trailing_data_truncation_and_cancelled_work() {
    // Exhaust every truncation offset of a small, source-free dependency graph.
    // The compiled stdlib closure is exercised by the semantic roundtrip tests.
    let root = ModuleIdentity::single_file("wire-root");
    let dependency = ModuleIdentity::single_file("wire-dependency");
    let empty = |identity, dependencies| MirModule {
        native_targets: vec![],
        identity,
        dependencies,
        interface_instances: vec![],
        host_types: vec![],
        structures: vec![],
        enumerations: vec![],
        source_name: String::new(),
        module_slots: vec![],
        abi: Default::default(),
        functions: vec![],
    };
    let original = verify_program(
        root.clone(),
        vec![
            empty(root, vec![dependency.clone()]),
            empty(dependency, vec![]),
        ],
        &Default::default(),
    )
    .unwrap();
    let bytes = encode_program(&original, &Default::default()).unwrap();
    let mut previous = bytes.clone();
    previous[4..6].copy_from_slice(&0u16.to_le_bytes());
    assert!(matches!(
        decode_program(&previous, &Default::default()),
        Err(MirCodecError::Version)
    ));
    for (runtime, helper) in [
        ("obsolete-runtime", KAGARI_RUNTIME_HELPER_ABI_VERSION),
        (KAGARI_RUNTIME_ABI_VERSION, "obsolete-helper"),
    ] {
        let header = DefaultOptions::new()
            .with_fixint_encoding()
            .with_little_endian()
            .serialize(&(MIR_MAGIC, MIR_FORMAT_VERSION, runtime, helper))
            .unwrap();
        assert!(matches!(
            decode_program(&header, &Default::default()),
            Err(MirCodecError::Version)
        ));
    }
    let mut trailing = bytes.clone();
    trailing.push(0);
    assert!(matches!(
        decode_program(&trailing, &Default::default()),
        Err(MirCodecError::Encoding(_))
    ));
    for length in 0..bytes.len() {
        assert!(decode_program(&bytes[..length], &Default::default()).is_err());
    }
    let cancel = CancellationToken::default();
    cancel.cancel();
    assert!(matches!(
        decode_program(&bytes, &cancel),
        Err(MirCodecError::Cancelled)
    ));
    assert!(matches!(
        encode_program(&original, &cancel),
        Err(MirCodecError::Cancelled)
    ));
}

#[test]
fn codec_rejects_impossible_module_counts_before_element_decoding() {
    let root = ModuleIdentity::single_file("wire");
    let mut prefix = DefaultOptions::new()
        .with_fixint_encoding()
        .with_little_endian()
        .serialize(&(
            MIR_MAGIC,
            MIR_FORMAT_VERSION,
            KAGARI_RUNTIME_ABI_VERSION,
            KAGARI_RUNTIME_HELPER_ABI_VERSION,
            root,
        ))
        .unwrap();
    prefix.extend_from_slice(&u64::MAX.to_le_bytes());
    let error = decode_program(&prefix, &Default::default()).unwrap_err();
    assert!(
        matches!(error, MirCodecError::Encoding(ref message) if message.contains("module count limit exceeded"))
    );
}

#[test]
fn whole_program_analysis_budget_cannot_be_reset_by_splitting_modules() {
    let original = program("fn main() {}");
    let root = original.root().clone();
    let mut dependencies = original.into_unverified();
    let slot = dependencies
        .iter()
        .position(|module| module.identity == root)
        .unwrap();
    let mut template = dependencies.remove(slot);
    let function = &mut template.functions[0];
    function.temps = vec![
        MirTemp {
            ty: ValueType::Unit
        };
        32_768
    ];
    let block = &mut function.blocks[0];
    block.instructions = vec![
        Instruction::LoadConst {
            dst: kagari_mir::instruction::MirValue {
                temp: kagari_mir::ids::TempId::new(0),
                ty: ValueType::Unit
            },
            constant: kagari_mir::instruction::Constant::Unit
        };
        256
    ];
    block.instruction_spans = vec![Default::default(); 256];
    block.instruction_scopes = vec![0; 256];
    block.terminator = Some(Terminator::Return(None));
    verify_mir(template.clone(), &Default::default()).unwrap();
    let mut modules = (0..24)
        .map(|index| {
            let mut module = template.clone();
            module.identity = ModuleIdentity::single_file(format!("member{index}"));
            module.functions[0].instance.declaration.module = module.identity.clone();
            module
        })
        .collect::<Vec<_>>();
    let linked: Vec<_> = modules
        .iter()
        .skip(1)
        .map(|module| module.identity.clone())
        .collect();
    modules[0].dependencies.extend(linked);
    let root = modules[0].identity.clone();
    modules.extend(dependencies);
    let error = decode_program(&forged(&root, &modules), &Default::default()).unwrap_err();
    assert!(
        matches!(error, MirCodecError::Verification(error) if matches!(error.kind,
            ProgramErrorKind::Verification(ref error) if matches!(error.kind,
                MirVerificationErrorKind::Limit { resource: "MIR analysis state bytes", .. }
            )
        ))
    );
}

#[test]
fn native_preparation_requires_canonical_semantics_not_independent_valid_payloads() {
    let original = program("fn main() -> i32 { 42 }");
    let other = program("fn main() -> i32 { 43 }");
    let wire = encode_program(&original, &Default::default()).unwrap();
    let bytecode =
        VerifiedBytecodeProgram::new(lower_program_to_bytecode(&original).unwrap()).unwrap();
    let prepared = verify_native_input(&wire, &bytecode, &Default::default()).unwrap();
    assert_same_bytecode(&original, &prepared);
    let different_bytecode =
        VerifiedBytecodeProgram::new(lower_program_to_bytecode(&other).unwrap()).unwrap();
    // Both programs are independently valid and have identical function signatures.
    assert!(matches!(
        verify_native_input(&wire, &different_bytecode, &Default::default()),
        Err(NativeInputError::Mismatch)
    ));
    let mut changed_origin = bytecode.to_unverified(&Default::default()).unwrap();
    changed_origin.modules[changed_origin.root.index()]
        .source_name
        .push_str("-changed");
    let changed_origin = VerifiedBytecodeProgram::new(changed_origin).unwrap();
    assert!(matches!(
        verify_native_input(&wire, &changed_origin, &Default::default()),
        Err(NativeInputError::Mismatch)
    ));
    let mut forged = bytecode.to_unverified(&Default::default()).unwrap();
    forged.modules[forged.root.index()].functions[0]
        .instructions
        .clear();
    assert!(matches!(
        VerifiedBytecodeProgram::new(forged),
        Err(ArtifactValidationError::Bytecode(_))
    ));
    let cancel = CancellationToken::default();
    cancel.cancel();
    assert!(matches!(
        verify_native_input(&wire, &bytecode, &cancel),
        Err(NativeInputError::Cancelled)
    ));
}

#[test]
fn codec_preserves_float_bits_and_constant_pool_identity() {
    for (ty, pairs) in [
        (
            "f32",
            [
                (0x7fc04242, 0x7fc04243),
                (0, 0x80000000),
                (0x7fc04242, 0x7fc04242),
            ],
        ),
        (
            "f64",
            [
                (0x7ff8000000004242, 0x7ff8000000004243),
                (0, 0x8000000000000000),
                (0x7ff8000000004242, 0x7ff8000000004242),
            ],
        ),
    ] {
        for (first, second) in pairs {
            let original = program(&format!(
                "fn main() -> {ty} {{ val a: {ty} = 0.0; val b: {ty} = 1.0; a + b }}"
            ));
            let root = original.root().clone();
            let mut modules = original.into_unverified();
            let mut bits = [first, second].into_iter();
            let slot = modules
                .iter()
                .position(|module| module.identity == root)
                .unwrap();
            for instruction in &mut modules[slot].functions[0].blocks[0].instructions {
                match instruction {
                    Instruction::LoadConst {
                        constant: Constant::F32(value),
                        ..
                    } => {
                        *value = f32::from_bits(bits.next().unwrap() as u32);
                    }
                    Instruction::LoadConst {
                        constant: Constant::F64(value),
                        ..
                    } => {
                        *value = f64::from_bits(bits.next().unwrap());
                    }
                    _ => {}
                }
            }
            assert!(bits.next().is_none());
            let original = verify_program(root, modules, &Default::default()).unwrap();
            let wire = encode_program(&original, &Default::default()).unwrap();
            let bytecode =
                VerifiedBytecodeProgram::new(lower_program_to_bytecode(&original).unwrap())
                    .unwrap();
            assert_eq!(
                bytecode.program().modules[bytecode.program().root.index()]
                    .constants
                    .len(),
                if first == second { 1 } else { 2 }
            );
            let decoded = verify_native_input(&wire, &bytecode, &Default::default()).unwrap();
            assert_same_bytecode(&original, &decoded);
            let mut changed = bytecode.to_unverified(&Default::default()).unwrap();
            let module = &mut changed.modules[changed.root.index()];
            // Keep pool and instruction operands consistent so the modified graph
            // is independently valid, including NaN payloads and signed zero.
            let operands = module.constants.iter_mut().chain(
                module
                    .functions
                    .iter_mut()
                    .flat_map(|function| &mut function.instructions)
                    .filter_map(|instruction| match instruction {
                        BytecodeInstruction::LoadConst { constant, .. } => Some(constant),
                        _ => None,
                    }),
            );
            for constant in operands {
                match constant {
                    ConstantOperand::F32(value) => *value = f32::from_bits(value.to_bits() ^ 1),
                    ConstantOperand::F64(value) => *value = f64::from_bits(value.to_bits() ^ 1),
                    constant => panic!("expected float constant, got {constant:?}"),
                }
            }
            let changed = VerifiedBytecodeProgram::new(changed).unwrap();
            assert!(matches!(
                verify_native_input(&wire, &changed, &Default::default()),
                Err(NativeInputError::Mismatch)
            ));
        }
    }
}

#[test]
fn artifact_integrity_does_not_substitute_for_native_correspondence() {
    use kagari_bytecode::{
        artifact::{ArtifactBuildOptions, KbcArtifact},
        native_input::PortableMir,
    };

    let first = program("fn main() -> i32 { 42 }");
    let second = program("fn main() -> i32 { 43 }");
    for (mir, matches) in [(&first, true), (&second, false)] {
        let artifact = KbcArtifact::from_program(
            lower_program_to_bytecode(&first).unwrap(),
            ArtifactBuildOptions {
                portable_mir: Some(PortableMir {
                    bytes: encode_program(mir, &Default::default()).unwrap(),
                }),
                ..Default::default()
            },
        )
        .unwrap();
        let decoded = KbcArtifact::from_bytes(&artifact.to_bytes().unwrap()).unwrap();
        let decoded = decoded.into_verified(&Default::default()).unwrap();
        let result = verify_native_input(
            &decoded.portable_mir().unwrap().bytes,
            decoded.bytecode(),
            &Default::default(),
        );
        if matches {
            assert_same_bytecode(&first, &result.unwrap());
        } else {
            assert!(matches!(result, Err(NativeInputError::Mismatch)));
        }
    }
}
