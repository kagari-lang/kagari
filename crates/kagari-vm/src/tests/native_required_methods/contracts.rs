//! Required methods retain checked provider authority on source-free routes.
use super::cases;
use crate::tests::common::compile_test_bytecode;
use kagari_abi::{
    callable::EngineNativeBinding,
    native_import::{EngineCoreOperation, EngineNativeOperation},
    scalar::BuiltinType,
    standard::{StandardIntrinsic, bindings::NativeProtocolMethod},
    types::AbiType,
};
use kagari_bytecode::{KbcArtifact, verify_program};
use kagari_common::{collection::CollectionAccess, range::RangeKind};

#[test]
fn required_method_imports_reject_forged_signatures_and_providers() {
    let mut checked = 0;
    for (name, source) in cases::cases() {
        let binding = match name {
            "list_len" => EngineNativeBinding::Intrinsic(StandardIntrinsic::ArrayLen),
            "list_empty" => EngineNativeBinding::Intrinsic(StandardIntrinsic::ArrayIsEmpty),
            "map_len" => EngineNativeBinding::Intrinsic(StandardIntrinsic::MapLen),
            "map_empty" => EngineNativeBinding::Intrinsic(StandardIntrinsic::MapIsEmpty),
            "set_len" => EngineNativeBinding::Intrinsic(StandardIntrinsic::SetLen),
            "set_empty" => EngineNativeBinding::Intrinsic(StandardIntrinsic::SetIsEmpty),
            "list_get" => EngineNativeBinding::Intrinsic(StandardIntrinsic::ArrayGet),
            "list_pop" => EngineNativeBinding::Intrinsic(StandardIntrinsic::ArrayPop),
            "list_remove" => EngineNativeBinding::Intrinsic(StandardIntrinsic::ArrayRemove),
            "list_push" => EngineNativeBinding::Intrinsic(StandardIntrinsic::ArrayPush),
            "list_insert" => EngineNativeBinding::Intrinsic(StandardIntrinsic::ArrayInsert),
            "list_clear" => EngineNativeBinding::Intrinsic(StandardIntrinsic::ArrayClear),
            "list_set" => EngineNativeBinding::Protocol(NativeProtocolMethod::CollectionSet),
            "list_swap" => EngineNativeBinding::Intrinsic(StandardIntrinsic::ArraySwap),
            "list_reverse" => EngineNativeBinding::Intrinsic(StandardIntrinsic::ArrayReverse),
            "list_truncate" => EngineNativeBinding::Intrinsic(StandardIntrinsic::ArrayTruncate),
            "map_clear" => EngineNativeBinding::Intrinsic(StandardIntrinsic::MapClear),
            "set_clear" => EngineNativeBinding::Intrinsic(StandardIntrinsic::SetClear),
            "next_lazy" => EngineNativeBinding::Protocol(NativeProtocolMethod::IterNext),
            _ if name.starts_with("iter_") => {
                EngineNativeBinding::Protocol(NativeProtocolMethod::CollectionIter)
            }
            _ if name.starts_with("bounds_") => {
                EngineNativeBinding::Protocol(NativeProtocolMethod::RangeStartBound)
            }
            _ if name.starts_with("parse_") => {
                EngineNativeBinding::Protocol(NativeProtocolMethod::NumericFromStr)
            }
            _ => panic!("missing required method {name}"),
        };
        let program = compile_test_bytecode(&source);
        let root = program.root.index();
        let index = program.modules[root]
            .engine_imports
            .iter()
            .position(|import| import.binding == binding)
            .unwrap_or_else(|| panic!("{name}: missing checked import"));
        let contract = &program.modules[root].engine_imports[index];
        match binding {
            EngineNativeBinding::Protocol(NativeProtocolMethod::CollectionIter) => assert_eq!(
                contract.resolve(),
                Some(EngineNativeOperation::Core(EngineCoreOperation::IterNew))
            ),
            EngineNativeBinding::Protocol(NativeProtocolMethod::IterNext) => assert_eq!(
                contract.resolve(),
                Some(EngineNativeOperation::Core(EngineCoreOperation::IterNext))
            ),
            EngineNativeBinding::Protocol(NativeProtocolMethod::RangeStartBound) => {
                assert_eq!(
                    contract.resolve(),
                    Some(EngineNativeOperation::Core(
                        EngineCoreOperation::RangeStartBound
                    ))
                );
                assert!(
                    program.modules[root]
                        .engine_imports
                        .iter()
                        .any(|import| import.binding
                            == EngineNativeBinding::Protocol(NativeProtocolMethod::RangeEndBound)
                            && import.resolve()
                                == Some(EngineNativeOperation::Core(
                                    EngineCoreOperation::RangeEndBound
                                )))
                );
            }
            _ => assert!(contract.resolve().is_some(), "{name}"),
        }
        for mutation in 0..5 {
            let mut forged = program.clone();
            let import = &mut forged.modules[root].engine_imports[index];
            match mutation {
                0 => import.signature.params[0] = AbiType::Builtin(BuiltinType::Bool),
                1 => {
                    import.signature.result = AbiType::Builtin(
                        if import.signature.result == AbiType::Builtin(BuiltinType::Unit) {
                            BuiltinType::Bool
                        } else {
                            BuiltinType::Unit
                        },
                    )
                }
                2 => import.binding_version += 1,
                3 => {
                    import.binding =
                        EngineNativeBinding::Protocol(NativeProtocolMethod::ResultFromIterator)
                }
                _ => {
                    import.instance.declaration.path.clear();
                }
            }
            assert!(
                verify_program(&forged).is_err(),
                "{name} mutation {mutation}"
            );
            assert!(
                KbcArtifact::from_program(forged, Default::default()).is_err(),
                "{name} encoded mutation {mutation}"
            );
            checked += 1;
        }
        if matches!(
            name,
            "list_push" | "list_insert" | "list_clear" | "list_set" | "map_clear" | "set_clear"
        ) {
            let mut forged = contract.clone();
            match &mut forged.signature.params[0] {
                AbiType::Array(_, access)
                | AbiType::Map { access, .. }
                | AbiType::Set(_, access) => *access = CollectionAccess::ReadOnly,
                _ => panic!("{name}: mutable storage"),
            }
            assert!(forged.resolve().is_none(), "{name}: readonly mutation");
            checked += 1;
        }
        if name == "iter_range" {
            for kind in [RangeKind::To, RangeKind::ToInclusive, RangeKind::Full] {
                let mut forged = contract.clone();
                forged.signature.params[0] =
                    AbiType::Range(Box::new(AbiType::Builtin(BuiltinType::I32)), kind);
                assert!(forged.resolve().is_none(), "non-iterable {kind:?}");
                checked += 1;
            }
        }
        if matches!(name, "iter_array" | "iter_map" | "iter_set") {
            let mut readonly = program.clone();
            let import = &mut readonly.modules[root].engine_imports[index];
            import.signature.params[0] = import.signature.params[0].read_only_view().unwrap();
            // The public native declaration is shared by both outer storage
            // views. Its nested payload and result still match exactly.
            verify_program(&readonly).unwrap();
            KbcArtifact::from_program(readonly, Default::default()).unwrap();
        }
        if name.starts_with("parse_") {
            let mut forged = contract.clone();
            let AbiType::StandardEnum { args, .. } = &mut forged.signature.result else {
                panic!("parse Result")
            };
            args[1] = AbiType::Builtin(BuiltinType::String);
            assert!(forged.resolve().is_none(), "{name}: associated error");
            checked += 1;
        }
    }
    assert_eq!(checked, 187);
}
