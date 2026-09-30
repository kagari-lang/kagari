use crate::tests::common;
use bincode::{DefaultOptions, Options};
use kagari_abi::{
    callable::EngineNativeBinding,
    native_import::{NativeWitness, NativeWitnessImplementation},
    scalar::BuiltinType,
    standard::{StandardIntrinsic, intrinsic, traits::StandardTrait},
    types::{AbiType, ConstraintAbi, GenericBoundAbi},
};
use kagari_bytecode::{KbcArtifact, verify_program};

#[test]
fn string_iterator_calls_reject_forged_constructor_contracts() {
    let mut checked = 0;
    for (method, args, operation) in [
        ("bytes", "", StandardIntrinsic::StringBytes),
        ("char_indices", "", StandardIntrinsic::StringCharIndices),
        ("split", "\",\"", StandardIntrinsic::StringSplit),
        ("splitn", "2usize,\",\"", StandardIntrinsic::StringSplitN),
        (
            "split_whitespace",
            "",
            StandardIntrinsic::StringSplitWhitespace,
        ),
        ("lines", "", StandardIntrinsic::StringLines),
    ] {
        let program = common::bytecode_ok(&format!(
            "fn main(){{val output=\"é😀,x\".{method}({args});}}"
        ));
        let root = program.root.index();
        let import = program.modules[root]
            .engine_imports
            .iter()
            .position(|i| i.binding == EngineNativeBinding::Intrinsic(operation))
            .unwrap();
        let artifact = KbcArtifact::from_program(program, Default::default()).unwrap();
        for mutation in 0..12 {
            let mut forged = artifact.clone();
            let contract = &mut forged.program.modules[root].engine_imports[import];
            match mutation {
                0 => contract.binding_version -= 1,
                1 => contract.signature.params.clear(),
                2 => contract.signature.params[0] = AbiType::Builtin(BuiltinType::Bool),
                3 => contract
                    .signature
                    .params
                    .push(AbiType::Builtin(BuiltinType::String)),
                4 => {
                    contract.signature.result =
                        AbiType::Iter(Box::new(AbiType::Builtin(BuiltinType::Bool)))
                }
                5 => contract.signature.result = AbiType::Builtin(BuiltinType::Never),
                6 => contract
                    .instance
                    .arguments
                    .push(AbiType::Builtin(BuiltinType::String)),
                7 => {
                    contract.instance.declaration = {
                        let mut declaration = contract.instance.declaration.clone();
                        declaration.path.last_mut().unwrap().name = "forged".into();
                        declaration
                    }
                }
                8 => contract.requirements.push(GenericBoundAbi {
                    ty: AbiType::Builtin(BuiltinType::String),
                    constraints: vec![ConstraintAbi::Trait(intrinsic::applied(
                        StandardTrait::Eq,
                        vec![],
                    ))],
                }),
                9 => {
                    let binding = if operation == StandardIntrinsic::StringLines {
                        StandardIntrinsic::StringBytes
                    } else {
                        StandardIntrinsic::StringLines
                    };
                    contract.binding = EngineNativeBinding::Intrinsic(binding);
                }
                10 => {
                    contract.signature.params[0] =
                        AbiType::Iter(Box::new(AbiType::Builtin(BuiltinType::String)))
                }
                _ => contract.witnesses.push(NativeWitness {
                    receiver: AbiType::Builtin(BuiltinType::String),
                    interface: intrinsic::applied(StandardTrait::Eq, vec![]),
                    implementation: NativeWitnessImplementation::Primitive,
                    methods: vec![],
                }),
            }
            let label = format!("{method} {mutation}");
            assert!(verify_program(&forged.program).is_err(), "{label}");
            let bytes = DefaultOptions::new()
                .with_fixint_encoding()
                .with_little_endian()
                .serialize(&forged)
                .unwrap();
            assert!(
                !KbcArtifact::from_bytes(&bytes)
                    .is_ok_and(|decoded| decoded.validate_for_loader(&Default::default()).is_ok()),
                "encoded {label}"
            );
            checked += 1;
        }
    }
    assert_eq!(checked, 72);
}
