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
use kagari_bytecode::{artifact::KbcArtifact, program::verify_program};
#[test]
fn list_queries_reject_forged_storage_and_script_selections() {
    for (binding, call) in [
        (NativeDefaultMethod::ListFirst, "first()"),
        (NativeDefaultMethod::ListLast, "last()"),
        (NativeDefaultMethod::ListBinarySearch, "binary_search(1)"),
    ] {
        for source in [
            "val source=[1];",
            "val source=Sequence{items:[1]};",
            "val source:List<i32> =Sequence{items:[1]};",
            "val source:List<i32> =[1];",
        ] {
            let program = common::bytecode_ok(&format!(
                r#"
struct Sequence<T> {{val items:ArrayList<T>}}
impl<T> Iterable for Sequence<T> {{type Item=T;type Iter=Iter<T>;fn iter(self)->Iter<T>{{self.items.iter()}}}}
impl<T> Index<usize> for Sequence<T> {{type Output=T;fn index(self,index:usize)->T{{self.items[index]}}}}
impl<T> List<T> for Sequence<T> {{fn len(self)->usize{{self.items.len()}}fn is_empty(self)->bool{{self.items.is_empty()}}fn get(self,index:usize)->Option<T>{{self.items.get(index)}}}}
fn main()->i32 {{{source}val out=source.{call};42}}
"#
            ));
            let root = program.root.index();
            let import = program.modules[root]
                .native_imports
                .iter()
                .position(|import| import.binding == EngineNativeBinding::TraitDefault(binding))
                .unwrap();
            let list = program.modules[root].native_imports[import]
                .witnesses
                .iter()
                .position(|witness| {
                    StandardTrait::from_id(&witness.interface.declaration)
                        == Some(StandardTrait::List)
                })
                .unwrap();
            verify_program(&program).unwrap();
            let artifact = KbcArtifact::from_program(program, Default::default()).unwrap();
            for mutation in 0..12 {
                let mut forged = artifact.clone();
                let contract = &mut forged.program.modules[root].native_imports[import];
                match mutation {
                    0 => {
                        contract.witnesses.remove(list);
                    }
                    1 => contract.witnesses.push(contract.witnesses[list].clone()),
                    2 => {
                        contract.witnesses[list].interface.arguments[0] =
                            AbiType::Builtin(BuiltinType::U32)
                    }
                    3 => contract.signature.result = AbiType::Builtin(BuiltinType::Bool),
                    4 => contract.instance.arguments.clear(),
                    5 => {
                        let witness = &mut contract.witnesses[list];
                        if witness.methods.len() > 1 {
                            witness.methods.swap(0, 2);
                        } else {
                            witness.methods.push(contract.instance.clone());
                        }
                    }
                    6 => contract.witnesses[list]
                        .methods
                        .push(contract.instance.clone()),
                    7 => {
                        contract.witnesses[list].implementation =
                            NativeWitnessImplementation::Primitive
                    }
                    8 => {
                        let witness = &mut contract.witnesses[list];
                        match &mut witness.implementation {
                            NativeWitnessImplementation::Table(instance) => {
                                instance.arguments.clear()
                            }
                            _ => witness.methods.push(contract.instance.clone()),
                        }
                    }
                    9 => contract.witnesses[list].receiver = AbiType::Builtin(BuiltinType::I32),
                    10 => {
                        contract.witnesses[list].interface.declaration =
                            traits::identity(StandardTrait::Iterator)
                    }
                    _ => {
                        contract.binding = EngineNativeBinding::TraitDefault(
                            if binding == NativeDefaultMethod::ListFirst {
                                NativeDefaultMethod::ListLast
                            } else {
                                NativeDefaultMethod::ListFirst
                            },
                        )
                    }
                }
                assert!(
                    verify_program(&forged.program).is_err(),
                    "{binding:?} {source} {mutation}"
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
                    "encoded {binding:?} {source} {mutation}"
                );
            }
        }
    }
}

#[test]
fn list_queries_reject_forged_required_shapes_and_parent_contracts() {
    use kagari_abi::{callable::CallableImplementation, types::PublicAbiItem};
    let program = common::bytecode_ok(
        "fn main()->i32{val source:List<i32> = [1];val a=source.first();val b=source.last();val c=source.binary_search(1);42}",
    );
    let artifact = KbcArtifact::from_program(program, Default::default()).unwrap();
    for mutation in 0..8 {
        let mut forged = artifact.clone();
        let list = forged
            .program
            .modules
            .iter_mut()
            .flat_map(|module| &mut module.public_items)
            .find_map(|item| match item {
                PublicAbiItem::Trait(trait_) if trait_.name == "List" => Some(trait_),
                _ => None,
            })
            .unwrap();
        let required: Vec<_> = list
            .methods
            .iter()
            .enumerate()
            .filter(|(_, method)| method.implementation == CallableImplementation::Required)
            .map(|(slot, _)| slot)
            .collect();
        match mutation {
            0 => list.methods[required[0]].return_type = AbiType::Builtin(BuiltinType::I32),
            1 => list.methods[required[2]].return_type = AbiType::Builtin(BuiltinType::Bool),
            2 => list.methods[required[2]].params[1].ty = AbiType::Builtin(BuiltinType::U32),
            3 => list.supertraits.clear(),
            4 | 5 => {
                let iterable = list
                    .supertraits
                    .iter_mut()
                    .find(|parent| {
                        StandardTrait::from_id(&parent.declaration) == Some(StandardTrait::Iterable)
                    })
                    .unwrap();
                let (_, output) = iterable
                    .associated_types
                    .iter_mut()
                    .find(|(id, _)| {
                        id.path.last().unwrap().name == if mutation == 4 { "Item" } else { "Iter" }
                    })
                    .unwrap();
                *output = AbiType::Builtin(BuiltinType::U32);
            }
            6 => list.methods.push(list.methods[required[2]].clone()),
            _ => list.methods[required[2]].implementation = CallableImplementation::Script,
        }
        assert!(
            verify_program(&forged.program).is_err(),
            "List declaration mutation {mutation}"
        );
        let bytes = DefaultOptions::new()
            .with_fixint_encoding()
            .with_little_endian()
            .serialize(&forged)
            .unwrap();
        assert!(
            !KbcArtifact::from_bytes(&bytes)
                .is_ok_and(|decoded| decoded.validate_for_loader(&Default::default()).is_ok()),
            "encoded List declaration mutation {mutation}"
        );
    }
}
