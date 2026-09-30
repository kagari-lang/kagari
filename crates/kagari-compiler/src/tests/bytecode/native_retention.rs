use crate::tests::common;
use bincode::{DefaultOptions, Options};
use kagari_abi::{
    callable::{EngineNativeBinding, NativeCall},
    scalar::BuiltinType,
    standard::StandardIntrinsic,
    types::AbiType,
};
use kagari_bytecode::{BytecodeInstruction, CallTarget, KbcArtifact, verify_program};
use kagari_common::collection::CollectionAccess;
#[test]
fn retention_rejects_forged_predicates_storage_and_bare_calls() {
    for kind in ["array", "map", "set"] {
        for heap in [false, true] {
            for generic in [false, true] {
                let item = if heap { "Key" } else { "i32" };
                let element = if heap {
                    "Key{id:20,tag:0},Key{id:22,tag:0}"
                } else {
                    "20,22"
                };
                let setup = match kind {
                    "array" => format!("val a:ArrayList<{item}> =[{element}];"),
                    "set" => {
                        format!("val a:LinkedHashSet<{item}> =LinkedHashSet::from([{element}]);")
                    }
                    _ => {
                        let pairs = if heap {
                            "(Key{id:20,tag:0},1),(Key{id:22,tag:0},2)"
                        } else {
                            "(20,1),(22,2)"
                        };
                        format!("val a:LinkedHashMap<{item},i32> =LinkedHashMap::from([{pairs}]);")
                    }
                };
                let parameters = if kind == "map" { "key,value" } else { "key" };
                let signature = if kind == "map" {
                    format!("{item},i32")
                } else {
                    item.into()
                };
                let action = if generic {
                    format!("{kind}_keep(a,p);")
                } else {
                    "a.retain(p);".into()
                };
                let program = common::bytecode_ok(&format!(
                    r#"
struct Key{{val id:i32,var tag:i32}}
impl PartialEq for Key{{fn eq(self,other:Self)->bool{{self.id==other.id}}}}impl Eq for Key{{}}impl Hash for Key{{fn hash(self)->i64{{0i64}}}}
fn array_keep<T>(a:ArrayList<T>,p:fn(T)->bool){{a.retain(p);}}
fn map_keep<K:Eq+Hash,V>(a:LinkedHashMap<K,V>,p:fn(K,V)->bool){{a.retain(p);}}
fn set_keep<T:Eq+Hash>(a:LinkedHashSet<T>,p:fn(T)->bool){{a.retain(p);}}
fn main(){{{setup}val p:fn({signature})->bool =|{parameters}|true;{action}}}
"#
                ));
                let binding = EngineNativeBinding::Intrinsic(match kind {
                    "array" => StandardIntrinsic::ArrayRetain,
                    "map" => StandardIntrinsic::MapRetain,
                    _ => StandardIntrinsic::SetRetain,
                });
                let root = program.root.index();
                let import = program.modules[root]
                    .engine_imports
                    .iter()
                    .position(|i| i.binding == binding)
                    .unwrap();
                let artifact = KbcArtifact::from_program(program, Default::default()).unwrap();
                for mutation in 0..16 {
                    let mut forged = artifact.clone();
                    let module = &mut forged.program.modules[root];
                    let contract = &mut module.engine_imports[import];
                    match mutation {
                        0 => {
                            let AbiType::Function { params, .. } =
                                &mut contract.signature.params[1]
                            else {
                                panic!()
                            };
                            params.push(AbiType::Builtin(BuiltinType::Bool));
                        }
                        1 => {
                            let AbiType::Function { result, .. } =
                                &mut contract.signature.params[1]
                            else {
                                panic!()
                            };
                            **result = AbiType::Builtin(BuiltinType::I32);
                        }
                        2 => {
                            let AbiType::Function { params, .. } =
                                &mut contract.signature.params[1]
                            else {
                                panic!()
                            };
                            params[0] = AbiType::Builtin(BuiltinType::Bool);
                        }
                        3 => {
                            let AbiType::Function { params, .. } =
                                &mut contract.signature.params[1]
                            else {
                                panic!()
                            };
                            params.pop();
                        }
                        4 => match &mut contract.signature.params[0] {
                            AbiType::Array(_, access)
                            | AbiType::Set(_, access)
                            | AbiType::Map { access, .. } => *access = CollectionAccess::ReadOnly,
                            _ => panic!(),
                        },
                        5 => {
                            contract.binding = EngineNativeBinding::Intrinsic(if kind == "array" {
                                StandardIntrinsic::SetRetain
                            } else {
                                StandardIntrinsic::ArrayRetain
                            })
                        }
                        6 => contract.signature.result = AbiType::Builtin(BuiltinType::Bool),
                        7 => contract
                            .signature
                            .params
                            .push(AbiType::Builtin(BuiltinType::Bool)),
                        8 => {
                            contract.signature.params.pop();
                        }
                        9 => {
                            contract.signature.params[0] = match &contract.signature.params[0] {
                                AbiType::Array(item, _) => {
                                    AbiType::Set(item.clone(), CollectionAccess::Mutable)
                                }
                                AbiType::Set(item, _) => {
                                    AbiType::Array(item.clone(), CollectionAccess::Mutable)
                                }
                                _ => AbiType::Builtin(BuiltinType::I32),
                            }
                        }
                        10 => {
                            if kind == "array" {
                                contract.signature.params[0] = AbiType::Array(
                                    Box::new(AbiType::Builtin(BuiltinType::Bool)),
                                    CollectionAccess::Mutable,
                                );
                            } else {
                                assert!(!contract.requirements.is_empty());
                                contract.requirements.clear();
                            }
                        }
                        11 => {
                            if kind == "array" {
                                contract.signature.params[1] = AbiType::Builtin(BuiltinType::Bool);
                            } else {
                                contract.requirements[0].ty = AbiType::Builtin(BuiltinType::Bool);
                            }
                        }
                        12 => contract
                            .instance
                            .arguments
                            .push(AbiType::Builtin(BuiltinType::Bool)),
                        13 => {
                            if kind == "array" {
                                let AbiType::Function { params, .. } =
                                    &mut contract.signature.params[1]
                                else {
                                    panic!()
                                };
                                params.push(AbiType::Builtin(BuiltinType::Unit));
                            } else {
                                assert!(!contract.witnesses.is_empty());
                                contract.witnesses.clear();
                            }
                        }
                        14 | 15 => {
                            let mut changed = false;
                            for function in &mut module.functions {
                                for instruction in &mut function.instructions {
                                    if let BytecodeInstruction::Call { callee, args, .. } =
                                        instruction
                                        && matches!(callee,CallTarget::Native(NativeCall::Engine(id)) if id.index()==import)
                                    {
                                        if mutation == 14 {
                                            *callee = CallTarget::StandardIntrinsic(match kind {
                                                "array" => StandardIntrinsic::ArrayRetain,
                                                "map" => StandardIntrinsic::MapRetain,
                                                _ => StandardIntrinsic::SetRetain,
                                            });
                                        } else {
                                            args[0] = args[1];
                                        }
                                        changed = true;
                                    }
                                }
                            }
                            assert!(changed);
                        }
                        _ => unreachable!(),
                    }
                    assert!(
                        verify_program(&forged.program).is_err(),
                        "{kind} {heap} {generic} {mutation}"
                    );
                    let bytes = DefaultOptions::new()
                        .with_fixint_encoding()
                        .with_little_endian()
                        .serialize(&forged)
                        .unwrap();
                    assert!(
                        !KbcArtifact::from_bytes(&bytes).is_ok_and(|decoded| decoded
                            .validate_for_loader(&Default::default())
                            .is_ok()),
                        "encoded {kind} {heap} {generic} {mutation}"
                    );
                }
            }
        }
    }
}
