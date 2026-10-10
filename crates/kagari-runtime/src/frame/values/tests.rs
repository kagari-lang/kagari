use super::*;
use crate::{
    Runtime,
    frame::types::EnvironmentRecord,
    module::execution::layout::Location,
    native::{
        application::NativeApplication,
        binding::{Codec, LinkedNativeFunction, NativeBinding},
        context::{CallableOwner, LinkedCallable, LinkedOperation},
    },
};
use kagari_bytecode::{
    instruction::{NativeImportId, Register},
    module::{BytecodeModule, CallableTarget},
    program::{BytecodeProgram, ModuleRef},
};
use kagari_common::identity::{
    DefinitionKind, DefinitionPath, DefinitionPathSegment, ModuleIdentity,
};
use kagari_contract::ids::FunctionRef;
use kagari_types::{callable::Signature, scalar::BuiltinType, ty::Ty};
use std::mem::size_of;

fn module() -> (Runtime, LoadedModule) {
    let mut runtime = Runtime::default();
    let module = runtime
        .load_program(
            "windows",
            BytecodeProgram {
                root: ModuleRef::new(0),
                modules: vec![BytecodeModule::default()],
            },
        )
        .unwrap();
    (runtime, module)
}

#[test]
fn frame_windows_reject_foreign_and_reused_identities() {
    let (_runtime, module) = module();
    let mut values = ExecutionValues::default();
    let mut foreign = ExecutionValues::default();
    let arguments = FrameArguments::plain(&[Value::I32(42)]);
    let first = values
        .allocate(
            2,
            1,
            &arguments,
            FrameMetadata {
                program: module.clone(),
                environment: None,
                invocation: None,
            },
            None,
        )
        .unwrap();
    let other = foreign
        .allocate(
            2,
            1,
            &arguments,
            FrameMetadata {
                program: module.clone(),
                environment: None,
                invocation: None,
            },
            None,
        )
        .unwrap();
    assert!(values.get(other).is_none());
    assert!(values.release(other).is_none());
    values.release(first).unwrap();
    let second = values
        .allocate(
            2,
            1,
            &arguments,
            FrameMetadata {
                program: module,
                environment: None,
                invocation: None,
            },
            None,
        )
        .unwrap();
    assert_eq!(first.index, second.index);
    assert!(values.get(first).is_none());
    assert!(values.ranges(first).is_none());
    assert!(values.release(first).is_none());
    assert_eq!(values.get(second).unwrap(), &[Value::Unit, Value::I32(42)]);
}

#[test]
fn retiring_an_outer_window_does_not_unroot_a_suspended_inner_window() {
    let (_runtime, module) = module();
    let mut values = ExecutionValues::default();
    let first = values
        .allocate(
            1,
            0,
            &FrameArguments::plain(&[Value::I32(1)]),
            FrameMetadata {
                program: module.clone(),
                environment: None,
                invocation: None,
            },
            None,
        )
        .unwrap();
    let second = values
        .allocate(
            1,
            0,
            &FrameArguments::plain(&[Value::I32(2)]),
            FrameMetadata {
                program: module.clone(),
                environment: None,
                invocation: None,
            },
            None,
        )
        .unwrap();
    values.release(first).unwrap();
    assert_eq!(values.get(second).unwrap(), &[Value::I32(2)]);
    let mut roots = Vec::new();
    values.append_values(&mut roots);
    assert_eq!(roots, [Value::I32(2)]);
    let mut metadata = Vec::new();
    values.append_metadata(&mut metadata);
    assert_eq!(metadata.len(), 1);
    values.release(second).unwrap();
    roots.clear();
    metadata.clear();
    values.append_values(&mut roots);
    values.append_metadata(&mut metadata);
    assert!(roots.is_empty());
    assert!(metadata.is_empty());

    // Keep one root parked while successively retiring older roots. Both banks
    // must stay proportional to live frames, even when slot order is reused.
    let args = [
        Value::U64(17),
        Value::F64(-0.0),
        Value::I32(-7),
        _runtime.gc().alloc_string("kept".into()).unwrap(),
    ];
    let mut previous = values
        .allocate(
            4,
            0,
            &FrameArguments::plain(&args),
            FrameMetadata {
                program: module.clone(),
                environment: None,
                invocation: None,
            },
            Some(scalar_layout()),
        )
        .unwrap();
    for _ in 0..16 {
        let next = values
            .allocate(
                4,
                0,
                &FrameArguments::plain(&args),
                FrameMetadata {
                    program: module.clone(),
                    environment: None,
                    invocation: None,
                },
                Some(scalar_layout()),
            )
            .unwrap();
        values.release(previous).unwrap();
        assert!(values.get(previous).is_none());
        assert_eq!(values.get(next).unwrap(), args);
        assert_eq!(values.payloads.len(), 3);
        assert_eq!(values.values.len(), 1);
        assert!(values.windows.len() <= 2);
        previous = next;
    }
    values.release(previous).unwrap();
    assert!(values.values.is_empty());
    assert!(values.payloads.is_empty());
}

#[test]
fn register_arguments_survive_growth_reordering_and_repeated_sources() {
    let (_runtime, module) = module();
    let mut values = ExecutionValues::default();
    let caller = values
        .allocate(
            2,
            0,
            &FrameArguments::plain(&[Value::I64(11), Value::I64(23)]),
            FrameMetadata {
                program: module.clone(),
                environment: None,
                invocation: None,
            },
            None,
        )
        .unwrap();
    let registers = [Register::new(1), Register::new(0), Register::new(1)];
    let transfers = registers
        .iter()
        .map(|register| {
            values
                .window(caller)
                .unwrap()
                .location(register.index())
                .unwrap()
        })
        .collect::<Vec<_>>();
    let arguments = FrameArguments::frame(caller, &transfers);
    let callee = values
        .allocate(
            8192,
            4096,
            &arguments,
            FrameMetadata {
                program: module.clone(),
                environment: None,
                invocation: None,
            },
            None,
        )
        .unwrap();
    assert_eq!(
        &values.get(callee).unwrap()[4096..4099],
        &[Value::I64(23), Value::I64(11), Value::I64(23)]
    );
    assert_eq!(
        values.get(caller).unwrap(),
        &[Value::I64(11), Value::I64(23)]
    );
    values.release(callee).unwrap();
    values.release(caller).unwrap();
    assert!(
        values
            .allocate(
                3,
                0,
                &arguments,
                FrameMetadata {
                    program: module,
                    environment: None,
                    invocation: None
                },
                None
            )
            .is_err()
    );
}

fn scalar_layout() -> Arc<FrameLayout> {
    Arc::new(FrameLayout {
        locations: vec![
            Location {
                operand: OperandSlot::new(0, false),
                representation: ValueType::U64,
                semantic: Some(BuiltinType::U64),
            },
            Location {
                operand: OperandSlot::new(1, false),
                representation: ValueType::F64,
                semantic: Some(BuiltinType::F64),
            },
            Location {
                operand: OperandSlot::new(2, false),
                representation: ValueType::I32,
                semantic: Some(BuiltinType::I8),
            },
            Location {
                operand: OperandSlot::new(0, true),
                representation: ValueType::Str,
                semantic: Some(BuiltinType::String),
            },
        ]
        .into(),
        register_count: 4,
        count: 4,
        scalar_count: 3,
        managed_count: 1,
    })
}

#[test]
fn scalar_windows_preserve_bits_initialization_and_managed_roots() {
    println!(
        "STORAGE_LAYOUT,payload_bytes={},initialization_bytes={},location_bytes={},window_bytes={}",
        size_of::<u64>(),
        size_of::<bool>(),
        size_of::<Location>(),
        size_of::<Window>()
    );
    let (_runtime, module) = module();
    let mut values = ExecutionValues::default();
    let slots = values
        .allocate(
            4,
            0,
            &FrameArguments::plain(&[]),
            FrameMetadata {
                program: module,
                environment: None,
                invocation: None,
            },
            Some(scalar_layout()),
        )
        .unwrap();
    let ranges = values.ranges(slots).unwrap();
    assert!(
        values
            .payload(&ranges, OperandSlot::new(0, false).scalar().unwrap())
            .is_none()
    );
    assert_eq!(values.with_value(slots, 0, Value::clone), Some(Value::Unit));
    values.set(slots, 0, Value::U64(u64::MAX)).unwrap();
    for bits in [0x8000_0000_0000_0000, 0x7ff0_0000_0000_0001, u64::MAX] {
        values
            .set(slots, 1, Value::F64(f64::from_bits(bits)))
            .unwrap();
        let Some(Value::F64(v)) = values.with_value(slots, 1, Value::clone) else {
            panic!()
        };
        assert_eq!(v.to_bits(), bits);
    }
    assert!(values.set(slots, 2, Value::I32(128)).is_none());
    assert!(values.set(slots, 2, Value::I64(1)).is_none());
    values.set(slots, 2, Value::I32(-128)).unwrap();
    let string = _runtime.gc().alloc_string("root".into()).unwrap();
    values.set(slots, 3, string).unwrap();
    let mut roots = Vec::new();
    values.append_values(&mut roots);
    assert_eq!(roots, [string]);
    values.release(slots).unwrap();
    assert!(values.payloads.is_empty());
    assert!(values.initialized.is_empty());
    assert!(values.values.is_empty());
    assert!(values.with_value(slots, 0, Value::clone).is_none());
}

#[test]
fn invalid_scalar_admission_never_publishes_a_partial_frame() {
    let (_runtime, module) = module();
    let mut values = ExecutionValues::default();
    assert!(
        values
            .allocate(
                4,
                2,
                &FrameArguments::plain(&[Value::I32(128)]),
                FrameMetadata {
                    program: module.clone(),
                    environment: None,
                    invocation: None
                },
                Some(scalar_layout())
            )
            .is_err()
    );
    assert_eq!(values.active_windows(), 0);
    assert!(values.payloads.is_empty());
    let slots = values
        .allocate(
            4,
            0,
            &FrameArguments::plain(&[Value::U64(u64::MAX)]),
            FrameMetadata {
                program: module.clone(),
                environment: None,
                invocation: None,
            },
            Some(scalar_layout()),
        )
        .unwrap();
    assert_eq!(
        values.with_value(slots, 0, Value::clone),
        Some(Value::U64(u64::MAX))
    );
    let mut narrow = scalar_layout();
    Arc::get_mut(&mut narrow).unwrap().locations[0].semantic = Some(BuiltinType::U8);
    let transfers = [scalar_layout().location(0).unwrap()];
    assert!(
        values
            .allocate(
                4,
                0,
                &FrameArguments::frame(slots, &transfers),
                FrameMetadata {
                    program: module.clone(),
                    environment: None,
                    invocation: None
                },
                Some(narrow.clone())
            )
            .is_err()
    );
    assert_eq!(values.active_windows(), 1);
    assert_eq!(values.payloads.len(), 3);
    assert_eq!(
        values.with_value(slots, 0, Value::clone),
        Some(Value::U64(u64::MAX))
    );
    values.set(slots, 0, Value::U64(255)).unwrap();
    let accepted = values
        .allocate(
            4,
            0,
            &FrameArguments::frame(slots, &transfers),
            FrameMetadata {
                program: module,
                environment: None,
                invocation: None,
            },
            Some(narrow),
        )
        .unwrap();
    assert_eq!(
        values.with_value(accepted, 0, Value::clone),
        Some(Value::U64(255))
    );
    values.release(accepted).unwrap();
    assert_eq!(
        values.with_value(slots, 0, Value::clone),
        Some(Value::U64(255))
    );
}

#[test]
fn native_application_eviction_keeps_active_window_edges_without_host_roots() {
    let (runtime, loaded) = module();
    let declaration = runtime
        .definition_context()
        .intern(&DefinitionPath {
            module: ModuleIdentity::single_file("native_window"),
            path: vec![DefinitionPathSegment {
                kind: DefinitionKind::Function,
                name: "unused".into(),
                occurrence: 0,
            }],
        })
        .unwrap();
    let empty = || {
        runtime
            .alloc_environment(
                EnvironmentRecord::new(runtime.definition_context(), vec![], vec![]).unwrap(),
            )
            .unwrap()
    };
    let source = empty();
    let selected = empty();
    let selected_id = selected.id;
    // Isolate the native descriptor's selected-call edge from its source scope.
    // The selected environment has no other executable parent or owning handle.
    let application = Arc::new(NativeApplication {
        owner: loaded.clone(),
        environment: source.clone(),
        function: LinkedNativeFunction {
            declaration,
            binding: NativeBinding::new(vec![], Codec::Value, |_| Ok(Value::Unit)),
            signature: Signature {
                params: vec![],
                result: Ty::Builtin(BuiltinType::Unit),
            },
            prepared_signature: Default::default(),
            result_adapter: None,
            selected: vec![LinkedOperation::Ready(LinkedCallable {
                owner: CallableOwner::Resolved(loaded.clone()),
                target: CallableTarget::Script(FunctionRef::new(0)),
                params: Box::new([]),
                result: Ty::Builtin(BuiltinType::Unit),
                primitive: None,
                environment: Some(selected),
                scoped_signature: None,
            })]
            .into_boxed_slice(),
        },
    });
    let import = NativeImportId::new(0);
    runtime
        .publish_native_application(&loaded, import, application.clone())
        .unwrap();
    let slots = runtime
        .resources()
        .frame_values
        .borrow_mut()
        .allocate(
            0,
            0,
            &FrameArguments::plain(&[]),
            FrameMetadata {
                program: loaded.clone(),
                environment: Some(source),
                invocation: None,
            },
            None,
        )
        .unwrap();
    slots
        .publish_native_application(&runtime.gc, application.clone())
        .unwrap();
    assert!(
        slots
            .publish_native_application(&runtime.gc, application.clone())
            .is_err()
    );
    let probe = Arc::downgrade(&application);
    for _ in 0..160 {
        let mut function = application.function.clone();
        function.selected = Box::new([]);
        let next = Arc::new(NativeApplication {
            owner: loaded.clone(),
            environment: empty(),
            function,
        });
        runtime
            .publish_native_application(&loaded, import, next)
            .unwrap();
    }
    drop(application);
    runtime.collect_garbage().unwrap();
    // Root accounting includes this one execution window; no host root was added.
    assert_eq!(runtime.gc.active_roots(), 1);
    assert!(probe.upgrade().is_some());
    assert!(runtime.gc.environment(selected_id).is_some());
    assert_eq!(runtime.gc.stats().environments, 130);
    runtime
        .resources()
        .frame_values
        .borrow_mut()
        .release(slots)
        .unwrap();
    runtime.collect_garbage().unwrap();
    assert!(probe.upgrade().is_none());
    assert!(runtime.gc.environment(selected_id).is_none());
    assert_eq!(runtime.gc.stats().environments, 128);
    let candidate = runtime
        .stage_reload_verified_program(&loaded, "windows", loaded.verified_program().clone())
        .unwrap();
    runtime.publish_staged_reload(candidate).unwrap();
    runtime.collect_garbage().unwrap();
    assert_eq!(runtime.gc.stats().environments, 0);
    assert_eq!(runtime.gc.active_roots(), 0);
}
