use crate::{
    disassembly::instructions::tests::{Fixture, assert_streaming_failures, r, t},
    instruction::{BytecodeInstruction, CallTarget, NativeImportId, RuntimeHelper},
    module::CallableTarget,
    program::ModuleRef,
};
use kagari_abi::representation::ValueType;
use kagari_contract::{
    callable::{
        adapter::SharedAdapterWitness,
        interface::{InterfaceCallContract, TypeNormalization},
        shared::SharedCall,
        witness::{OperationWitness, SharedMethodWitness},
    },
    effects::EffectSet,
    ids::FunctionRef,
    native_import::callables::{NativeCallableApplication, NativeCallableOrigin},
    standard::RuntimePrimitive,
    types::ConcreteFunctionIdentity,
};
use kagari_types::{
    callable::{CallableImplementation, Signature},
    declaration::requirement::NativeCallableRequirement,
    reference::ReferenceAccess,
    scalar::BuiltinType,
    ty::Ty,
};

fn call(target: CallTarget) -> BytecodeInstruction {
    BytecodeInstruction::Call {
        dst: Some(r(0)),
        callee: target,
        args: vec![r(2), r(1)],
    }
}

impl Fixture {
    fn instance(&self) -> ConcreteFunctionIdentity {
        ConcreteFunctionIdentity {
            declaration: self.declaration,
            arguments: vec![t()],
        }
    }

    fn signature(&self) -> Signature {
        Signature {
            params: vec![Ty::Builtin(BuiltinType::U16)],
            result: Ty::Builtin(BuiltinType::Bool),
        }
    }

    fn shared(&self) -> SharedCall {
        SharedCall {
            constructions: vec![],
            receiver_access: None,
            instance: self.instance(),
            implementation: CallableImplementation::Script,
            arguments: vec![Ty::Builtin(BuiltinType::U8)],
            signature: self.signature(),
            operations: vec![],
        }
    }

    fn requirement(&self) -> NativeCallableRequirement {
        NativeCallableRequirement {
            receiver: Ty::Builtin(BuiltinType::U64),
            interface: self.nominal(),
            member: self.member,
            arguments: vec![Ty::Builtin(BuiltinType::F64)],
        }
    }
}

const INSTANCE: &str = "fixture::app::entry arguments=[i32]";
const SIGNATURE: &str = "{params=[u16], result=bool}";
const REQUIREMENT: &str = "{receiver=u64, interface=trait fixture::app::Interface<i32>, member=fixture::app::Interface::method, arguments=[f64]}";
const SHARED: &str = "{receiver_access=none, instance=fixture::app::entry arguments=[i32], implementation=script, arguments=[u8], signature={params=[u16], result=bool}, operations=[]}";

#[test]
fn call_targets_preserve_owner_kind_arguments_results_and_physical_signature() {
    let fixture = Fixture::new();
    let cases = vec![
        (
            CallTarget::Function(FunctionRef::new(0)),
            "function @1:0 fixture::app::entry arguments=[i32]",
        ),
        (
            CallTarget::ModuleFunction {
                module: ModuleRef::new(0),
                function: FunctionRef::new(0),
            },
            "module_function @0:0 fixture::dependency::\"foreign\"",
        ),
        (
            CallTarget::Native(NativeImportId::new(8)),
            "native @1:native#8",
        ),
        (CallTarget::Slot(r(3)), "register s3"),
        (
            CallTarget::ClosureRegister {
                register: r(3),
                params: vec![ValueType::I32, ValueType::HostHandle],
                return_type: ValueType::HeapObject,
            },
            "closure_register s3, physical_params=[i32, host_handle], physical_result=heap_object",
        ),
    ];
    for (target, expected) in cases {
        assert_eq!(
            fixture.render(&call(target)),
            format!("call dst=s0, target={expected}, args=[s2, s1]")
        );
    }
    assert_eq!(
        fixture.render(&BytecodeInstruction::Call {
            dst: None,
            callee: CallTarget::Function(FunctionRef::new(0)),
            args: vec![]
        }),
        "call dst=none, target=function @1:0 fixture::app::entry arguments=[i32], args=[]"
    );
    for (target, expected) in [
        (
            CallableTarget::Script(FunctionRef::new(0)),
            "shared.script @0:0 fixture::dependency::\"foreign\"",
        ),
        (
            CallableTarget::Native(NativeImportId::new(8)),
            "shared.native @0:native#8",
        ),
    ] {
        let target = CallTarget::Shared {
            module: ModuleRef::new(0),
            target,
            contract: Box::new(fixture.shared()),
        };
        assert_eq!(
            fixture.render(&call(target)),
            format!("call dst=s0, target={expected} contract={SHARED}, args=[s2, s1]")
        );
    }
}

#[test]
fn runtime_primitives_helpers_and_artifact_names_are_not_debug_dumps() {
    let mut fixture = Fixture::new();
    for (primitive, name) in [
        (RuntimePrimitive::ValuePartialCmp, "value_partial_cmp"),
        (RuntimePrimitive::ValueCmp, "value_cmp"),
        (RuntimePrimitive::ValueEq, "value_eq"),
        (RuntimePrimitive::ValueHash, "value_hash"),
        (RuntimePrimitive::ValueDebug, "value_debug"),
        (RuntimePrimitive::ValueDisplay, "value_display"),
        (RuntimePrimitive::StringPartsJoin, "string_parts_join"),
        (RuntimePrimitive::Assert, "assert"),
    ] {
        assert_eq!(
            fixture.render(&call(CallTarget::RuntimePrimitive(primitive))),
            format!("call dst=s0, target=runtime_primitive.{name}, args=[s2, s1]")
        );
    }
    for (helper, expected) in [
        (RuntimeHelper::ReflectTypeOf, "reflect_type_of"),
        (
            RuntimeHelper::ReflectGetField("x\n\"".into()),
            "reflect_get_field \"x\\n\\\"\"",
        ),
        (
            RuntimeHelper::ReflectSetField("x\t\\".into()),
            "reflect_set_field \"x\\t\\\\\"",
        ),
        (RuntimeHelper::ReflectSetIndex, "reflect_set_index"),
        (RuntimeHelper::DynamicCall, "dynamic_call"),
    ] {
        assert_eq!(
            fixture.render(&call(CallTarget::RuntimeHelper(helper))),
            format!("call dst=s0, target=runtime_helper.{expected}, args=[s2, s1]")
        );
    }
    fixture.program.modules[0].functions[0].name = "bad\nname\u{202e}\u{1b}".into();
    let rendered = fixture.render(&call(CallTarget::ModuleFunction {
        module: ModuleRef::new(0),
        function: FunctionRef::new(0),
    }));
    assert!(!rendered.contains(['\n', '\u{202e}', '\u{1b}']));
    assert!(rendered.contains("bad\\nname\\u{202e}\\u{1b}"));
}

#[test]
fn interface_dispatch_retains_normalizations_receiver_and_forwarded_operations() {
    let fixture = Fixture::new();
    for (receiver, expected) in [(None, "none"), (Some(Ty::Builtin(BuiltinType::U8)), "u8")] {
        let instruction = call(CallTarget::InterfaceMethod {
            module: ModuleRef::new(1),
            contract: Box::new(InterfaceCallContract {
                constructions: vec![],
                normalizations: vec![
                    TypeNormalization {
                        source: t(),
                        result: Ty::Builtin(BuiltinType::U32),
                    },
                    TypeNormalization {
                        source: Ty::Builtin(BuiltinType::I64),
                        result: t(),
                    },
                ],
                receiver,
                operations: vec![OperationWitness::Forward(Box::new(fixture.requirement()))],
                interface: fixture.nominal(),
                method_slot: 0,
                arguments: vec![Ty::Builtin(BuiltinType::F32)],
            }),
        });
        assert_eq!(
            fixture.render(&instruction),
            format!(
                "call dst=s0, target=interface_method module=@1, contract={{interface=trait fixture::app::Interface<i32>, declaration=@1:trait_contract#0, method_slot=0, member=fixture::app::Interface::method, arguments=[f32], receiver={expected}, normalizations=[{{source=i32, result=u32}}, {{source=i64, result=i32}}], operations=[forward{REQUIREMENT}]}}, args=[s2, s1]"
            )
        );
        assert_streaming_failures(&fixture, &instruction);
    }
}

#[test]
fn shared_witness_tree_prints_every_nested_contract_fact_and_propagates_io_errors() {
    let fixture = Fixture::new();
    let selected = OperationWitness::Selected(Box::new(NativeCallableApplication {
        origin: NativeCallableOrigin::Implementation,
        requirement: fixture.requirement(),
        instance: fixture.instance(),
        implementation: CallableImplementation::Native(fixture.declaration),
        signature: fixture.signature(),
        effects: EffectSet {
            reads_module: true,
            writes_path: true,
            may_trap: true,
            may_suspend: true,
            ..Default::default()
        },
    }));
    let method = OperationWitness::SharedMethod(Box::new(SharedMethodWitness {
        constructions: vec![],
        requirement: fixture.requirement(),
        implementation: ConcreteFunctionIdentity {
            declaration: fixture.table,
            arguments: vec![t()],
        },
        operations: vec![
            selected,
            OperationWitness::Forward(Box::new(fixture.requirement())),
        ],
    }));
    let mut nested = fixture.shared();
    nested.receiver_access = Some(ReferenceAccess::Shared);
    nested.operations.push(method);
    let adapter = OperationWitness::SharedAdapter(Box::new(SharedAdapterWitness {
        requirement: fixture.requirement(),
        call: nested,
    }));
    let mut shared = fixture.shared();
    shared.receiver_access = Some(ReferenceAccess::Mutable);
    shared.operations.push(adapter);
    let instruction = call(CallTarget::Shared {
        module: ModuleRef::new(1),
        target: CallableTarget::Script(FunctionRef::new(0)),
        contract: Box::new(shared),
    });
    let selected = format!(
        "selected{{target=@1:native#0, origin=implementation, requirement={REQUIREMENT}, instance={INSTANCE}, implementation=native fixture::app::entry, signature={SIGNATURE}, recorded_effects=[reads_module, writes_path, may_trap, may_suspend]}}"
    );
    let method = format!(
        "shared_method{{target=@1:interface#0, requirement={REQUIREMENT}, implementation=fixture::app::table arguments=[i32], operations=[{selected}, forward{REQUIREMENT}]}}"
    );
    let nested = format!(
        "{{receiver_access=shared, instance={INSTANCE}, implementation=script, arguments=[u8], signature={SIGNATURE}, operations=[{method}]}}"
    );
    let adapter =
        format!("shared_adapter{{target=@1:0, requirement={REQUIREMENT}, call={nested}}}");
    let shared = format!(
        "{{receiver_access=mutable, instance={INSTANCE}, implementation=script, arguments=[u8], signature={SIGNATURE}, operations=[{adapter}]}}"
    );
    assert_eq!(
        fixture.render(&instruction),
        format!(
            "call dst=s0, target=shared.script @1:0 {INSTANCE} contract={shared}, args=[s2, s1]"
        )
    );
    assert_streaming_failures(&fixture, &instruction);
}

#[test]
fn selected_origins_and_executable_targets_are_explicit() {
    let fixture = Fixture::new();
    for (implementation, expected, origin, origin_text, target) in [
        (
            CallableImplementation::Script,
            "script",
            NativeCallableOrigin::ProtocolAdapter,
            "protocol_adapter",
            "@1:0",
        ),
        (
            CallableImplementation::Native(fixture.declaration),
            "native fixture::app::entry",
            NativeCallableOrigin::Implementation,
            "implementation",
            "@1:native#0",
        ),
    ] {
        let mut shared = fixture.shared();
        shared.operations.push(OperationWitness::Selected(Box::new(
            NativeCallableApplication {
                origin,
                requirement: fixture.requirement(),
                instance: fixture.instance(),
                implementation,
                signature: fixture.signature(),
                effects: EffectSet::default(),
            },
        )));
        let instruction = call(CallTarget::Shared {
            module: ModuleRef::new(1),
            target: CallableTarget::Script(FunctionRef::new(0)),
            contract: Box::new(shared),
        });
        let selected = format!(
            "selected{{target={target}, origin={origin_text}, requirement={REQUIREMENT}, instance={INSTANCE}, implementation={expected}, signature={SIGNATURE}, recorded_effects=[]}}"
        );
        assert_eq!(
            fixture.render(&instruction),
            format!(
                "call dst=s0, target=shared.script @1:0 {INSTANCE} contract={{receiver_access=none, instance={INSTANCE}, implementation=script, arguments=[u8], signature={SIGNATURE}, operations=[{selected}]}}, args=[s2, s1]"
            )
        );
    }
}
