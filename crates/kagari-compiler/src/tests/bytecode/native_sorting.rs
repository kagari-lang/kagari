use crate::tests::common;
use bincode::{DefaultOptions, Options};
use kagari_abi::{
    callable::{EngineNativeBinding, NativeCall},
    native_import::NativeWitnessImplementation,
    scalar::BuiltinType,
    standard::{
        RuntimePrimitive,
        traits::{self, StandardTrait},
    },
    types::AbiType,
};
use kagari_bytecode::{
    artifact::KbcArtifact,
    instruction::{BytecodeInstruction, CallTarget},
    program::verify_program,
};
use kagari_common::collection::CollectionAccess;

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

#[test]
fn prepared_array_imports_reject_forged_storage_callbacks_and_comparison_targets() {
    for mode in ["sort", "sort_by", "sort_by_key", "dedup"] {
        for nominal in [false, true] {
            for generic in [false, true] {
                let item = if nominal { "Rank<i32>" } else { "i32" };
                let value = if nominal { "Rank{key:1}" } else { "1" };
                let callback = match mode {
                    "sort_by" => format!("val p:fn({item},{item})->Ordering =|a,b|a.cmp(b);"),
                    "sort_by_key" => format!("val p:fn({item})->{item} =|item|item;"),
                    _ => String::new(),
                };
                let arguments = if callback.is_empty() {
                    "items"
                } else {
                    "items,p"
                };
                let action = if generic {
                    format!("{mode}({arguments});")
                } else {
                    format!(
                        "items.{mode}({});",
                        if callback.is_empty() { "" } else { "p" }
                    )
                };
                let program = common::bytecode_ok(&format!(
                    r#"
struct Rank<T>{{val key:T}}
impl<T:PartialEq> PartialEq for Rank<T>{{fn eq(self,other:Self)->bool{{self.key==other.key}}}}
impl<T:Eq> Eq for Rank<T>{{}}
impl<T:PartialOrd> PartialOrd for Rank<T>{{fn partial_cmp(self,other:Self)->Option<Ordering>{{self.key.partial_cmp(other.key)}}}}
impl<T:Ord> Ord for Rank<T>{{fn cmp(self,other:Self)->Ordering{{self.key.cmp(other.key)}}}}
fn sort<T:Ord>(items:ArrayList<T>){{items.sort();}}
fn sort_by<T>(items:ArrayList<T>,p:fn(T,T)->Ordering){{items.sort_by(p);}}
fn sort_by_key<T,K:Ord>(items:ArrayList<T>,p:fn(T)->K){{items.sort_by_key(p);}}
fn dedup<T:PartialEq>(items:ArrayList<T>){{items.dedup();}}
fn main()->i32{{val items:ArrayList<{item}> =[{value}];{callback}{action}42}}
"#
                ));
                let binding = EngineNativeBinding::Intrinsic(match mode {
                    "sort" => RuntimePrimitive::ArraySort,
                    "sort_by" => RuntimePrimitive::ArraySortBy,
                    "sort_by_key" => RuntimePrimitive::ArraySortByKey,
                    _ => RuntimePrimitive::ArrayDedup,
                });
                let root = program.root.index();
                let import = program.modules[root]
                    .native_imports
                    .iter()
                    .position(|i| i.binding == binding)
                    .unwrap();
                let artifact = KbcArtifact::from_program(program, Default::default()).unwrap();
                for mutation in 0..10 {
                    let mut forged = artifact.clone();
                    let module = &mut forged.program.modules[root];
                    let contract = &mut module.native_imports[import];
                    match mutation {
                        0 => contract.signature.params[0] = AbiType::Builtin(BuiltinType::Bool),
                        1 => contract.signature.result = AbiType::Builtin(BuiltinType::Bool),
                        2 => {
                            let AbiType::Array(_, access) = &mut contract.signature.params[0]
                            else {
                                panic!()
                            };
                            *access = CollectionAccess::ReadOnly;
                        }
                        3 => {
                            let AbiType::Array(item, _) = &contract.signature.params[0] else {
                                panic!()
                            };
                            contract.signature.params[0] =
                                AbiType::Set(item.clone(), CollectionAccess::Mutable);
                        }
                        4 => contract
                            .instance
                            .arguments
                            .push(AbiType::Builtin(BuiltinType::Bool)),
                        5 => {
                            contract.binding = EngineNativeBinding::Intrinsic(if mode == "dedup" {
                                RuntimePrimitive::ArraySort
                            } else {
                                RuntimePrimitive::ArrayDedup
                            })
                        }
                        6 => contract.binding_version -= 1,
                        7 => contract.signature.params.pop().map(|_| ()).unwrap(),
                        8 | 9 => {
                            let mut changed = false;
                            for function in &mut module.functions {
                                for instruction in &mut function.instructions {
                                    if let BytecodeInstruction::Call { callee, args, .. } =
                                        instruction
                                        && matches!(callee,CallTarget::Native(id) if id.index()==import)
                                    {
                                        if mutation == 8 {
                                            let EngineNativeBinding::Intrinsic(operation) = binding
                                            else {
                                                panic!()
                                            };
                                            *callee = CallTarget::RuntimePrimitive(operation);
                                        } else {
                                            args.clear();
                                        }
                                        changed = true;
                                    }
                                }
                            }
                            assert!(changed);
                        }
                        _ => unreachable!(),
                    }
                    reject(
                        &forged,
                        &format!("{mode} {nominal} {generic} base {mutation}"),
                    );
                }
                if mode == "sort_by" || mode == "sort_by_key" {
                    for mutation in 0..4 {
                        let mut forged = artifact.clone();
                        let contract = &mut forged.program.modules[root].native_imports[import];
                        let AbiType::Function { params, result } =
                            &mut contract.signature.params[1]
                        else {
                            panic!()
                        };
                        match mutation {
                            0 => params.push(AbiType::Builtin(BuiltinType::Bool)),
                            1 => params.pop().map(|_| ()).unwrap(),
                            2 => params[0] = AbiType::Builtin(BuiltinType::Bool),
                            _ => **result = AbiType::Builtin(BuiltinType::Bool),
                        }
                        reject(
                            &forged,
                            &format!("{mode} {nominal} {generic} callback {mutation}"),
                        );
                    }
                }
                if mode != "sort_by" {
                    for mutation in 0..9 {
                        let mut forged = artifact.clone();
                        let contract = &mut forged.program.modules[root].native_imports[import];
                        assert_eq!(contract.witnesses.len(), 1);
                        let witness = &mut contract.witnesses[0];
                        match mutation {
                            0 => contract.witnesses.clear(),
                            1 => witness.receiver = AbiType::Builtin(BuiltinType::Bool),
                            2 => {
                                witness.interface.declaration =
                                    traits::identity(StandardTrait::Hash)
                            }
                            3 => witness.methods.push(contract.instance.clone()),
                            4 => {
                                witness.implementation = if witness.implementation
                                    == NativeWitnessImplementation::Primitive
                                {
                                    NativeWitnessImplementation::Derived
                                } else {
                                    NativeWitnessImplementation::Primitive
                                };
                                witness.methods.clear();
                            }
                            5 => {
                                if nominal {
                                    assert_eq!(witness.methods.len(), 1);
                                    witness.methods[0]
                                        .arguments
                                        .push(AbiType::Builtin(BuiltinType::Bool));
                                } else {
                                    witness
                                        .interface
                                        .arguments
                                        .push(AbiType::Builtin(BuiltinType::Bool));
                                }
                            }
                            6 => {
                                if nominal {
                                    witness.methods.clear();
                                } else {
                                    contract.requirements.clear();
                                }
                            }
                            7 => {
                                let duplicate = witness.clone();
                                contract.witnesses.push(duplicate);
                            }
                            8 => contract.requirements[0].ty = AbiType::Builtin(BuiltinType::Bool),
                            _ => unreachable!(),
                        }
                        reject(
                            &forged,
                            &format!("{mode} {nominal} {generic} witness {mutation}"),
                        );
                    }
                }
            }
        }
    }
}

#[test]
fn dedup_rejects_bypassed_composed_equality_and_wrong_derived_results() {
    for (item, value) in [
        ("(Rank<i32>,i32)", "(Rank{key:1},2)"),
        ("Option<Rank<i32>>", "Some(Rank{key:1})"),
        ("Choice<Rank<i32>>", "Choice::Item(Rank{key:1})"),
    ] {
        let program = common::bytecode_ok(&format!(
            r#"
struct Rank<T>{{val key:T}}
impl<T:PartialEq> PartialEq for Rank<T>{{fn eq(self,other:Self)->bool{{self.key==other.key}}}}
enum Choice<T>{{Empty,Item(T)}}
fn main()->i32{{val items:ArrayList<{item}> =[{value}];items.dedup();42}}
"#
        ));
        let root = program.root.index();
        let import = program.modules[root]
            .native_imports
            .iter()
            .position(|i| i.binding == EngineNativeBinding::Intrinsic(RuntimePrimitive::ArrayDedup))
            .unwrap();
        let artifact = KbcArtifact::from_program(program, Default::default()).unwrap();
        for mutation in 0..5 {
            let mut forged = artifact.clone();
            let module = &mut forged.program.modules[root];
            let declaration = module.native_imports[import].instance.declaration.clone();
            let witness = &mut module.native_imports[import].witnesses[0];
            assert_eq!(witness.implementation, NativeWitnessImplementation::Derived);
            match mutation {
                0 => {
                    witness.implementation = NativeWitnessImplementation::Primitive;
                    witness.methods.clear();
                }
                1 => witness.methods.clear(),
                2 => witness.methods[0].arguments.clear(),
                3 => witness.methods[0].declaration = declaration.clone(),
                _ => {
                    let target = witness.methods[0].clone();
                    let function = module
                        .functions
                        .iter_mut()
                        .find(|f| f.identity.as_ref() == Some(&target))
                        .unwrap();
                    function.metadata.semantic.result = Some(AbiType::Builtin(BuiltinType::I32));
                }
            }
            reject(&forged, &format!("{item} {mutation}"));
        }
    }
}
