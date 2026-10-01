use crate::tests::common;
use bincode::{DefaultOptions, Options};
use kagari_abi::{
    callable::EngineNativeBinding,
    native_import::{NativeWitness, NativeWitnessImplementation},
    scalar::BuiltinType,
    standard::{bindings::NativeDefaultMethod, intrinsic, traits::StandardTrait},
    types::AbiType,
};
use kagari_bytecode::{KbcArtifact, verify_program};
use kagari_common::{collection::CollectionAccess, identity::associated_type_id};
fn rejected(artifact: &KbcArtifact, label: &str) {
    assert!(verify_program(&artifact.program).is_err(), "{label}");
    let bytes = DefaultOptions::new()
        .with_fixint_encoding()
        .with_little_endian()
        .serialize(artifact)
        .unwrap();
    assert!(
        !KbcArtifact::from_bytes(&bytes)
            .is_ok_and(|artifact| artifact.validate_for_loader(&Default::default()).is_ok()),
        "encoded {label}"
    );
}
#[test]
fn terminal_destinations_reject_wrong_source_factory_predicate_and_output_applications() {
    let mut checked = 0;
    for (item, destination, values) in [
        ("i32", "ArrayList<i32>", "20,22"),
        ("i32", "Bag<i32>", "20,22"),
        ("Key", "LinkedHashSet<Key>", "Key{value:20},Key{value:22}"),
        (
            "(Key,i32)",
            "LinkedHashMap<Key,i32>",
            "(Key{value:20},0),(Key{value:22},1)",
        ),
        ("Option<i32>", "Option<ArrayList<i32>>", "Some(20),Some(22)"),
        (
            "Result<Option<i32>,String>",
            "Result<Option<Bag<i32>>,String>",
            "Ok(Some(20)),Ok(Some(22))",
        ),
    ] {
        for operation in [NativeDefaultMethod::Collect, NativeDefaultMethod::Partition] {
            let output = if operation == NativeDefaultMethod::Collect {
                destination.to_string()
            } else {
                format!("({destination},{destination})")
            };
            let call = if operation == NativeDefaultMethod::Collect {
                "source.collect()"
            } else {
                "source.partition(|item|true)"
            };
            let program = common::bytecode_ok(&format!(
                r#"
struct Bag<T>{{val items:ArrayList<T>}}
impl<T> FromIterator<T> for Bag<T>{{fn from_iter<I:Iterable<Item=T>>(source:I)->Self{{Bag{{items:source.iter().collect::<ArrayList<T>>()}}}}}}
struct Key{{val value:i32}}
impl PartialEq for Key{{fn eq(self,other:Self)->bool{{self.value==other.value}}}}impl Eq for Key{{}}impl Hash for Key{{fn hash(self)->i64{{0i64}}}}
struct Cursor<T>{{val items:ArrayList<T>,var index:usize}}
impl<T> Iterator for Cursor<T>{{type Item=T;fn next(self)->Option<T>{{val value=self.items.get(self.index);self.index+=1usize;value}}}}
fn main(){{val items:ArrayList<{item}> =[{values}];val source=Cursor{{items,index:0}};val result:{output} = {call};}}
"#
            ));
            let root = program.root.index();
            let index = program.modules[root]
                .native_imports
                .iter()
                .position(|import| import.binding == EngineNativeBinding::TraitDefault(operation))
                .unwrap();
            let artifact = KbcArtifact::from_program(program, Default::default()).unwrap();
            for mutation in 0..23 {
                let mut forged = artifact.clone();
                let import = &mut forged.program.modules[root].native_imports[index];
                let boolean = AbiType::Builtin(BuiltinType::Bool);
                let factory = import
                    .witnesses
                    .iter()
                    .position(|witness| {
                        StandardTrait::from_id(&witness.interface.declaration)
                            == Some(StandardTrait::FromIterator)
                    })
                    .unwrap();
                let next = import
                    .witnesses
                    .iter()
                    .position(|witness| {
                        StandardTrait::from_id(&witness.interface.declaration)
                            == Some(StandardTrait::Iterator)
                            && witness.receiver == import.signature.params[0]
                    })
                    .unwrap();
                match mutation {
                    0 => import.binding_version -= 1,
                    1 => import.signature.params.clear(),
                    2 => import.signature.params[0] = boolean,
                    3 => import.signature.result = boolean,
                    4 => import.instance.arguments[0] = boolean,
                    5 => import.requirements.clear(),
                    6 => import.witnesses.clear(),
                    7 => import.witnesses[factory].receiver = boolean,
                    8 => import.witnesses[factory].interface.arguments[0] = boolean,
                    9 => {
                        import.witnesses[factory].implementation =
                            NativeWitnessImplementation::Primitive
                    }
                    10 => import.witnesses[factory]
                        .methods
                        .push(import.instance.clone()),
                    11 => import.witnesses.push(NativeWitness {
                        receiver: boolean,
                        interface: intrinsic::applied(StandardTrait::Eq, vec![]),
                        implementation: NativeWitnessImplementation::Primitive,
                        methods: vec![],
                    }),
                    12 => {
                        import.binding = EngineNativeBinding::TraitDefault(
                            if operation == NativeDefaultMethod::Collect {
                                NativeDefaultMethod::Partition
                            } else {
                                NativeDefaultMethod::Collect
                            },
                        )
                    }
                    13 => import.witnesses[next].receiver = boolean,
                    14 => {
                        let witness = &mut import.witnesses[next];
                        witness.interface.associated_types.insert(
                            associated_type_id(&witness.interface.declaration, "Item"),
                            boolean,
                        );
                    }
                    15 => {
                        import.instance.declaration.path.last_mut().unwrap().name = "forged".into()
                    }
                    16 => {
                        import.witnesses.remove(factory);
                    }
                    17 => {
                        let NativeWitnessImplementation::Table(instance) =
                            &mut import.witnesses[factory].implementation
                        else {
                            panic!()
                        };
                        instance.arguments.push(boolean);
                    }
                    18 => import.signature.params.push(boolean),
                    19 => import.instance.arguments.push(boolean),
                    20 => {
                        if operation == NativeDefaultMethod::Partition {
                            let AbiType::Function { params, .. } = &mut import.signature.params[1]
                            else {
                                panic!()
                            };
                            params[0] = boolean;
                        } else {
                            import.witnesses[next].methods.clear();
                        }
                    }
                    21 => {
                        if operation == NativeDefaultMethod::Partition {
                            let AbiType::Tuple(outputs) = &mut import.signature.result else {
                                panic!()
                            };
                            outputs[1] = boolean;
                        } else {
                            import.witnesses[next].methods[0].arguments.clear();
                        }
                    }
                    _ => {
                        if operation == NativeDefaultMethod::Partition {
                            let AbiType::Function { result, .. } = &mut import.signature.params[1]
                            else {
                                panic!()
                            };
                            **result = AbiType::Builtin(BuiltinType::I32);
                        } else {
                            import.signature.params[0] =
                                AbiType::Array(Box::new(boolean), CollectionAccess::Mutable);
                        }
                    }
                }
                rejected(&forged, &format!("{operation:?} {destination} {mutation}"));
                checked += 1;
            }
            let factory = artifact.program.modules[root].native_imports[index]
                .witnesses
                .iter()
                .find(|witness| {
                    StandardTrait::from_id(&witness.interface.declaration)
                        == Some(StandardTrait::FromIterator)
                        && !witness.methods.is_empty()
                });
            if let Some(factory) = factory {
                let mut forged = artifact.clone();
                let import = &mut forged.program.modules[root].native_imports[index];
                let source = if !matches!(
                    factory.methods[0].arguments.last(),
                    Some(AbiType::Array(..))
                ) {
                    AbiType::Array(
                        Box::new(factory.interface.arguments[0].clone()),
                        CollectionAccess::Mutable,
                    )
                } else {
                    import.signature.params[0].clone()
                };
                let witness = import
                    .witnesses
                    .iter_mut()
                    .find(|witness| witness == &factory)
                    .unwrap();
                *witness.methods[0].arguments.last_mut().unwrap() = source;
                rejected(
                    &forged,
                    &format!("{operation:?} {destination} wrong factory source"),
                );
                checked += 1;
            }
        }
    }
    assert_eq!(checked, 280);
}
