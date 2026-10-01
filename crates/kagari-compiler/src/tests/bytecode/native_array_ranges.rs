use crate::tests::common;
use bincode::{DefaultOptions, Options};
use kagari_abi::{
    callable::{EngineNativeBinding, NativeCall},
    native_import::NativeWitnessImplementation,
    scalar::BuiltinType,
    standard::{RuntimePrimitive, traits::StandardTrait},
    types::AbiType,
};
use kagari_bytecode::{
    artifact::KbcArtifact,
    instruction::{BytecodeInstruction, CallTarget},
    program::verify_program,
};
use kagari_common::collection::CollectionAccess;
#[test]
fn array_intervals_reject_forged_bounds_and_result_authority() {
    for method in ["copy", "remove"] {
        for source in ["bounded", "full", "custom", "generic"] {
            let range = match source {
                "bounded" => "0usize..2usize",
                "full" => "..",
                _ => "Region{lower:0usize,upper:2usize}",
            };
            let action = match (method, source) {
                ("copy", "generic") => "copy(a,r);",
                ("copy", _) => "a.copy_within(r,0usize);",
                ("remove", "generic") => "val output:List<i32> =remove(a,r);",
                _ => "val output:List<i32> =a.remove_range(r);",
            };
            let program = common::bytecode_ok(&format!(
                r#"
struct Region{{val lower:usize,val upper:usize}}
impl RangeBounds<usize> for Region{{fn start_bound(self)->Bound<usize>{{Bound::Included(self.lower)}}fn end_bound(self)->Bound<usize>{{Bound::Excluded(self.upper)}}}}
fn copy<R:RangeBounds<usize>>(a:ArrayList<i32>,r:R){{a.copy_within(r,0usize);}}
fn remove<R:RangeBounds<usize>>(a:ArrayList<i32>,r:R)->List<i32>{{a.remove_range(r)}}
fn main(){{val a=[20,22];val r={range};{action}}}
"#
            ));
            let binding = EngineNativeBinding::Intrinsic(if method == "copy" {
                RuntimePrimitive::ArrayCopyWithin
            } else {
                RuntimePrimitive::ArrayRemoveRange
            });
            let root = program.root.index();
            let import = program.modules[root]
                .native_imports
                .iter()
                .position(|i| i.binding == binding)
                .unwrap();
            let artifact = KbcArtifact::from_program(program, Default::default()).unwrap();
            for mutation in 0..16 {
                let mut forged = artifact.clone();
                let module = &mut forged.program.modules[root];
                let contract = &mut module.native_imports[import];
                let witness = contract
                    .witnesses
                    .iter()
                    .position(|w| {
                        StandardTrait::from_id(&w.interface.declaration)
                            == Some(StandardTrait::RangeBounds)
                    })
                    .unwrap();
                match mutation {
                    0 => {
                        contract.witnesses.remove(witness);
                    }
                    1 => contract.witnesses[witness].receiver = AbiType::Builtin(BuiltinType::Bool),
                    2 => {
                        contract.witnesses[witness].interface.arguments[0] =
                            AbiType::Builtin(BuiltinType::I32)
                    }
                    3 => {
                        contract.witnesses[witness].implementation =
                            NativeWitnessImplementation::Primitive
                    }
                    4 => {
                        contract.witnesses[witness].implementation =
                            NativeWitnessImplementation::Interface
                    }
                    5 => {
                        if contract.witnesses[witness].methods.is_empty() {
                            contract.witnesses[witness]
                                .methods
                                .push(contract.instance.clone())
                        } else {
                            contract.witnesses[witness].methods.pop();
                        }
                    }
                    6 => {
                        if contract.witnesses[witness].methods.len() == 2 {
                            contract.witnesses[witness].methods.swap(0, 1)
                        } else {
                            contract.witnesses[witness]
                                .methods
                                .push(contract.instance.clone())
                        }
                    }
                    7 => {
                        let NativeWitnessImplementation::Table(instance) =
                            &mut contract.witnesses[witness].implementation
                        else {
                            panic!()
                        };
                        instance.arguments.push(AbiType::Builtin(BuiltinType::Bool));
                    }
                    8 => contract.requirements.clear(),
                    9 => {
                        let AbiType::Array(_, access) = &mut contract.signature.params[0] else {
                            panic!()
                        };
                        *access = CollectionAccess::ReadOnly;
                    }
                    10 => {
                        contract
                            .signature
                            .params
                            .push(AbiType::Builtin(BuiltinType::Bool));
                    }
                    11 => {
                        contract.signature.result = AbiType::Array(
                            Box::new(AbiType::Builtin(BuiltinType::I32)),
                            CollectionAccess::Mutable,
                        )
                    }
                    12 => {
                        if method == "remove" {
                            let factory = contract
                                .witnesses
                                .iter()
                                .position(|w| {
                                    StandardTrait::from_id(&w.interface.declaration)
                                        == Some(StandardTrait::List)
                                })
                                .unwrap();
                            contract.witnesses.remove(factory);
                        } else {
                            contract.signature.params[2] = AbiType::Builtin(BuiltinType::I32)
                        }
                    }
                    13 => {
                        if method == "remove" {
                            let factory = contract
                                .witnesses
                                .iter_mut()
                                .find(|w| {
                                    StandardTrait::from_id(&w.interface.declaration)
                                        == Some(StandardTrait::List)
                                })
                                .unwrap();
                            let AbiType::Array(_, access) = &mut factory.receiver else {
                                panic!()
                            };
                            *access = CollectionAccess::ReadOnly;
                        } else {
                            contract.binding =
                                EngineNativeBinding::Intrinsic(RuntimePrimitive::ArrayCopyFrom)
                        }
                    }
                    14 => contract
                        .instance
                        .arguments
                        .push(AbiType::Builtin(BuiltinType::Bool)),
                    _ => {
                        let mut replaced = false;
                        for f in &mut module.functions {
                            for instruction in &mut f.instructions {
                                if let BytecodeInstruction::Call { callee, .. } = instruction
                                    && matches!(callee,CallTarget::Native(id) if id.index()==import)
                                {
                                    *callee = CallTarget::RuntimePrimitive(
                                        RuntimePrimitive::ArrayCopyWithin,
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
