use crate::tests::common;
use bincode::{DefaultOptions, Options};
use kagari_abi::{
    callable::EngineNativeBinding,
    native_import::NativeWitnessImplementation,
    scalar::BuiltinType,
    standard::{StandardIntrinsic, bindings::NativeProtocolMethod, traits::StandardTrait},
    types::AbiType,
};
use kagari_bytecode::{KbcArtifact, verify_program};
use kagari_common::{collection::CollectionAccess, identity::associated_type_id};
#[test]
fn key_construction_rejects_forged_traversal_storage_and_key_methods() {
    let mut checked = 0;
    for destination in ["map", "set"] {
        for shape in ["nominal", "tuple"] {
            let key = if shape == "nominal" {
                "Key<i32>"
            } else {
                "(Key<i32>,i32)"
            };
            let value = if shape == "nominal" {
                "Key{id:20}"
            } else {
                "(Key{id:20},0)"
            };
            let item = if destination == "map" {
                format!("({key},i32)")
            } else {
                key.to_owned()
            };
            let elements = if destination == "map" {
                format!("({value},42)")
            } else {
                value.to_owned()
            };
            let owner = if destination == "map" {
                "LinkedHashMap"
            } else {
                "LinkedHashSet"
            };
            let storage = if destination == "map" {
                format!("{owner}<{key},i32>")
            } else {
                format!("{owner}<{key}>")
            };
            for method in ["from", "from_iter"] {
                for source in ["native", "proxy", "dynamic", "custom"] {
                    let setup = match source {
                        "native" => format!("val source:ArrayList<{item}> =[{elements}];"),
                        "dynamic" if method == "from_iter" => format!(
                            "val source:Iterable<Item={item},Iter=Iter<{item}>> =Proxy{{items:[{elements}]}};"
                        ),
                        "dynamic" => {
                            format!("val source:List<{item}> =Proxy{{items:[{elements}]}};")
                        }
                        "custom" if method == "from_iter" => {
                            format!("val source=Span{{items:[{elements}]}};")
                        }
                        _ => format!("val source=Proxy{{items:[{elements}]}};"),
                    };
                    let program = common::bytecode_ok(&format!(
                        r#"
struct Key<T>{{val id:T}}
impl<T:PartialEq> PartialEq for Key<T>{{fn eq(self,other:Self)->bool{{self.id==other.id}}}}
impl<T:Eq> Eq for Key<T>{{}}
impl<T:Eq+Hash> Hash for Key<T>{{fn hash(self)->i64{{0i64}}}}
struct Proxy<T>{{val items:ArrayList<T>}}
impl<T> Index<usize> for Proxy<T>{{type Output=T;fn index(self,i:usize)->T{{self.items[i]}}}}
impl<T> Iterable for Proxy<T>{{type Item=T;type Iter=Iter<T>;fn iter(self)->Iter<T>{{self.items.iter()}}}}
impl<T> List<T> for Proxy<T>{{fn len(self)->usize{{self.items.len()}}fn is_empty(self)->bool{{self.items.is_empty()}}fn get(self,i:usize)->Option<T>{{self.items.get(i)}}}}
struct Cursor<T>{{val items:ArrayList<T>,var index:usize}}
impl<T> Iterator for Cursor<T>{{type Item=T;fn next(self)->Option<T>{{val item=self.items.get(self.index);self.index+=1usize;item}}}}
struct Span<T>{{val items:ArrayList<T>}}
impl<T> Iterable for Span<T>{{type Item=T;type Iter=Cursor<T>;fn iter(self)->Cursor<T>{{Cursor{{items:self.items,index:0usize}}}}}}
fn main(){{{setup}val output:{storage} ={owner}::{method}(source);}}
"#
                    ));
                    let root = program.root.index();
                    let binding = if method == "from_iter" {
                        EngineNativeBinding::Protocol(NativeProtocolMethod::CollectionFromIterator)
                    } else {
                        EngineNativeBinding::Intrinsic(if destination == "map" {
                            StandardIntrinsic::LinkedHashMapFrom
                        } else {
                            StandardIntrinsic::LinkedHashSetFrom
                        })
                    };
                    let import = program.modules[root]
                        .engine_imports
                        .iter()
                        .position(|i| i.binding == binding)
                        .unwrap();
                    let artifact = KbcArtifact::from_program(program, Default::default()).unwrap();
                    for mutation in 0..30 {
                        let mut forged = artifact.clone();
                        let contract = &mut forged.program.modules[root].engine_imports[import];
                        let iterable = contract
                            .witnesses
                            .iter()
                            .position(|w| {
                                StandardTrait::from_id(&w.interface.declaration)
                                    == Some(StandardTrait::Iterable)
                            })
                            .unwrap();
                        let next = contract
                            .witnesses
                            .iter()
                            .position(|w| {
                                StandardTrait::from_id(&w.interface.declaration)
                                    == Some(StandardTrait::Iterator)
                            })
                            .unwrap();
                        match mutation {
                            0 => {
                                contract.witnesses.remove(iterable);
                            }
                            1 => {
                                contract.witnesses.remove(next);
                            }
                            2 => {
                                contract.witnesses[next].receiver =
                                    AbiType::Builtin(BuiltinType::I32)
                            }
                            3 | 4 => {
                                let witness = &mut contract.witnesses
                                    [if mutation == 3 { iterable } else { next }];
                                witness.interface.associated_types.insert(
                                    associated_type_id(&witness.interface.declaration, "Item"),
                                    AbiType::Builtin(BuiltinType::Bool),
                                );
                            }
                            5 => {
                                let witness = &mut contract.witnesses[iterable];
                                if let NativeWitnessImplementation::Table(target) =
                                    &mut witness.implementation
                                {
                                    target.arguments.push(AbiType::Builtin(BuiltinType::Bool));
                                } else {
                                    witness.implementation = NativeWitnessImplementation::Primitive;
                                }
                            }
                            6 => contract.witnesses[next]
                                .methods
                                .push(contract.instance.clone()),
                            7 => {
                                if let Some(index) = contract.witnesses.iter().position(|w| {
                                    StandardTrait::from_id(&w.interface.declaration)
                                        == Some(StandardTrait::List)
                                }) {
                                    contract.witnesses.remove(index);
                                } else {
                                    contract.requirements.clear();
                                }
                            }
                            8 | 9 => match &mut contract.signature.result {
                                AbiType::Map { key, access, .. } | AbiType::Set(key, access) => {
                                    if mutation == 8 {
                                        *access = CollectionAccess::ReadOnly;
                                    } else {
                                        **key = AbiType::Builtin(BuiltinType::Bool);
                                    }
                                }
                                _ => panic!(),
                            },
                            10 => contract
                                .signature
                                .params
                                .push(AbiType::Builtin(BuiltinType::I32)),
                            11 => {
                                contract.binding = EngineNativeBinding::Intrinsic(
                                    StandardIntrinsic::ArrayListFromFn,
                                )
                            }
                            12 => contract
                                .instance
                                .arguments
                                .push(AbiType::Builtin(BuiltinType::Bool)),
                            13 => contract.binding_version -= 1,
                            14..30 => {
                                let protocol = if mutation < 22 {
                                    StandardTrait::PartialEq
                                } else {
                                    StandardTrait::Hash
                                };
                                let selected = contract
                                    .witnesses
                                    .iter()
                                    .position(|w| {
                                        StandardTrait::from_id(&w.interface.declaration)
                                            == Some(protocol)
                                    })
                                    .unwrap();
                                let witness = &mut contract.witnesses[selected];
                                match (mutation - 14) % 8 {
                                    0 => witness.methods.clear(),
                                    1 => witness.methods[0] = contract.instance.clone(),
                                    2 => witness.methods[0]
                                        .arguments
                                        .push(AbiType::Builtin(BuiltinType::Bool)),
                                    3 => {
                                        witness.implementation =
                                            NativeWitnessImplementation::Primitive;
                                        witness.methods.clear();
                                    }
                                    4 => witness.receiver = AbiType::Builtin(BuiltinType::Bool),
                                    5 => {
                                        let duplicate = witness.clone();
                                        contract.witnesses.push(duplicate);
                                    }
                                    6 => {
                                        witness.interface.associated_types.insert(
                                            witness.interface.declaration.clone(),
                                            AbiType::Builtin(BuiltinType::Bool),
                                        );
                                    }
                                    _ => {
                                        contract.witnesses.remove(selected);
                                    }
                                }
                            }
                            _ => unreachable!(),
                        }
                        assert!(
                            verify_program(&forged.program).is_err(),
                            "{destination} {method} {shape} {source} {mutation}"
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
                            "encoded {destination} {method} {shape} {source} {mutation}"
                        );
                        checked += 1;
                    }
                }
            }
        }
    }
    assert_eq!(checked, 960);
}
