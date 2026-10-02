use crate::tests::common;
use bincode::{DefaultOptions, Options};
use kagari_abi::{
    callable::EngineNativeBinding,
    language::Protocol,
    native_import::{NativeWitness, NativeWitnessImplementation},
    scalar::BuiltinType,
    standard::{RuntimePrimitive, intrinsic, surface::StandardEnum},
    types::{AbiType, ConstraintAbi, GenericBoundAbi, PublicAbiItem},
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
            .is_ok_and(|decoded| decoded.validate_for_loader(&Default::default()).is_ok()),
        "encoded {label}"
    );
}

#[test]
fn protocol_entries_reject_forged_signatures_witnesses_and_required_methods() {
    let mut checked = 0;
    for (body, operation) in [
        (
            "val result=\"42\".parse::<i32>();",
            RuntimePrimitive::StringParse,
        ),
        (
            "val result=\"42\".parse::<Wrapped<i32>>();",
            RuntimePrimitive::StringParse,
        ),
        (
            "std::debug::assert_eq(42,42,\"equal\");",
            RuntimePrimitive::AssertEq,
        ),
        (
            "std::debug::assert_eq(Wrapped{value:42},Wrapped{value:42},\"equal\");",
            RuntimePrimitive::AssertEq,
        ),
        (
            "std::debug::assert_eq(Some(Wrapped{value:42}),Some(Wrapped{value:42}),\"equal\");",
            RuntimePrimitive::AssertEq,
        ),
        (
            "val source:List<i32> =[42];std::debug::assert_eq(source,source,\"equal\");",
            RuntimePrimitive::AssertEq,
        ),
    ] {
        let program = common::bytecode_ok(&format!(
            r#"
struct Wrapped<T> {{val value:T}}
impl<T:FromStr> FromStr for Wrapped<T> {{type Err=<T as FromStr>::Err;fn from_str(text:String)->Result<Self,Self::Err>{{text.parse::<T>().map(|value|Wrapped{{value:value}})}}}}
impl<T:PartialEq> PartialEq for Wrapped<T> {{fn eq(self,other:Self)->bool{{self.value==other.value}}}}
fn main(){{{body}}}
"#
        ));
        let root = program.root.index();
        let index = program.modules[root]
            .native_imports
            .iter()
            .position(|import| import.binding == EngineNativeBinding::Intrinsic(operation))
            .unwrap();
        let artifact = KbcArtifact::from_program(program, Default::default()).unwrap();
        for mutation in 0..19 {
            let mut forged = artifact.clone();
            let import = &mut forged.program.modules[root].native_imports[index];
            let boolean = AbiType::Builtin(BuiltinType::Bool);
            match mutation {
                0 => import.binding_version -= 1,
                1 => import.signature.params.clear(),
                2 => import.signature.params[0] = boolean,
                3 => import.signature.result = AbiType::Builtin(BuiltinType::Never),
                4 => import.instance.arguments[0] = boolean,
                5 => import.requirements.clear(),
                6 => import.witnesses.clear(),
                7 => import.witnesses[0].receiver = boolean,
                8 => import.witnesses[0].interface = intrinsic::applied(Protocol::Eq, vec![]),
                9 => import.witnesses[0].implementation = NativeWitnessImplementation::Host,
                10 => import.witnesses[0].methods.push(import.instance.clone()),
                11 => import.witnesses.push(NativeWitness {
                    receiver: boolean,
                    interface: intrinsic::applied(Protocol::Eq, vec![]),
                    implementation: NativeWitnessImplementation::Primitive,
                    methods: vec![],
                }),
                12 => {
                    import.binding = EngineNativeBinding::Intrinsic(
                        if operation == RuntimePrimitive::StringParse {
                            RuntimePrimitive::AssertEq
                        } else {
                            RuntimePrimitive::StringParse
                        },
                    )
                }
                13 => import.requirements.push(GenericBoundAbi {
                    ty: boolean,
                    constraints: vec![ConstraintAbi::Trait(intrinsic::applied(
                        Protocol::Eq,
                        vec![],
                    ))],
                }),
                14 => import.signature.params.push(boolean),
                15 => import.instance.declaration.path.last_mut().unwrap().name = "forged".into(),
                16 => import.witnesses[0].interface.arguments.push(boolean),
                17 => {
                    let selected = &mut import.witnesses[0];
                    selected.interface.associated_types.insert(
                        associated_type_id(&selected.interface.declaration, "Err"),
                        boolean,
                    );
                }
                _ => import.instance.arguments.push(boolean),
            }
            rejected(&forged, &format!("{body} {mutation}"));
            checked += 1;
        }
    }
    assert_eq!(checked, 114);
}

#[test]
fn generic_from_str_keeps_exact_associated_error_and_executable_method_signatures() {
    let program = common::bytecode_ok(
        r#"
struct Wrapped<T> {val value:T}
impl<T:FromStr> FromStr for Wrapped<T> {type Err=<T as FromStr>::Err;fn from_str(text:String)->Result<Self,Self::Err>{text.parse::<T>().map(|value|Wrapped{value:value})}}
fn main(){val result="bad".parse::<Wrapped<i32>>();}
"#,
    );
    let root = program.root.index();
    let table_index=program.modules[root].public_items.iter().position(|item|matches!(item,PublicAbiItem::InterfaceTable(table) if matches!(&table.trait_type,AbiType::Trait(interface) if Protocol::from_id(&interface.declaration)==Some(Protocol::FromStr)))).unwrap();
    let slot = program.modules[root]
        .interface_tables
        .iter()
        .find(|table| {
            let PublicAbiItem::InterfaceTable(abi) =
                &program.modules[root].public_items[table_index]
            else {
                panic!()
            };
            table.declaration == abi.declaration
        })
        .unwrap()
        .methods[0]
        .function
        .index();
    let artifact = KbcArtifact::from_program(program, Default::default()).unwrap();
    for mutation in 0..5 {
        let mut forged = artifact.clone();
        let module = &mut forged.program.modules[root];
        let function = &mut module.functions[slot];
        match mutation {
            0 => {
                let Some(AbiType::StandardEnum {
                    kind: StandardEnum::Result,
                    args,
                }) = &mut function.metadata.semantic.result
                else {
                    panic!()
                };
                args[1] = AbiType::Builtin(BuiltinType::Bool);
            }
            1 => {
                function
                    .metadata
                    .semantic
                    .params
                    .insert(0, AbiType::Builtin(BuiltinType::I32));
            }
            2 => {
                let PublicAbiItem::InterfaceTable(table) = &mut module.public_items[table_index]
                else {
                    panic!()
                };
                let AbiType::Trait(interface) = &mut table.trait_type else {
                    panic!()
                };
                interface.associated_types.insert(
                    associated_type_id(&interface.declaration, "Err"),
                    AbiType::Builtin(BuiltinType::Bool),
                );
            }
            3 => {
                let index=module.native_imports.iter().position(|import|import.binding==EngineNativeBinding::Intrinsic(RuntimePrimitive::StringParse) && matches!(&import.signature.result,AbiType::StandardEnum{args,..} if matches!(args[0],AbiType::Struct(_)))).unwrap();
                let AbiType::StandardEnum { args, .. } =
                    &mut module.native_imports[index].signature.result
                else {
                    panic!()
                };
                args[1] = AbiType::Builtin(BuiltinType::Bool);
            }
            _ => {
                let index=module.native_imports.iter().position(|import|import.binding==EngineNativeBinding::Intrinsic(RuntimePrimitive::StringParse) && matches!(&import.signature.result,AbiType::StandardEnum{args,..} if matches!(args[0],AbiType::Struct(_)))).unwrap();
                module.native_imports[index]
                    .witnesses
                    .iter_mut()
                    .find(|witness| !witness.methods.is_empty())
                    .unwrap()
                    .methods
                    .clear();
            }
        }
        rejected(&forged, &format!("associated error/method {mutation}"));
    }
}
