//! Allocation accounting isolates prepared native invocation from frame setup and compilation.
use super::native_allocations_counter::{Counts, measured, verify_counter};
use crate::bytecode_fixture::load_bytecode;
use kagari_bytecode::instruction::{BytecodeInstruction, CallTarget, Slot};
use kagari_compiler::{bytecode::lower_program_to_bytecode, source::program::lower_program_to_mir};
use kagari_contract::ids::FunctionRef;
use kagari_hir::{analysis::AnalysisDatabase, compile_input::program::CheckedCompilation};
use kagari_runtime::{
    Runtime,
    frame::{
        cursor::kernel::RegionExit,
        driver::{ExecutionAction, ReturnAction},
    },
    module::ExecutableModule,
    native::{
        binding::NativeResult,
        builder::ModuleBuilder,
        context::{NativeCall, ScriptCall},
        conversion::{arguments::ArgumentValues, context::ConversionContext},
        declarations::FunctionDecl,
        module::NativeModule,
        types::Type,
        views::ValueHandle,
    },
    value::{
        place::{PublishedResult, ResultPlace},
        view::ValueView,
    },
};
use kagari_source::source_database::{SourceDatabase, SourceLayer};
use kagari_stdlib::{catalog as foundation_catalog, declarations::StandardDeclarations};
use kagari_types::{scalar::BuiltinType, ty::Ty};
use std::{hint::black_box, sync::Arc, time::Duration};

fn module() -> NativeModule {
    let language = StandardDeclarations::default();
    let mut module = ModuleBuilder::new(
        "measure::native",
        &language.catalog().expect("explicit standard providers"),
    )
    .unwrap();
    let add = module
        .define_function(
            FunctionDecl::new("add")
                .parameter("a", Type::i32())
                .parameter("b", Type::i32())
                .returns(Type::i32()),
        )
        .unwrap();
    module
        .bind(
            add,
            |cx: &mut NativeCall<'_>, a: i32, b: i32| -> NativeResult<i32> {
                assert_eq!(
                    cx.argument_type_argument(0)?.ty(),
                    &Ty::Builtin(BuiltinType::I32)
                );
                assert_eq!(
                    cx.result_type_argument()?.ty(),
                    &Ty::Builtin(BuiltinType::I32)
                );
                Ok(a + b)
            },
        )
        .unwrap();
    let sum = module
        .define_function(
            FunctionDecl::new("sum")
                .parameter("values", Type::array(Type::i32()).unwrap())
                .returns(Type::i32()),
        )
        .unwrap();
    module
        .bind(
            sum,
            |cx: &mut NativeCall<'_>, values: ValueHandle<'_>| -> NativeResult<i32> {
                assert_eq!(
                    cx.argument_type_parameter(0, 0)?.ty(),
                    &Ty::Builtin(BuiltinType::I32)
                );
                let values = values.value(cx)?;
                let len = cx.array_len(&values)?;
                let mut sum = 0;
                for index in 0..len {
                    sum += cx
                        .array_element(&values, index)?
                        .expect("in-bounds array element")
                        .as_i32()
                        .expect("checked i32 element");
                }
                Ok(sum)
            },
        )
        .unwrap();
    module.finish().unwrap()
}

fn load() -> (Runtime, ExecutableModule) {
    let module = module();
    let mut sources = SourceDatabase::default();
    let root = sources
        .set(
            "allocation.kgr",
            include_str!("../fixtures/native/allocation-native-boundaries.kgr").into(),
            SourceLayer::Base,
        )
        .unwrap();
    let mut analysis = AnalysisDatabase::default();
    analysis.set_native_modules(
        foundation_catalog::shared()
            .unwrap()
            .into_iter()
            .chain([Arc::new(module.to_declaration())])
            .collect(),
    );
    let snapshot = analysis
        .snapshot(sources.snapshot(), &Default::default())
        .unwrap();
    let checked = snapshot.check_program(root, &Default::default()).unwrap();
    let mir = lower_program_to_mir(
        &CheckedCompilation::from_program(&checked, &Default::default()).unwrap(),
        &Default::default(),
    )
    .unwrap();
    let program = lower_program_to_bytecode(&mir, &Default::default())
        .map(|program| program.into_unverified())
        .unwrap();
    let mut runtime = Runtime::default();
    NativeModule::install_all(&kagari_stdlib::modules().unwrap(), &mut runtime).unwrap();
    module.install(&mut runtime).unwrap();
    let loaded = load_bytecode(&mut runtime, "allocation", program).unwrap();
    let _loaded_program = loaded;
    let loaded = runtime.select_program(&_loaded_program).unwrap();
    (runtime, loaded)
}

struct CallSite {
    function: FunctionRef,
    pc: usize,
    arguments: Vec<Slot>,
    destination: Option<Slot>,
}

impl CallSite {
    fn find(loaded: &ExecutableModule, name: &str) -> Self {
        let function = loaded
            .bytecode
            .functions
            .iter()
            .find(|function| function.name == name)
            .unwrap();
        let (pc, arguments, destination) = function
            .instructions
            .iter()
            .enumerate()
            .find_map(|(pc, instruction)| match instruction {
                BytecodeInstruction::Call {
                    callee: CallTarget::Native(_),
                    args,
                    dst,
                } => Some((pc, args.clone(), *dst)),
                _ => None,
            })
            .expect("ordinary direct native call");
        Self {
            function: function.id,
            pc,
            arguments,
            destination,
        }
    }
}

fn unexpected_callback<'result>(
    _: &mut Runtime,
    _: &ExecutableModule,
    _: ScriptCall<'_, '_>,
    _: ArgumentValues<'_>,
    _: ResultPlace<'result>,
) -> NativeResult<PublishedResult<'result>> {
    panic!("the measured scalar and bulk entries have no script callbacks")
}

fn sample(
    runtime: &mut Runtime,
    loaded: &ExecutableModule,
    site: &CallSite,
    arguments: &[ValueView<'_>],
    calls: usize,
    expected: i32,
) -> (Counts, Duration) {
    let mut stack_activation = runtime.enter_execution_stack(loaded).unwrap();
    stack_activation
        .with(|runtime, stack| {
            stack
                .push(runtime, loaded.slot(), site.function, arguments, None)
                .unwrap();
            for (register, value) in site.arguments.iter().zip(arguments) {
                stack.write_slot(runtime, *register, value).unwrap();
            }
            let invoke = |runtime: &mut Runtime| {
                stack
                    .current_mut(runtime)
                    .unwrap()
                    .jump_to(runtime, site.pc)
                    .unwrap();
                assert!(matches!(
                    stack
                        .execute_region(runtime, &mut Some(1), unexpected_callback)
                        .unwrap(),
                    RegionExit::Call(ExecutionAction::NativeEntry)
                ));
                assert_eq!(
                    stack
                        .start_native_entry(runtime, unexpected_callback)
                        .unwrap(),
                    ExecutionAction::NativeReturn
                );
                assert!(matches!(
                    stack.finish_native_return(runtime).unwrap(),
                    ReturnAction::Continue(ExecutionAction::Script)
                ));
            };
            // Warm the boundary and initialize every thread-local/cache outside accounting.
            for _ in 0..128 {
                invoke(runtime);
            }
            let roots = runtime.gc_stats().root_registrations;
            let occupancy = runtime.gc_stats().provider_units;
            let sample = measured(|| {
                for _ in 0..calls {
                    black_box(&invoke)(runtime);
                }
            });
            assert_eq!(
                sample.0,
                Counts::default(),
                "prepared boundary must not allocate or release Rust heap storage"
            );
            assert_eq!(runtime.gc_stats().root_registrations, roots);
            assert_eq!(runtime.gc_stats().provider_units, occupancy);
            assert_eq!(
                stack
                    .current(runtime)
                    .unwrap()
                    .read_slot(runtime, site.destination.unwrap())
                    .unwrap()
                    .as_i32(),
                Some(expected)
            );
            Ok(sample)
        })
        .unwrap()
}

#[test]
fn warmed_scalar_and_array_read_boundaries_add_no_allocations() {
    verify_counter();
    let (mut runtime, loaded) = load();
    let scalar = CallSite::find(&loaded, "scalar");
    let (counts, elapsed) = sample(
        &mut runtime,
        &loaded,
        &scalar,
        &[ValueView::from(20), ValueView::from(22)],
        100_000,
        42,
    );
    println!(
        "scalar,calls=100000,{counts:?},elapsed_ns={}",
        elapsed.as_nanos()
    );
    runtime.collect_garbage().unwrap();
    let baseline = runtime.gc_stats();
    let bulk = CallSite::find(&loaded, "bulk");
    for length in [0, 16, 16_384] {
        let mut cx = ConversionContext::new(&mut runtime, &loaded).unwrap();
        let item = cx.type_for::<i32>().unwrap();
        let values = vec![ValueView::from(1i32); length];
        let array = cx.allocate_array(&item, &values).unwrap();
        let (counts, elapsed) = sample(
            cx.runtime_mut(),
            &loaded,
            &bulk,
            &[array],
            1_000,
            length as i32,
        );
        println!(
            "bulk_i32,elements={length},calls=1000,{counts:?},elapsed_ns={}",
            elapsed.as_nanos()
        );
        drop(cx);
        runtime.collect_garbage().unwrap();
        assert_eq!(
            runtime.gc_stats().root_registrations,
            baseline.root_registrations
        );
        assert_eq!(runtime.gc_stats().objects, baseline.objects);
    }
}

#[test]
fn warmed_script_argument_windows_match_borrowed_slice_allocation_cost() {
    verify_counter();
    let (mut runtime, loaded) = load();
    runtime.collect_garbage().unwrap();
    let baseline = runtime.gc_stats().root_registrations;
    let caller = loaded
        .bytecode
        .functions
        .iter()
        .find(|function| function.name == "script_caller")
        .unwrap();
    let (pc, module, callee, arguments, destination) = caller
        .instructions
        .iter()
        .enumerate()
        .find_map(|(pc, instruction)| match instruction {
            BytecodeInstruction::Call {
                callee: CallTarget::ModuleFunction { module, function },
                args,
                dst,
            } => Some((pc, *module, *function, args.as_slice(), *dst)),
            BytecodeInstruction::Call {
                callee: CallTarget::Function(function),
                args,
                dst,
            } => Some((pc, loaded.slot(), *function, args.as_slice(), *dst)),
            _ => None,
        })
        .unwrap();
    let mut stack_activation = runtime.enter_execution_stack(&loaded).unwrap();
    stack_activation
        .with(|runtime, stack| {
            stack
                .push(
                    runtime,
                    loaded.slot(),
                    caller.id,
                    &[ValueView::from(20), ValueView::from(11)],
                    None,
                )
                .unwrap();
            assert!(matches!(
                stack
                    .execute_region(runtime, &mut None, unexpected_callback)
                    .unwrap(),
                RegionExit::Call(ExecutionAction::Script)
            ));
            let callee_owner = loaded.member(module).unwrap();
            let parameters = &callee_owner.bytecode.functions[callee.index()]
                .metadata
                .parameter_slots;
            for (&parameter, expected) in parameters.iter().zip([11, 20, 11]) {
                assert_eq!(
                    stack
                        .current(runtime)
                        .unwrap()
                        .read_slot(runtime, parameter)
                        .unwrap()
                        .as_i32(),
                    Some(expected)
                );
            }
            let RegionExit::Return(value) = stack
                .execute_region(runtime, &mut None, unexpected_callback)
                .unwrap()
            else {
                panic!("concrete scalar callee must remain in its prepared region");
            };
            stack.finish_return(runtime, value).unwrap();
            let external = [
                ValueView::from(11),
                ValueView::from(20),
                ValueView::from(11),
            ];
            stack
                .push(runtime, module, callee, &external, None)
                .unwrap();
            stack.pop(runtime).unwrap();
            let enter = |runtime: &mut Runtime| {
                stack
                    .current_mut(runtime)
                    .unwrap()
                    .jump_to(runtime, pc)
                    .unwrap();
                assert!(matches!(
                    stack
                        .execute_region(black_box(runtime), &mut None, unexpected_callback)
                        .unwrap(),
                    RegionExit::Call(ExecutionAction::Script)
                ));
            };
            let (window, _) = measured(|| {
                for _ in 0..1000 {
                    enter(runtime);
                    stack.pop(runtime).unwrap();
                }
            });
            let (returned, _) = measured(|| {
                for _ in 0..1000 {
                    for (register, value) in arguments.iter().zip(&external) {
                        stack.write_slot(runtime, *register, value).unwrap();
                    }
                    enter(runtime);
                    let RegionExit::Return(value) = stack
                        .execute_region(runtime, &mut None, unexpected_callback)
                        .unwrap()
                    else {
                        panic!("scalar return");
                    };
                    assert!(matches!(
                        stack.finish_return(runtime, value).unwrap(),
                        ReturnAction::Continue(ExecutionAction::Script)
                    ));
                    assert_eq!(
                        stack
                            .current(runtime)
                            .unwrap()
                            .read_slot(runtime, destination.unwrap())
                            .unwrap()
                            .as_i32(),
                        Some(42)
                    );
                }
            });
            assert_eq!(
                returned,
                Counts::default(),
                "warm scalar entry, execution and return must not allocate"
            );
            let (slice, _) = measured(|| {
                for _ in 0..1000 {
                    stack
                        .push(runtime, module, callee, black_box(&external), None)
                        .unwrap();
                    stack.pop(runtime).unwrap();
                }
            });
            assert_eq!(
                window, slice,
                "register-to-window copying must add no packing allocations"
            );
            assert_eq!(
                window,
                Counts::default(),
                "warmed direct scalar frame entry must not allocate"
            );
            println!(
                "SCRIPT_FRAME_ALLOCATIONS,calls=1000,window={window:?},borrowed_slice={slice:?}"
            );

            Ok(())
        })
        .unwrap();
    drop(stack_activation);
    runtime.collect_garbage().unwrap();
    assert_eq!(runtime.gc_stats().root_registrations, baseline);
}
