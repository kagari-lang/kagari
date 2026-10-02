use crate::tests::common;
use bincode::{DefaultOptions, Options};
use kagari_abi::{
    callable::EngineNativeBinding, language::Protocol, native_import::NativeWitnessImplementation,
    scalar::BuiltinType, standard::bindings::NativeDefaultMethod, types::AbiType,
};
use kagari_bytecode::{artifact::KbcArtifact, program::verify_program};
use kagari_common::{collection::CollectionAccess, identity::associated_type_id};
const OPERATIONS: &[(&str, NativeDefaultMethod)] = &[
    ("union", NativeDefaultMethod::SetUnion),
    ("intersection", NativeDefaultMethod::SetIntersection),
    ("difference", NativeDefaultMethod::SetDifference),
    (
        "symmetric_difference",
        NativeDefaultMethod::SetSymmetricDifference,
    ),
    ("is_subset", NativeDefaultMethod::SetIsSubset),
    ("is_superset", NativeDefaultMethod::SetIsSuperset),
    ("is_disjoint", NativeDefaultMethod::SetIsDisjoint),
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
#[test]
fn set_defaults_reject_forged_dual_traversal_membership_and_key_selection() {
    let mut checked = 0;
    for (method, operation) in OPERATIONS {
        let relation = method.starts_with("is_");
        for shape in ["nominal", "float"] {
            if shape == "float" && !relation {
                continue;
            }
            let (item, element) = if shape == "nominal" {
                ("Key", "Key{id:20}")
            } else {
                ("f64", "20.0")
            };
            for route in ["native", "proxy", "dynamic"] {
                if shape == "float" && route == "native" {
                    continue;
                }
                let storage = if route == "native" {
                    format!("val source:LinkedHashSet<{item}> =LinkedHashSet::from([{element}]);")
                } else {
                    format!("val source=Policy{{items:[{element}]}};")
                };
                let source = if route == "dynamic" {
                    format!("{storage}val view:Set<{item}> =source;")
                } else {
                    storage
                };
                let receiver = if route == "dynamic" { "view" } else { "source" };
                let result = if relation {
                    "bool".to_owned()
                } else {
                    format!("LinkedHashSet<{item}>")
                };
                let program = common::bytecode_ok(&format!(
                    r#"
struct Key{{val id:i32}}
impl PartialEq for Key{{fn eq(self,other:Self)->bool{{self.id==other.id}}}}impl Eq for Key{{}}impl Hash for Key{{fn hash(self)->i64{{0i64}}}}
struct Policy<T:PartialEq>{{val items:ArrayList<T>}}
impl<T:PartialEq> Iterable for Policy<T>{{type Item=T;type Iter=Iter<T>;fn iter(self)->Iter<T>{{self.items.iter()}}}}
impl<T:PartialEq> Set<T> for Policy<T>{{fn len(self)->usize{{self.items.len()}}fn is_empty(self)->bool{{self.items.is_empty()}}fn contains(self,value:T)->bool{{self.items.contains(value)}}}}
fn main(){{{source}val rhs:Set<{item}> =Policy{{items:[{element}]}};val output:{result} ={receiver}.{method}(rhs);}}
"#
                ));
                let root = program.root.index();
                let import = program.modules[root]
                    .native_imports
                    .iter()
                    .position(|i| i.binding == EngineNativeBinding::TraitDefault(*operation))
                    .unwrap();
                let artifact = KbcArtifact::from_program(program, Default::default()).unwrap();
                for mutation in 0..18 {
                    let mut forged = artifact.clone();
                    let contract = &mut forged.program.modules[root].native_imports[import];
                    let selected = |source, protocol| {
                        contract
                            .witnesses
                            .iter()
                            .position(|w| {
                                w.receiver == contract.signature.params[source]
                                    && Protocol::from_id(&w.interface.declaration) == Some(protocol)
                            })
                            .unwrap()
                    };
                    let left = selected(0, Protocol::Set);
                    let right = selected(1, Protocol::Set);
                    let left_iter = selected(0, Protocol::Iterable);
                    let right_iter = selected(1, Protocol::Iterable);
                    let next = contract
                        .witnesses
                        .iter()
                        .position(|w| {
                            Protocol::from_id(&w.interface.declaration) == Some(Protocol::Iterator)
                        })
                        .unwrap();
                    match mutation {
                        0..5 => {
                            let index = [left, right, left_iter, right_iter, next][mutation];
                            contract.witnesses.remove(index);
                        }
                        5 => {
                            let witness = &mut contract.witnesses[left_iter];
                            witness.interface.associated_types.insert(
                                associated_type_id(&witness.interface.declaration, "Item"),
                                AbiType::Builtin(BuiltinType::Bool),
                            );
                        }
                        6 => {
                            contract.signature.params[1] = AbiType::Array(
                                Box::new(AbiType::Builtin(BuiltinType::Bool)),
                                CollectionAccess::Mutable,
                            )
                        }
                        7 => {
                            if let AbiType::Set(_, access) = &mut contract.signature.result {
                                *access = CollectionAccess::ReadOnly;
                            } else {
                                contract.signature.result = AbiType::Builtin(BuiltinType::Never);
                            }
                        }
                        8 => contract
                            .signature
                            .params
                            .push(AbiType::Builtin(BuiltinType::Bool)),
                        9 => contract
                            .instance
                            .arguments
                            .push(AbiType::Builtin(BuiltinType::Bool)),
                        10 => contract.binding_version -= 1,
                        11 => {
                            contract.witnesses[left].interface.arguments[0] =
                                AbiType::Builtin(BuiltinType::Bool)
                        }
                        12 => {
                            contract.witnesses[right].implementation =
                                NativeWitnessImplementation::Primitive
                        }
                        13 => {
                            contract.witnesses[next].implementation =
                                NativeWitnessImplementation::Derived
                        }
                        14 => contract.witnesses[left_iter]
                            .methods
                            .push(contract.instance.clone()),
                        15 => {
                            let w = &mut contract.witnesses[left];
                            if w.methods.is_empty() {
                                w.methods.push(contract.instance.clone());
                            } else {
                                w.methods.pop();
                            }
                        }
                        16 => {
                            let w = &mut contract.witnesses[right];
                            w.interface.associated_types.insert(
                                w.interface.declaration.clone(),
                                AbiType::Builtin(BuiltinType::Bool),
                            );
                        }
                        _ => {
                            let duplicate = contract.witnesses[right].clone();
                            contract.witnesses.push(duplicate);
                        }
                    }
                    reject(
                        &forged,
                        &format!("{method} {shape} {route} contract {mutation}"),
                    );
                    checked += 1;
                }
                for protocol in [Protocol::Hash, Protocol::PartialEq] {
                    let contract = &artifact.program.modules[root].native_imports[import];
                    let Some(selected) = contract.witnesses.iter().position(|w| {
                        Protocol::from_id(&w.interface.declaration) == Some(protocol)
                    }) else {
                        continue;
                    };
                    for mutation in 0..8 {
                        let mut forged = artifact.clone();
                        let contract = &mut forged.program.modules[root].native_imports[import];
                        let w = &mut contract.witnesses[selected];
                        assert_eq!(w.methods.len(), 1);
                        match mutation {
                            0 => w.methods.clear(),
                            1 => w.methods[0] = contract.instance.clone(),
                            2 => w.methods[0]
                                .arguments
                                .push(AbiType::Builtin(BuiltinType::Bool)),
                            3 => {
                                w.implementation = NativeWitnessImplementation::Primitive;
                                w.methods.clear();
                            }
                            4 => w.receiver = AbiType::Builtin(BuiltinType::Bool),
                            5 => w.implementation = NativeWitnessImplementation::Derived,
                            6 => {
                                let duplicate = w.clone();
                                contract.witnesses.push(duplicate);
                            }
                            _ => {
                                contract.witnesses.remove(selected);
                            }
                        }
                        reject(
                            &forged,
                            &format!("{method} {shape} {route} {protocol:?} key {mutation}"),
                        );
                        checked += 1;
                    }
                }
            }
        }
    }
    assert_eq!(checked, 726);
}
