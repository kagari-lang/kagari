use crate::tests::common;
use bincode::{DefaultOptions, Options};
use kagari_abi::{
    callable::EngineNativeBinding,
    language::Protocol,
    native_import::NativeWitnessImplementation,
    scalar::BuiltinType,
    standard::{RuntimePrimitive, bindings::NativeDefaultMethod},
    types::{AbiType, PublicAbiItem},
};
use kagari_bytecode::{artifact::KbcArtifact, program::verify_program};
use kagari_common::collection::CollectionAccess;
#[test]
fn map_snapshots_reject_forged_traversal_and_result_construction() {
    for source in ["map", "custom", "dynamic", "native"] {
        for (direct, default, method) in [
            (
                RuntimePrimitive::MapKeys,
                NativeDefaultMethod::MapKeysView,
                "keys",
            ),
            (
                RuntimePrimitive::MapValues,
                NativeDefaultMethod::MapValuesView,
                "values",
            ),
            (
                RuntimePrimitive::MapEntries,
                NativeDefaultMethod::MapEntriesView,
                "entries",
            ),
        ] {
            let setup = match source {
                "map" => "val source=LinkedHashMap::from([(1,2)]);",
                "custom" => "val source=Association{items:[(1,2)]};",
                "dynamic" => "val source:Map<i32,i32> =Association{items:[(1,2)]};",
                _ => "val source:Map<i32,i32> =LinkedHashMap::from([(1,2)]);",
            };
            let program = common::bytecode_ok(&format!(
                r#"
struct Association<K,V> {{val items:ArrayList<(K,V)>}}
impl<K,V> Iterable for Association<K,V> {{type Item=(K,V);type Iter=Iter<(K,V)>;fn iter(self)->Iter<(K,V)>{{self.items.iter()}}}}
impl<K,V> Map<K,V> for Association<K,V> {{fn len(self)->usize{{self.items.len()}}fn is_empty(self)->bool{{self.items.is_empty()}}fn contains_key(self,key:K)->bool{{true}}fn get(self,key:K)->Option<V>{{self.items.get(0usize).map(|pair|pair[1])}}}}
fn main()->i32{{{setup}val out=source.{method}();42}}
"#
            ));
            let root = program.root.index();
            let binding = if source == "map" {
                EngineNativeBinding::Intrinsic(direct)
            } else {
                EngineNativeBinding::TraitDefault(default)
            };
            let import = program.modules[root]
                .native_imports
                .iter()
                .position(|import| import.binding == binding)
                .unwrap();
            let factory = program.modules[root].native_imports[import]
                .witnesses
                .iter()
                .position(|witness| {
                    Protocol::from_id(&witness.interface.declaration) == Some(Protocol::List)
                })
                .unwrap();
            let artifact = KbcArtifact::from_program(program, Default::default()).unwrap();
            for mutation in 0..16 {
                let mut forged = artifact.clone();
                let module = &mut forged.program.modules[root];
                let contract = &mut module.native_imports[import];
                let witness = &mut contract.witnesses[factory];
                match mutation {
                    0 => {
                        contract.witnesses.remove(factory);
                    }
                    1 => {
                        let duplicate = witness.clone();
                        contract.witnesses.push(duplicate);
                    }
                    2 => contract.signature.result = AbiType::Builtin(BuiltinType::I32),
                    3 => {
                        let AbiType::Array(_, access) = &mut witness.receiver else {
                            panic!()
                        };
                        *access = CollectionAccess::ReadOnly;
                    }
                    4 => {
                        witness.implementation = NativeWitnessImplementation::Primitive;
                        witness.methods.clear();
                    }
                    5 | 6 => {
                        let NativeWitnessImplementation::Table(target) =
                            &mut witness.implementation
                        else {
                            panic!()
                        };
                        if mutation == 5 {
                            target.arguments.push(AbiType::Builtin(BuiltinType::I32));
                        } else {
                            target.declaration.path[0].name = "missing".into();
                        }
                    }
                    7 => witness.methods.swap(0, 2),
                    8 => witness.methods.clear(),
                    9 | 10 => {
                        let NativeWitnessImplementation::Table(target) = &witness.implementation
                        else {
                            panic!()
                        };
                        let table = module
                            .public_items
                            .iter_mut()
                            .find_map(|item| match item {
                                PublicAbiItem::InterfaceTable(table)
                                    if table.declaration == target.declaration =>
                                {
                                    Some(table)
                                }
                                _ => None,
                            })
                            .unwrap();
                        if mutation == 9 {
                            table.native_bridge = false;
                        } else {
                            table.for_type = AbiType::Array(
                                Box::new(AbiType::Builtin(BuiltinType::I64)),
                                CollectionAccess::Mutable,
                            );
                        }
                    }
                    11 => {
                        witness.methods[0].declaration.path.last_mut().unwrap().name =
                            "missing".into()
                    }
                    12..=14 => {
                        let protocol = match mutation {
                            12 => Protocol::Iterable,
                            13 => Protocol::Iterator,
                            _ => Protocol::Map,
                        };
                        if let Some(slot) = contract.witnesses.iter().position(|witness| {
                            Protocol::from_id(&witness.interface.declaration) == Some(protocol)
                        }) {
                            if mutation == 14 {
                                contract.witnesses[slot]
                                    .methods
                                    .push(contract.instance.clone());
                            } else {
                                contract.witnesses.remove(slot);
                            }
                        } else {
                            contract.signature.params[0] = AbiType::Builtin(BuiltinType::I32);
                        }
                    }
                    _ => {
                        let NativeWitnessImplementation::Table(target) = &witness.implementation
                        else {
                            panic!()
                        };
                        module
                            .interface_tables
                            .retain(|table| table.declaration != target.declaration);
                    }
                }
                assert!(
                    verify_program(&forged.program).is_err(),
                    "{source} {method} {mutation}"
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
                    "encoded {source} {method} {mutation}"
                );
            }
        }
    }
}
