use crate::tests::common;
use bincode::{DefaultOptions, Options};
use kagari_abi::{
    callable::{EngineNativeBinding, NativeCall},
    native_import::NativeWitnessImplementation,
    scalar::BuiltinType,
    standard::{RuntimePrimitive, bindings::NativeProtocolMethod, traits::StandardTrait},
    types::AbiType,
};
use kagari_bytecode::{BytecodeInstruction, CallTarget, KbcArtifact, verify_program};
use kagari_common::{collection::CollectionAccess, identity::associated_type_id};

#[test]
fn array_copy_rejects_forged_traversal_and_mutation_authority() {
    for method in ["from", "copy", "extend", "from_iter"] {
        for source in ["native", "proxy", "dynamic", "custom"] {
            let setup = match source {
                "native" => "val source=[20,22];",
                "dynamic" if method == "from_iter" => {
                    "val source:Iterable<Item=i32,Iter=Iter<i32>> =Proxy{items:[20,22]};"
                }
                "dynamic" => "val source:List<i32> =Proxy{items:[20,22]};",
                "custom" if method == "from_iter" => "val source=Span{items:[20,22]};",
                _ => "val source=Proxy{items:[20,22]};",
            };
            let body = match method {
                "from" => "val output:ArrayList<i32> =ArrayList::from(source);",
                "from_iter" => "val output:ArrayList<i32> =ArrayList::from_iter(source);",
                "copy" => "val output=[0,0];output.copy_from(source);",
                _ => "val output=[0];output.extend(source);",
            };
            let program = common::bytecode_ok(&format!(
                r#"
struct Proxy<T>{{val items:ArrayList<T>}}
impl<T> Index<usize> for Proxy<T>{{type Output=T;fn index(self,i:usize)->T{{self.items[i]}}}}
impl<T> Iterable for Proxy<T>{{type Item=T;type Iter=Iter<T>;fn iter(self)->Iter<T>{{self.items.iter()}}}}
impl<T> List<T> for Proxy<T>{{fn len(self)->usize{{self.items.len()}}fn is_empty(self)->bool{{self.items.is_empty()}}fn get(self,i:usize)->Option<T>{{self.items.get(i)}}}}
struct Cursor<T>{{val items:ArrayList<T>,var index:usize}}
impl<T> Iterator for Cursor<T>{{type Item=T;fn next(self)->Option<T>{{val item=self.items.get(self.index);self.index+=1usize;item}}}}
struct Span<T>{{val items:ArrayList<T>}}
impl<T> Iterable for Span<T>{{type Item=T;type Iter=Cursor<T>;fn iter(self)->Cursor<T>{{Cursor{{items:self.items,index:0usize}}}}}}
fn main(){{{setup}{body}}}
"#
            ));
            let root = program.root.index();
            let binding = match method {
                "from" => EngineNativeBinding::Intrinsic(RuntimePrimitive::ArrayListFrom),
                "copy" => EngineNativeBinding::Intrinsic(RuntimePrimitive::ArrayCopyFrom),
                "extend" => EngineNativeBinding::Intrinsic(RuntimePrimitive::ArrayExtend),
                _ => EngineNativeBinding::Protocol(NativeProtocolMethod::CollectionFromIterator),
            };
            let import = program.modules[root]
                .native_imports
                .iter()
                .position(|import| import.binding == binding)
                .unwrap();
            let artifact = KbcArtifact::from_program(program, Default::default()).unwrap();
            for mutation in 0..14 {
                let mut forged = artifact.clone();
                let module = &mut forged.program.modules[root];
                let contract = &mut module.native_imports[import];
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
                    2 => contract.witnesses[next].receiver = AbiType::Builtin(BuiltinType::I32),
                    3 => {
                        let witness = &mut contract.witnesses[iterable];
                        witness.interface.associated_types.insert(
                            associated_type_id(&witness.interface.declaration, "Item"),
                            AbiType::Builtin(BuiltinType::Bool),
                        );
                    }
                    4 => {
                        let witness = &mut contract.witnesses[next];
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
                    8 | 9 => {
                        let array = if matches!(method, "copy" | "extend") {
                            &mut contract.signature.params[0]
                        } else {
                            &mut contract.signature.result
                        };
                        let AbiType::Array(item, access) = array else {
                            panic!()
                        };
                        if mutation == 8 {
                            *access = CollectionAccess::ReadOnly;
                        } else {
                            **item = AbiType::Builtin(BuiltinType::Bool);
                        }
                    }
                    10 => contract
                        .signature
                        .params
                        .push(AbiType::Builtin(BuiltinType::I32)),
                    11 => {
                        contract.binding =
                            EngineNativeBinding::Intrinsic(RuntimePrimitive::ArrayListFromFn)
                    }
                    12 => contract
                        .instance
                        .arguments
                        .push(AbiType::Builtin(BuiltinType::Bool)),
                    _ => {
                        let mut replaced = false;
                        for function in &mut module.functions {
                            for instruction in &mut function.instructions {
                                if let BytecodeInstruction::Call { callee, .. } = instruction
                                    && matches!(callee,CallTarget::Native(id) if id.index()==import)
                                {
                                    *callee = CallTarget::RuntimePrimitive(
                                        RuntimePrimitive::ArrayListFrom,
                                    );
                                    replaced = true;
                                }
                            }
                        }
                        assert!(replaced);
                    }
                }
                assert!(
                    verify_program(&forged.program).is_err(),
                    "{method} {source} {mutation}"
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
                    "encoded {method} {source} {mutation}"
                );
            }
        }
    }
}
