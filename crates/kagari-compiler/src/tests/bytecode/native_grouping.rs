use crate::tests::common;
use bincode::{DefaultOptions, Options};
use kagari_abi::{
    callable::EngineNativeBinding,
    native_import::NativeWitnessImplementation,
    scalar::BuiltinType,
    standard::{bindings::NativeDefaultMethod, traits::StandardTrait},
    types::AbiType,
};
use kagari_bytecode::{KbcArtifact, verify_program};
use kagari_common::{collection::CollectionAccess, identity::associated_type_id};

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
fn grouping_rejects_forged_items_callbacks_results_and_key_applications() {
    let mut checked = 0;
    for shape in [
        "scalar",
        "nominal",
        "tuple",
        "option",
        "identity",
        "interface",
    ] {
        let (key, expression) = match shape {
            "scalar" => ("i32", "20"),
            "nominal" => ("Key", "Key{id:20}"),
            "tuple" => ("(Key,i32)", "(Key{id:20},0)"),
            "option" => ("Option<Key>", "Some(Key{id:20})"),
            "identity" => ("ArrayList<i32>", "[20]"),
            _ => ("List<i32>", "[20]"),
        };
        for route in ["native", "custom", "generic"] {
            let source = if route == "native" {
                "items.iter()"
            } else {
                "Cursor{items:items,index:0usize}"
            };
            let call = if route == "generic" {
                "group(source,|item|item.key)"
            } else {
                "source.group_by(|item|item.key)"
            };
            let program = common::bytecode_ok(&format!(
                r#"
struct Key{{val id:i32}}impl PartialEq for Key{{fn eq(self,other:Self)->bool{{self.id==other.id}}}}impl Eq for Key{{}}impl Hash for Key{{fn hash(self)->i64{{0i64}}}}
struct Item<K>{{val key:K}}
struct Cursor<T>{{val items:ArrayList<T>,var index:usize}}impl<T> Iterator for Cursor<T>{{type Item=T;fn next(self)->Option<T>{{val item=self.items.get(self.index);self.index+=1usize;item}}}}
fn group<K:Eq+Hash,I:Iterator<Item=Item<K>>>(source:I,key:fn(Item<K>)->K)->LinkedHashMap<K,ArrayList<Item<K>>>{{source.group_by(key)}}
fn main(){{val items:ArrayList<Item<{key}>> =[Item{{key:{expression}}}];val source={source};val output:LinkedHashMap<{key},ArrayList<Item<{key}>>> ={call};}}
"#
            ));
            let root = program.root.index();
            let import = program.modules[root]
                .engine_imports
                .iter()
                .position(|i| {
                    i.binding == EngineNativeBinding::TraitDefault(NativeDefaultMethod::GroupBy)
                })
                .unwrap();
            let artifact = KbcArtifact::from_program(program, Default::default()).unwrap();
            for mutation in 0..17 {
                let mut forged = artifact.clone();
                let contract = &mut forged.program.modules[root].engine_imports[import];
                let next = contract
                    .witnesses
                    .iter()
                    .position(|w| {
                        StandardTrait::from_id(&w.interface.declaration)
                            == Some(StandardTrait::Iterator)
                    })
                    .unwrap();
                let hash = contract
                    .witnesses
                    .iter()
                    .position(|w| {
                        StandardTrait::from_id(&w.interface.declaration)
                            == Some(StandardTrait::Hash)
                    })
                    .unwrap();
                let eq = contract
                    .witnesses
                    .iter()
                    .position(|w| {
                        StandardTrait::from_id(&w.interface.declaration) == Some(StandardTrait::Eq)
                    })
                    .unwrap();
                let partial = contract
                    .witnesses
                    .iter()
                    .position(|w| {
                        StandardTrait::from_id(&w.interface.declaration)
                            == Some(StandardTrait::PartialEq)
                    })
                    .unwrap();
                match mutation {
                    0..4 => {
                        contract
                            .witnesses
                            .remove([next, hash, eq, partial][mutation]);
                    }
                    4 => contract.binding_version -= 1,
                    5 => contract
                        .instance
                        .arguments
                        .push(AbiType::Builtin(BuiltinType::Bool)),
                    6 => contract
                        .signature
                        .params
                        .push(AbiType::Builtin(BuiltinType::Bool)),
                    7 => {
                        let AbiType::Function { params, .. } = &mut contract.signature.params[1]
                        else {
                            panic!()
                        };
                        params[0] = AbiType::Builtin(BuiltinType::Bool);
                    }
                    8 => {
                        let AbiType::Function { result, .. } = &mut contract.signature.params[1]
                        else {
                            panic!()
                        };
                        **result = AbiType::Builtin(BuiltinType::Bool);
                    }
                    9 => {
                        let AbiType::Map { access, .. } = &mut contract.signature.result else {
                            panic!()
                        };
                        *access = CollectionAccess::ReadOnly;
                    }
                    10 => {
                        let AbiType::Map { value, .. } = &mut contract.signature.result else {
                            panic!()
                        };
                        **value = AbiType::Array(
                            Box::new(AbiType::Builtin(BuiltinType::Bool)),
                            CollectionAccess::Mutable,
                        );
                    }
                    11 => {
                        let witness = &mut contract.witnesses[next];
                        witness.interface.associated_types.insert(
                            associated_type_id(&witness.interface.declaration, "Item"),
                            AbiType::Builtin(BuiltinType::Bool),
                        );
                    }
                    12 => {
                        contract.witnesses[next].implementation =
                            NativeWitnessImplementation::Derived
                    }
                    13 => {
                        let witness = &mut contract.witnesses[next];
                        if witness.methods.is_empty() {
                            witness.methods.push(contract.instance.clone());
                        } else {
                            witness.methods.clear();
                        }
                    }
                    14 => {
                        let witness = contract.witnesses[next].clone();
                        contract.witnesses.push(witness);
                    }
                    15 => contract.signature.result = AbiType::Builtin(BuiltinType::Never),
                    _ => contract.witnesses[next].receiver = AbiType::Builtin(BuiltinType::Bool),
                }
                reject(&forged, &format!("{shape} {route} contract {mutation}"));
                checked += 1;
            }
            for protocol in [StandardTrait::Hash, StandardTrait::PartialEq] {
                let contract = &artifact.program.modules[root].engine_imports[import];
                let selected = contract
                    .witnesses
                    .iter()
                    .position(|w| {
                        StandardTrait::from_id(&w.interface.declaration) == Some(protocol)
                    })
                    .unwrap();
                if contract.witnesses[selected].methods.is_empty() {
                    continue;
                }
                for mutation in 0..8 {
                    let mut forged = artifact.clone();
                    let contract = &mut forged.program.modules[root].engine_imports[import];
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
                        4 => witness.receiver = AbiType::Builtin(BuiltinType::Bool),
                        5 => witness.implementation = NativeWitnessImplementation::Host,
                        6 => {
                            let duplicate = witness.clone();
                            contract.witnesses.push(duplicate);
                        }
                        _ => {
                            contract.witnesses.remove(selected);
                        }
                    }
                    reject(&forged, &format!("{shape} {route} {protocol:?} {mutation}"));
                    checked += 1;
                }
            }
        }
    }
    assert_eq!(checked, 450);
}
