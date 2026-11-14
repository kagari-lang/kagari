//! Source-owned frame contracts; obsolete independent window identities are gone.
mod measurements;
use crate::{
    Runtime,
    error::RuntimeErrorKind,
    execution::roots::{ExecutionRoots, SessionId, ValueBank},
    execution_metadata::managed::sources::MetadataSource,
    frame::{
        arguments::FrameArguments,
        values::{FrameAllocationError, FrameMetadata, FrameSlots, FrameStorage},
    },
    module::{
        ExecutableModule, InstalledProgram,
        execution::{
            layout::{FrameLayout, Location},
            operands::OperandSlot,
        },
        managed::source::RevisionSource,
    },
    native::conversion::arguments::ArgumentValues,
    test_support::update_programs as programs,
    value::{
        carriers::{RootedValue, ScopedValue},
        strings::StringObject,
        view::ValueView,
    },
};
use kagari_abi::representation::ValueType;
use kagari_gc::error::GcError;
use kagari_types::{scalar::BuiltinType, ty::Ty};

fn module() -> (Runtime, InstalledProgram, ExecutableModule) {
    let mut runtime = Runtime::default();
    let program = programs::load_empty(&mut runtime, "frame-storage");
    let owner = runtime.select_program(&program).unwrap();
    (runtime, program, owner)
}

/// Exercise production frame admission with the Runtime's actual current revision.
fn allocate(
    runtime: &mut Runtime,
    owner: &ExecutableModule,
    execution: SessionId,
    layout: &FrameLayout,
    parameters: &[usize],
    arguments: FrameArguments<'_, '_>,
) -> Result<FrameStorage, FrameAllocationError> {
    let Runtime {
        memory,
        resources,
        modules,
        ..
    } = runtime;
    memory
        .scope(resources, |cx, roots| {
            ExecutionRoots::with(roots, cx, |slots| {
                let body = RevisionSource::Current.resolve(modules, slots, owner)?;
                Ok(FrameSlots::allocate(
                    slots,
                    execution,
                    layout,
                    parameters,
                    arguments,
                    FrameMetadata {
                        body,
                        inherited_body: None,
                        dependencies: &[],
                    },
                    resources,
                ))
            })
        })
        .map_err(FrameAllocationError::Storage)?
}

fn location(index: usize, representation: ValueType, semantic: BuiltinType) -> Location {
    Location {
        operand: OperandSlot::new(index, representation == ValueType::Str),
        representation,
        semantic: Some(semantic),
        borrowed_receiver: false,
    }
}

fn layout(locations: Vec<Location>, scalar_count: usize, managed_count: usize) -> FrameLayout {
    FrameLayout {
        count: locations.len(),
        locations: locations.into(),
        reused_slots: Box::default(),
        scalar_count,
        managed_count,
    }
}

#[test]
fn scalar_frames_preserve_initialization_bits_and_actual_managed_edges() {
    let (mut runtime, _program, owner) = module();
    runtime.collect_garbage().unwrap();
    let baseline = runtime.gc_stats();
    let execution = runtime.memory.enter_execution(&runtime.resources).unwrap();
    let layout = layout(
        vec![
            location(0, ValueType::U64, BuiltinType::U64),
            location(1, ValueType::F64, BuiltinType::F64),
            location(2, ValueType::I32, BuiltinType::I8),
            location(0, ValueType::Str, BuiltinType::String),
        ],
        3,
        1,
    );
    let storage = allocate(
        &mut runtime,
        &owner,
        execution,
        &layout,
        &[],
        FrameArguments::plain(&[]),
    )
    .unwrap();
    let frame = storage.slots();
    assert!(
        frame
            .inspect(&runtime.memory, layout.locations[0])
            .unwrap()
            .is_unit()
    );
    runtime
        .memory
        .scope(&runtime.resources, |cx, roots| {
            ExecutionRoots::with(roots, cx, |slots| {
                assert!(frame.read(slots, layout.locations[0])?.is_unit());
                frame.write(slots, layout.locations[0], ScopedValue::from(u64::MAX))?;
                for bits in [0x8000_0000_0000_0000, 0x7ff0_0000_0000_0001, u64::MAX] {
                    frame.write(
                        slots,
                        layout.locations[1],
                        ScopedValue::from(f64::from_bits(bits)),
                    )?;
                    assert_eq!(
                        frame.read(slots, layout.locations[1])?.scalar_payload(),
                        Some((ValueType::F64, bits))
                    );
                }
                for value in [ScopedValue::from(128i32), ScopedValue::from(1i64)] {
                    assert!(matches!(
                        frame.write(slots, layout.locations[2], value),
                        Err(GcError::Representation)
                    ));
                }
                assert!(frame.read(slots, layout.locations[2])?.is_unit());
                frame.write(slots, layout.locations[2], ScopedValue::from(-128i32))?;
                let text = StringObject::alloc(slots.scope(), "frame root 🙂".into())?;
                frame.write(slots, layout.locations[3], ScopedValue::managed(text))
            })
        })
        .unwrap();
    assert_eq!(
        frame
            .inspect(&runtime.memory, layout.locations[0])
            .unwrap()
            .as_u64(),
        Some(u64::MAX)
    );
    runtime.collect_garbage().unwrap();
    assert_eq!(runtime.gc_stats().objects, baseline.objects + 1);
    assert_eq!(
        runtime.gc_stats().root_registrations,
        baseline.root_registrations
    );
    runtime
        .memory
        .scope(&runtime.resources, |cx, roots| {
            ExecutionRoots::with(roots, cx, |slots| {
                let text = frame.read(slots, layout.locations[3])?;
                StringObject::read(slots.scope(), text, |text| {
                    assert_eq!(text, "frame root 🙂")
                })?;
                storage.retire(slots)
            })
        })
        .unwrap();
    assert!(matches!(
        frame.inspect(&runtime.memory, layout.locations[0]),
        Err(GcError::Index)
    ));
    runtime
        .memory
        .retire_execution(&runtime.resources, execution)
        .unwrap();
    runtime.collect_garbage().unwrap();
    assert_eq!(runtime.gc_stats().objects, baseline.objects);
    assert_eq!(
        runtime.gc_stats().root_registrations,
        baseline.root_registrations
    );
}

#[test]
fn frame_arguments_survive_bank_growth_reordering_and_repeated_sources() {
    let (mut runtime, _program, owner) = module();
    let execution = runtime.memory.enter_execution(&runtime.resources).unwrap();
    let caller_layout = layout(
        vec![
            location(0, ValueType::I64, BuiltinType::I64),
            location(1, ValueType::I64, BuiltinType::I64),
        ],
        2,
        0,
    );
    let arguments = [ValueView::from(11i64), ValueView::from(23i64)];
    let caller = allocate(
        &mut runtime,
        &owner,
        execution,
        &caller_layout,
        &[0, 1],
        FrameArguments::plain(&arguments),
    )
    .unwrap();
    let frame = caller.slots();
    let transfers = [
        caller_layout.locations[1],
        caller_layout.locations[0],
        caller_layout.locations[1],
    ];
    let callee_layout = layout(
        (0..8192)
            .map(|i| location(i, ValueType::I64, BuiltinType::I64))
            .collect(),
        8192,
        0,
    );
    let callee = allocate(
        &mut runtime,
        &owner,
        execution,
        &callee_layout,
        &[4096, 4097, 4098],
        FrameArguments::frame(frame, &transfers),
    )
    .unwrap();
    for (slot, expected) in [(4096, 23), (4097, 11), (4098, 23)] {
        assert_eq!(
            callee
                .slots()
                .inspect(&runtime.memory, callee_layout.locations[slot])
                .unwrap()
                .as_i64(),
            Some(expected)
        );
    }
    for (slot, expected) in [(0, 11), (1, 23)] {
        assert_eq!(
            frame
                .inspect(&runtime.memory, caller_layout.locations[slot])
                .unwrap()
                .as_i64(),
            Some(expected)
        );
    }
    runtime
        .memory
        .scope(&runtime.resources, |cx, roots| {
            ExecutionRoots::with(roots, cx, |slots| {
                callee.retire(slots)?;
                caller.retire(slots)
            })
        })
        .unwrap();
    // A retired source cannot become valid because the destination grows back
    // over its previous coordinates. Admission must precede all bank growth.
    assert!(matches!(
        allocate(
            &mut runtime,
            &owner,
            execution,
            &callee_layout,
            &[4096, 4097, 4098],
            FrameArguments::frame(frame, &transfers)
        ),
        Err(FrameAllocationError::Storage(GcError::Index))
    ));
    runtime
        .memory
        .scope(&runtime.resources, |cx, roots| {
            ExecutionRoots::with(roots, cx, |slots| {
                assert_eq!(slots.len(execution, ValueBank::Managed)?, 0);
                assert!(slots.storage(execution)?.scalars.is_empty());
                Ok(())
            })
        })
        .unwrap();
    runtime
        .memory
        .retire_execution(&runtime.resources, execution)
        .unwrap();
}

#[test]
fn scalar_parameter_admission_preserves_domains_and_never_publishes_partial_frames() {
    let (mut runtime, _program, owner) = module();
    let execution = runtime.memory.enter_execution(&runtime.resources).unwrap();
    let narrow = layout(
        vec![
            location(0, ValueType::U64, BuiltinType::U64),
            location(1, ValueType::I32, BuiltinType::I8),
        ],
        2,
        0,
    );
    let invalid = [ValueView::from(u64::MAX), ValueView::from(128i32)];
    assert!(matches!(
        allocate(
            &mut runtime,
            &owner,
            execution,
            &narrow,
            &[0, 1],
            FrameArguments::plain(&invalid)
        ),
        Err(FrameAllocationError::Execution(error))
            if error.kind() == RuntimeErrorKind::ModuleValidation
    ));
    runtime
        .memory
        .scope(&runtime.resources, |cx, roots| {
            ExecutionRoots::with(roots, cx, |slots| {
                assert!(slots.storage(execution)?.scalars.is_empty());
                assert!(slots.storage(execution)?.initialized.is_empty());
                Ok(())
            })
        })
        .unwrap();
    let character = layout(vec![location(0, ValueType::Char, BuiltinType::Char)], 1, 0);
    let input = [ValueView::from('🙂')];
    let source = allocate(
        &mut runtime,
        &owner,
        execution,
        &character,
        &[0],
        FrameArguments::plain(&input),
    )
    .unwrap();
    let from = source.slots();
    runtime
        .memory
        .scope(&runtime.resources, |cx, roots| {
            ExecutionRoots::with(roots, cx, |slots| {
                assert!(matches!(
                    from.write(slots, character.locations[0], ScopedValue::from(0x1f642i64)),
                    Err(GcError::Representation)
                ));
                Ok(())
            })
        })
        .unwrap();
    assert!(matches!(
        allocate(
            &mut runtime,
            &owner,
            execution,
            &narrow,
            &[0],
            FrameArguments::frame(from, &character.locations)
        ),
        Err(FrameAllocationError::Storage(GcError::Representation))
    ));
    let target = allocate(
        &mut runtime,
        &owner,
        execution,
        &character,
        &[0],
        FrameArguments::frame(from, &character.locations),
    )
    .unwrap();
    assert_eq!(
        target
            .slots()
            .inspect(&runtime.memory, character.locations[0])
            .unwrap()
            .as_char(),
        Some('🙂')
    );
    runtime
        .memory
        .scope(&runtime.resources, |cx, roots| {
            ExecutionRoots::with(roots, cx, |slots| {
                target.retire(slots)?;
                source.retire(slots)
            })
        })
        .unwrap();
    runtime
        .memory
        .retire_execution(&runtime.resources, execution)
        .unwrap();
}

#[test]
fn method_frame_keeps_its_complete_computation_after_host_sources_retire() {
    let mut runtime = Runtime::default();
    let _program = runtime
        .load_verified_program(
            "method-frame",
            programs::source_code(include_str!(
                "../../../tests/fixtures/metadata/interface-parents.kgr"
            )),
        )
        .unwrap();
    let owner = runtime.select_program(&_program).unwrap();
    runtime.collect_garbage().unwrap();
    let baseline_roots = runtime.gc_stats().root_registrations;
    let value = runtime
        .make_interface(&owner, 0, &RootedValue::from(7_i32))
        .unwrap();
    let description = runtime.interface_description(&value).unwrap();
    let method = runtime
        .resolve_interface_method_slot(&value, description.interface_type(), 0, &[])
        .unwrap();
    let admitted = method.admit(&mut runtime).unwrap();
    drop((value, method));
    let mut activation = runtime.enter_interface_execution_stack(&admitted).unwrap();
    let copied = activation
        .with(|runtime, stack| {
            stack.push_interface_method(runtime, &admitted, ArgumentValues::scalars(&[]), None)?;
            assert_eq!(runtime.resources.uncommon_frames.live(), 1);
            // Both the original interface lease and computation lease are gone. The
            // frame source now owns the actual interface/application/body graph.
            drop(admitted);
            runtime.collect_garbage()?;
            assert_eq!(runtime.gc_stats().root_registrations, baseline_roots);
            let (method, slots, parameter) = {
                let frame = stack.current(runtime)?;
                let callee = frame.loaded().execution().callee(frame.target()).unwrap();
                (
                    frame.invocation(&runtime.resources).unwrap(),
                    frame.slots(),
                    callee
                        .slot_layout
                        .location(callee.parameter_slots[0])
                        .unwrap(),
                )
            };
            runtime
                .memory
                .scope(&runtime.resources, |cx, roots| {
                    ExecutionRoots::with(roots, cx, |source| {
                        let value = slots.read(source, parameter)?;
                        assert_eq!(value.scalar_payload(), Some((ValueType::I32, 7)));
                        Ok(())
                    })
                })
                .unwrap();
            let computation = method.computation().unwrap().clone();
            let view = method.view(
                runtime,
                &[MetadataSource::AdmittedApplication(&computation)],
            )?;
            assert_eq!(view.return_type(), &Ty::Builtin(BuiltinType::I32));
            stack.pop(runtime)?;
            assert_eq!(runtime.resources.uncommon_frames.live(), 0);
            Ok(computation)
        })
        .unwrap();
    drop(activation);
    runtime.collect_garbage().unwrap();
    assert_eq!(
        runtime.application_record(&copied).unwrap_err().kind(),
        RuntimeErrorKind::ModuleValidation
    );
    assert_eq!(runtime.gc_stats().root_registrations, baseline_roots);
    assert!(!runtime.is_quarantined());
}
