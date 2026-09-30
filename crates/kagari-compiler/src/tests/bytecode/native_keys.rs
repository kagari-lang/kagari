use crate::tests::common;
use bincode::{DefaultOptions, Options};
use kagari_abi::{
    callable::{EngineNativeBinding, NativeCall},
    native_import::NativeWitnessImplementation,
    scalar::BuiltinType,
    standard::{StandardIntrinsic, traits::StandardTrait},
    types::AbiType,
};
use kagari_bytecode::{BytecodeInstruction, CallTarget, KbcArtifact, verify_program};
use kagari_common::collection::CollectionAccess;
const OPERATIONS: &[StandardIntrinsic] = &[
    StandardIntrinsic::MapGet,
    StandardIntrinsic::MapContainsKey,
    StandardIntrinsic::MapInsert,
    StandardIntrinsic::MapRemove,
    StandardIntrinsic::SetContains,
    StandardIntrinsic::SetInsert,
    StandardIntrinsic::SetRemove,
    StandardIntrinsic::MapGetOrInsertWith,
    StandardIntrinsic::MapUpdate,
];
fn reject(artifact: &KbcArtifact, label: &str) {
    assert!(verify_program(&artifact.program).is_err(), "{label}");
    let bytes = DefaultOptions::new()
        .with_fixint_encoding()
        .with_little_endian()
        .serialize(artifact)
        .unwrap();
    assert!(
        !KbcArtifact::from_bytes(&bytes)
            .is_ok_and(|decoded| decoded.validate_for_loader(&Default::default()).is_ok()),
        "encoded {label}"
    );
}
fn source(operation: StandardIntrinsic, shape: &str) -> String {
    let (item, value) = match shape {
        "scalar" => ("i32", "1"),
        "nominal" => ("Key<i32>", "Key{id:1}"),
        "tuple" => ("(Key<i32>,i32)", "(Key{id:1},0)"),
        "option" => ("Option<Key<i32>>", "Some(Key{id:1})"),
        _ => ("Choice<Key<i32>>", "Choice::Item(Key{id:1})"),
    };
    let set = matches!(
        operation,
        StandardIntrinsic::SetContains
            | StandardIntrinsic::SetInsert
            | StandardIntrinsic::SetRemove
    );
    let storage = if set {
        format!("LinkedHashSet<{item}>")
    } else {
        format!("LinkedHashMap<{item},i32>")
    };
    let constructor = if set {
        "LinkedHashSet::new()"
    } else {
        "LinkedHashMap::new()"
    };
    let action = match operation {
        StandardIntrinsic::MapGet => "items.get(key);",
        StandardIntrinsic::MapContainsKey => "items.contains_key(key);",
        StandardIntrinsic::MapInsert => "items.insert(key,42);",
        StandardIntrinsic::MapRemove => "items.remove(key);",
        StandardIntrinsic::SetContains => "items.contains(key);",
        StandardIntrinsic::SetInsert => "items.insert(key);",
        StandardIntrinsic::SetRemove => "items.remove(key);",
        StandardIntrinsic::MapGetOrInsertWith => "items.get_or_insert_with(key,||42);",
        StandardIntrinsic::MapUpdate => "items.update(key,|previous|previous.unwrap_or(0)+42);",
        _ => panic!(),
    };
    format!(
        r#"
struct Key<T>{{val id:T}}
impl<T:PartialEq> PartialEq for Key<T>{{fn eq(self,other:Self)->bool{{self.id==other.id}}}}
impl<T:Eq> Eq for Key<T>{{}}
impl<T:Eq+Hash> Hash for Key<T>{{fn hash(self)->i64{{0i64}}}}
enum Choice<T>{{Empty,Item(T)}}
fn main()->i32{{val items:{storage} ={constructor};val key:{item} ={value};{action}42}}
"#
    )
}
#[test]
fn key_imports_reject_forged_storage_arguments_authority_and_selected_methods() {
    let mut checked = 0;
    for operation in OPERATIONS {
        for shape in ["scalar", "nominal"] {
            let program = common::bytecode_ok(&source(*operation, shape));
            let root = program.root.index();
            let binding = EngineNativeBinding::Intrinsic(*operation);
            let import = program.modules[root]
                .engine_imports
                .iter()
                .position(|contract| contract.binding == binding)
                .unwrap();
            let artifact = KbcArtifact::from_program(program, Default::default()).unwrap();
            for mutation in 0..10 {
                let mut forged = artifact.clone();
                let module = &mut forged.program.modules[root];
                let contract = &mut module.engine_imports[import];
                match mutation {
                    0 => {
                        contract.signature.params[0] = AbiType::Array(
                            Box::new(AbiType::Builtin(BuiltinType::I32)),
                            CollectionAccess::Mutable,
                        )
                    }
                    1 => contract.signature.params[1] = AbiType::Builtin(BuiltinType::Bool),
                    2 => contract.signature.result = AbiType::Builtin(BuiltinType::Never),
                    3 => {
                        contract
                            .signature
                            .params
                            .push(AbiType::Builtin(BuiltinType::Bool));
                    }
                    4 => contract.requirements.clear(),
                    5 => contract.witnesses.clear(),
                    6 => contract
                        .instance
                        .arguments
                        .push(AbiType::Builtin(BuiltinType::Bool)),
                    7 => contract.binding_version -= 1,
                    8 => {
                        contract.binding =
                            EngineNativeBinding::Intrinsic(StandardIntrinsic::ArrayDedup);
                    }
                    9 => {
                        let mut changed = false;
                        for function in &mut module.functions {
                            for instruction in &mut function.instructions {
                                if let BytecodeInstruction::Call { callee, .. } = instruction
                                    && matches!(callee,CallTarget::Native(NativeCall::Engine(id)) if id.index()==import)
                                {
                                    *callee = CallTarget::StandardIntrinsic(*operation);
                                    changed = true;
                                }
                            }
                        }
                        assert!(changed);
                    }
                    _ => unreachable!(),
                };
                reject(
                    &forged,
                    &format!("{operation:?} {shape} storage {mutation}"),
                );
                checked += 1;
            }
            if shape == "nominal" {
                for protocol in [StandardTrait::PartialEq, StandardTrait::Hash] {
                    for mutation in 0..8 {
                        let mut forged = artifact.clone();
                        let contract = &mut forged.program.modules[root].engine_imports[import];
                        let selected = contract
                            .witnesses
                            .iter()
                            .position(|w| {
                                StandardTrait::from_id(&w.interface.declaration) == Some(protocol)
                            })
                            .unwrap();
                        let witness = &mut contract.witnesses[selected];
                        assert_eq!(witness.methods.len(), 1);
                        match mutation {
                            0 => witness.methods.clear(),
                            1 => witness.methods[0] = contract.instance.clone(),
                            2 => witness.methods[0]
                                .arguments
                                .push(AbiType::Builtin(BuiltinType::Bool)),
                            3 => {
                                witness.implementation = NativeWitnessImplementation::Primitive;
                                witness.methods.clear();
                            }
                            4 => {
                                witness.implementation = NativeWitnessImplementation::Derived;
                            }
                            5 => witness.receiver = AbiType::Builtin(BuiltinType::Bool),
                            6 => {
                                let duplicated = witness.clone();
                                contract.witnesses.push(duplicated);
                            }
                            7 => {
                                witness.interface.associated_types.insert(
                                    witness.interface.declaration.clone(),
                                    AbiType::Builtin(BuiltinType::Bool),
                                );
                            }
                            _ => unreachable!(),
                        };
                        reject(
                            &forged,
                            &format!("{operation:?} {protocol:?} witness {mutation}"),
                        );
                        checked += 1;
                    }
                }
            }
            if matches!(
                operation,
                StandardIntrinsic::MapGetOrInsertWith | StandardIntrinsic::MapUpdate
            ) {
                for mutation in 0..4 {
                    let mut forged = artifact.clone();
                    let contract = &mut forged.program.modules[root].engine_imports[import];
                    let AbiType::Function { params, result } = &mut contract.signature.params[2]
                    else {
                        panic!()
                    };
                    match mutation {
                        0 => params.push(AbiType::Builtin(BuiltinType::Bool)),
                        1 => **result = AbiType::Builtin(BuiltinType::Bool),
                        2 => {
                            if params.is_empty() {
                                params.push(AbiType::Builtin(BuiltinType::I32));
                            } else {
                                params.clear();
                            }
                        }
                        _ => {
                            let AbiType::Map { access, .. } = &mut contract.signature.params[0]
                            else {
                                panic!()
                            };
                            *access = CollectionAccess::ReadOnly;
                        }
                    };
                    reject(
                        &forged,
                        &format!("{operation:?} {shape} callback {mutation}"),
                    );
                    checked += 1;
                }
            }
        }
    }
    assert_eq!(checked, 340);
}
#[test]
fn composed_key_imports_reject_bypassed_hash_equality_and_forged_helper_results() {
    let mut checked = 0;
    for shape in ["tuple", "option", "choice"] {
        let program = common::bytecode_ok(&source(StandardIntrinsic::MapGet, shape));
        let root = program.root.index();
        let import = program.modules[root]
            .engine_imports
            .iter()
            .position(|c| c.binding == EngineNativeBinding::Intrinsic(StandardIntrinsic::MapGet))
            .unwrap();
        let artifact = KbcArtifact::from_program(program, Default::default()).unwrap();
        for protocol in [StandardTrait::PartialEq, StandardTrait::Hash] {
            for mutation in 0..6 {
                let mut forged = artifact.clone();
                let module = &mut forged.program.modules[root];
                let contract = &mut module.engine_imports[import];
                let witness = contract
                    .witnesses
                    .iter_mut()
                    .find(|w| StandardTrait::from_id(&w.interface.declaration) == Some(protocol))
                    .unwrap();
                assert_eq!(witness.implementation, NativeWitnessImplementation::Derived);
                match mutation {
                    0 => {
                        witness.implementation = NativeWitnessImplementation::Primitive;
                        witness.methods.clear();
                    }
                    1 => witness.methods.clear(),
                    2 => witness.methods[0] = contract.instance.clone(),
                    3 => witness.methods[0].arguments.clear(),
                    4 => witness.receiver = AbiType::Builtin(BuiltinType::Bool),
                    _ => {
                        let target = witness.methods[0].clone();
                        let helper = module
                            .functions
                            .iter_mut()
                            .find(|f| f.identity.as_ref() == Some(&target))
                            .unwrap();
                        helper.metadata.semantic.result =
                            Some(AbiType::Builtin(if protocol == StandardTrait::Hash {
                                BuiltinType::Bool
                            } else {
                                BuiltinType::I64
                            }));
                    }
                }
                reject(
                    &forged,
                    &format!("{shape} {protocol:?} composed {mutation}"),
                );
                checked += 1;
            }
        }
    }
    assert_eq!(checked, 36);
}
