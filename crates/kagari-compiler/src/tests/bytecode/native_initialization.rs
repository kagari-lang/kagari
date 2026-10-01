use crate::tests::common;
use bincode::{DefaultOptions, Options};
use kagari_abi::{
    callable::{EngineNativeBinding, NativeCall},
    scalar::BuiltinType,
    standard::RuntimePrimitive,
    types::AbiType,
};
use kagari_bytecode::{BytecodeInstruction, CallTarget, KbcArtifact, verify_program};
use kagari_common::collection::CollectionAccess;

#[test]
fn array_initializers_reject_forged_callback_and_storage_contracts() {
    for (item, expression) in [
        ("usize", "i"),
        ("Cell", "Cell{value:i}"),
        ("(usize,ArrayList<usize>)", "(i,[i])"),
    ] {
        let program = common::bytecode_ok(&format!(
            "struct Cell{{var value:usize}}fn build<T>(count:usize,initializer:fn(usize)->T)->ArrayList<T>{{ArrayList::from_fn(count,initializer)}}fn main(){{val values:ArrayList<{item}> =build(3usize,|i|{expression});}}"
        ));
        let root = program.root.index();
        let import = program.modules[root]
            .native_imports
            .iter()
            .position(|import| {
                import.binding == EngineNativeBinding::Intrinsic(RuntimePrimitive::ArrayListFromFn)
            })
            .unwrap();
        let artifact = KbcArtifact::from_program(program, Default::default()).unwrap();
        for mutation in 0..10 {
            let mut forged = artifact.clone();
            let contract = &mut forged.program.modules[root].native_imports[import];
            match mutation {
                0 => contract.signature.params[0] = AbiType::Builtin(BuiltinType::I32),
                1 => {
                    contract.signature.params.pop().unwrap();
                }
                2 => contract
                    .signature
                    .params
                    .push(AbiType::Builtin(BuiltinType::USize)),
                3 => contract.signature.params[1] = AbiType::Builtin(BuiltinType::USize),
                4..=6 => {
                    let AbiType::Function { params, result } = &mut contract.signature.params[1]
                    else {
                        panic!()
                    };
                    match mutation {
                        4 => params.clear(),
                        5 => params[0] = AbiType::Builtin(BuiltinType::I32),
                        _ => **result = AbiType::Builtin(BuiltinType::Bool),
                    }
                }
                7 => {
                    let AbiType::Array(_, access) = &mut contract.signature.result else {
                        panic!()
                    };
                    *access = CollectionAccess::ReadOnly;
                }
                8 => {
                    contract.signature.result = AbiType::Array(
                        Box::new(AbiType::Builtin(BuiltinType::Bool)),
                        CollectionAccess::Mutable,
                    )
                }
                _ => {
                    let mut replaced = false;
                    for function in &mut forged.program.modules[root].functions {
                        for instruction in &mut function.instructions {
                            if let BytecodeInstruction::Call { callee, .. } = instruction
                                && matches!(callee,CallTarget::Native(id) if id.index()==import)
                            {
                                *callee =
                                    CallTarget::RuntimePrimitive(RuntimePrimitive::ArrayListFromFn);
                                replaced = true;
                            }
                        }
                    }
                    assert!(replaced);
                }
            }
            assert!(
                verify_program(&forged.program).is_err(),
                "{item} {mutation}"
            );
            let bytes = DefaultOptions::new()
                .with_fixint_encoding()
                .with_little_endian()
                .serialize(&forged)
                .unwrap();
            assert!(
                !KbcArtifact::from_bytes(&bytes)
                    .is_ok_and(|decoded| decoded.validate_for_loader(&Default::default()).is_ok()),
                "encoded {item} {mutation}"
            );
        }
    }
}
