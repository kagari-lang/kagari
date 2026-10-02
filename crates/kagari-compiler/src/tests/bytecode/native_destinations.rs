use crate::tests::common;
use bincode::{DefaultOptions, Options};
use kagari_abi::{
    callable::EngineNativeBinding,
    language::Protocol,
    native_import::{NativeWitness, NativeWitnessImplementation},
    scalar::BuiltinType,
    standard::{bindings::NativeProtocolMethod, intrinsic},
    types::AbiType,
};
use kagari_bytecode::{artifact::KbcArtifact, program::verify_program};
use kagari_common::identity::associated_type_id;

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
fn fallible_destinations_reject_forged_sources_factories_and_nested_methods() {
    let mut checked = 0;
    for (output, item, values) in [
        ("Option<ArrayList<i32>>", "Option<i32>", "Some(42)"),
        ("Option<Bag<i32>>", "Option<i32>", "Some(42)"),
        (
            "Result<LinkedHashSet<Key>,String>",
            "Result<Key,String>",
            "Ok(Key{value:42})",
        ),
        (
            "Result<LinkedHashMap<Key,i32>,String>",
            "Result<(Key,i32),String>",
            "Ok((Key{value:42},42))",
        ),
        (
            "Result<Option<Bag<i32>>,String>",
            "Result<Option<i32>,String>",
            "Ok(Some(42))",
        ),
        (
            "Option<Result<ArrayList<i32>,String>>",
            "Option<Result<i32,String>>",
            "Some(Ok(42))",
        ),
    ] {
        let program = common::bytecode_ok(&format!(
            r#"
struct Bag<T>{{val items:ArrayList<T>}}
impl<T> FromIterator<T> for Bag<T>{{fn from_iter<I:Iterable<Item=T>>(source:I)->Self{{Bag{{items:source.iter().collect::<ArrayList<T>>()}}}}}}
struct Key{{val value:i32}}
impl PartialEq for Key{{fn eq(self,other:Self)->bool{{self.value==other.value}}}}impl Eq for Key{{}}impl Hash for Key{{fn hash(self)->i64{{0i64}}}}
fn main(){{val items:ArrayList<{item}> =[{values}];val result:{output} = <{output} as FromIterator<{item}>>::from_iter(items.iter());}}
"#
        ));
        let root = program.root.index();
        let index = program.modules[root]
            .native_imports
            .iter()
            .position(|import| {
                matches!(
                    import.binding,
                    EngineNativeBinding::Protocol(
                        NativeProtocolMethod::OptionFromIterator
                            | NativeProtocolMethod::ResultFromIterator
                    )
                )
            })
            .unwrap();
        let artifact = KbcArtifact::from_program(program, Default::default()).unwrap();
        for mutation in 0..20 {
            let mut forged = artifact.clone();
            let import = &mut forged.program.modules[root].native_imports[index];
            let boolean = AbiType::Builtin(BuiltinType::Bool);
            let factory = import
                .witnesses
                .iter()
                .position(|witness| {
                    Protocol::from_id(&witness.interface.declaration)
                        == Some(Protocol::FromIterator)
                })
                .unwrap();
            let next = import
                .witnesses
                .iter()
                .position(|witness| {
                    Protocol::from_id(&witness.interface.declaration) == Some(Protocol::Iterator)
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
                    interface: intrinsic::applied(Protocol::Eq, vec![]),
                    implementation: NativeWitnessImplementation::Primitive,
                    methods: vec![],
                }),
                12 => {
                    import.binding =
                        EngineNativeBinding::Protocol(NativeProtocolMethod::CollectionFromIterator)
                }
                13 => import.witnesses[next].receiver = boolean,
                14 => {
                    let witness = &mut import.witnesses[next];
                    witness.interface.associated_types.insert(
                        associated_type_id(&witness.interface.declaration, "Item"),
                        boolean,
                    );
                }
                15 => import.instance.declaration.path.last_mut().unwrap().name = "forged".into(),
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
                _ => import.instance.arguments.push(boolean),
            }
            rejected(&forged, &format!("{output} {mutation}"));
            checked += 1;
        }
        let selected = artifact.program.modules[root].native_imports[index]
            .witnesses
            .iter()
            .find(|witness| {
                Protocol::from_id(&witness.interface.declaration) == Some(Protocol::FromIterator)
                    && !witness.methods.is_empty()
            });
        if let Some(selected) = selected {
            for mutation in 0..3 {
                let mut forged = artifact.clone();
                let module = &mut forged.program.modules[root];
                if mutation == 0 {
                    let witness = module.native_imports[index]
                        .witnesses
                        .iter_mut()
                        .find(|witness| witness == &selected)
                        .unwrap();
                    witness.methods[0].arguments.pop();
                } else {
                    let function = module
                        .functions
                        .iter_mut()
                        .find(|function| function.identity.as_ref() == selected.methods.first())
                        .unwrap();
                    if mutation == 1 {
                        function
                            .metadata
                            .semantic
                            .params
                            .insert(0, AbiType::Builtin(BuiltinType::Bool));
                    } else {
                        function.metadata.semantic.result =
                            Some(AbiType::Builtin(BuiltinType::Bool));
                    }
                }
                rejected(&forged, &format!("script method {output} {mutation}"));
                checked += 1;
            }
        }
    }
    assert_eq!(checked, 126);
}
