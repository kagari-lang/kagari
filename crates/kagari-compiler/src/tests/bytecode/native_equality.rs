use crate::tests::common;
use bincode::{DefaultOptions, Options};
use kagari_abi::{
    callable::EngineNativeBinding,
    native_import::NativeWitnessImplementation,
    scalar::BuiltinType,
    standard::{
        bindings::NativeDefaultMethod,
        traits::{self, StandardTrait},
    },
    types::AbiType,
};
use kagari_bytecode::{KbcArtifact, verify_program};

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
fn list_equality_rejects_bypassed_and_forged_composition_witnesses() {
    for (ty, value) in [
        ("i32", "1"),
        ("(i32,i32)", "(1,2)"),
        ("Key<i32>", "Key{value:1}"),
        ("(Key<i32>,i32)", "(Key{value:1},2)"),
        ("Option<Key<i32>>", "Some(Key{value:1})"),
        ("Choice<Key<i32>>", "Choice::Item(Key{value:1})"),
    ] {
        for (binding, call) in [
            (NativeDefaultMethod::ListContains, "contains(value)"),
            (NativeDefaultMethod::ListStartsWith, "starts_with(needle)"),
            (NativeDefaultMethod::ListEndsWith, "ends_with(needle)"),
        ] {
            let program = common::bytecode_ok(&format!(
                r#"
struct Key<T> {{val value:T}}
impl<T:PartialEq> PartialEq for Key<T> {{fn eq(self,other:Self)->bool{{self.value==other.value}}}}
enum Choice<T> {{Empty,Item(T)}}
fn main()->i32 {{val value:{ty} ={value};val source:ArrayList<{ty}> =[value];val needle:List<{ty}> =[value];val out=source.{call};42}}
"#
            ));
            let root = program.root.index();
            let import = program.modules[root]
                .native_imports
                .iter()
                .position(|import| import.binding == EngineNativeBinding::TraitDefault(binding))
                .unwrap();
            let equality = program.modules[root].native_imports[import]
                .witnesses
                .iter()
                .position(|witness| {
                    StandardTrait::from_id(&witness.interface.declaration)
                        == Some(StandardTrait::PartialEq)
                })
                .unwrap();
            let derived = program.modules[root].native_imports[import].witnesses[equality]
                .implementation
                == NativeWitnessImplementation::Derived;
            let artifact = KbcArtifact::from_program(program, Default::default()).unwrap();
            for mutation in 0..8 {
                let mut forged = artifact.clone();
                let contract = &mut forged.program.modules[root].native_imports[import];
                let witness = &mut contract.witnesses[equality];
                match mutation {
                    0 => {
                        contract.witnesses.remove(equality);
                    }
                    1 => {
                        let duplicate = witness.clone();
                        contract.witnesses.push(duplicate);
                    }
                    2 => witness.receiver = AbiType::Builtin(BuiltinType::U32),
                    3 => witness.interface.declaration = traits::identity(StandardTrait::Eq),
                    4 => witness.methods.push(contract.instance.clone()),
                    5 => {
                        witness.implementation =
                            if witness.implementation == NativeWitnessImplementation::Primitive {
                                NativeWitnessImplementation::Derived
                            } else {
                                NativeWitnessImplementation::Primitive
                            };
                        witness.methods.clear();
                    }
                    6 => contract.binding_version -= 1,
                    _ => {
                        let slot = contract
                            .witnesses
                            .iter()
                            .position(|witness| {
                                StandardTrait::from_id(&witness.interface.declaration)
                                    == Some(StandardTrait::Iterable)
                            })
                            .unwrap();
                        contract.witnesses.remove(slot);
                    }
                }
                reject(&forged, &format!("{binding:?} {ty} {mutation}"));
            }
            if derived {
                for mutation in 0..5 {
                    let mut forged = artifact.clone();
                    let witness = &mut forged.program.modules[root].native_imports[import]
                        .witnesses[equality];
                    match mutation {
                        0 => witness.methods.clear(),
                        1 => witness.methods[0].arguments.clear(),
                        2 => witness.methods[0].declaration.path[0].name = "$derived_Ord".into(),
                        3 => witness.methods[0].declaration.path[0].occurrence = 1,
                        _ => {
                            let target = witness.methods[0].clone();
                            let function = forged.program.modules[root]
                                .functions
                                .iter_mut()
                                .find(|function| function.identity.as_ref() == Some(&target))
                                .unwrap();
                            function.identity = None;
                            let id = function.id;
                            forged.program.modules[root].function_table[id.index()].identity = None;
                        }
                    }
                    reject(&forged, &format!("derived {binding:?} {ty} {mutation}"));
                }
            }
        }
    }
}
