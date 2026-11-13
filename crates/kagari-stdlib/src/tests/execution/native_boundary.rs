#[cfg(test)]
use crate::tests::support::bytecode::load_bytecode;
use kagari_runtime::{
    Runtime,
    error::RuntimeError,
    module::{InstalledProgram, entry::ProgramEntry},
    native::{
        binding::{Codec, NativeBinding, NativeResult},
        builder::ModuleBuilder,
        context::NativeCall,
        conversion::{IntoKagari, context::ConversionContext},
        declarations::{CallableRequirement, FunctionDecl, MethodDecl},
        module::NativeModule,
        scalar::NativeScalar,
        storage::{NativePayload, NativeStorage},
        typed::NativeContext,
        types::Type,
        value_handle::ScriptValue,
    },
    value::{
        carriers::{RootedValue, StoredValue},
        view::ValueView,
    },
};

use kagari_bytecode::program::BytecodeProgram;
use kagari_common::identity::metadata::DefinitionMetadata;
use kagari_compiler::{bytecode::lower_program_to_bytecode, source::program::lower_program_to_mir};
use kagari_gc::{
    error::GcError,
    object::{GcObject, ObjectCharge},
    scan::{EdgeVisitor, ScanBudget, ScanKind, ScanStatus},
};
use kagari_hir::{analysis::AnalysisDatabase, compile_input::program::CheckedCompilation};
use kagari_types::{scalar::BuiltinType, ty::Ty};

use kagari_source::source_database::{SourceDatabase, SourceLayer};

use crate as foundation;
use crate::declarations::StandardDeclarations;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::{ops::Bound, sync::Arc};

mod native_boundary_artifacts;
mod native_boundary_callbacks;
mod native_boundary_control;
mod native_boundary_conversion;
mod native_boundary_enums;
mod native_boundary_functions;
mod native_boundary_gc;
mod native_boundary_host;
mod native_boundary_interfaces;
mod native_boundary_payload;
mod native_boundary_references;
mod native_boundary_resources;
mod native_boundary_storage;
mod native_boundary_transfer;
mod support;

fn compile_program(
    text: &str,
    module: Option<&NativeModule>,
) -> DefinitionMetadata<BytecodeProgram> {
    let modules = foundation::modules().unwrap();
    compile_with_modules(
        text,
        modules
            .iter()
            .filter(|installed| {
                module.is_none_or(|module| {
                    module.declaration().identity != installed.declaration().identity
                })
            })
            .chain(module),
    )
}

/// Reload fixtures include only their providers, not unrelated protected foundation code.
pub(super) fn compile_without_foundation(
    text: &str,
    module: Option<&NativeModule>,
) -> DefinitionMetadata<BytecodeProgram> {
    compile_with_modules(text, module)
}

fn compile_with_modules<'a>(
    text: &str,
    modules: impl IntoIterator<Item = &'a NativeModule>,
) -> DefinitionMetadata<BytecodeProgram> {
    let mut sources = SourceDatabase::default();
    let root = sources
        .set("main.kgr", text.into(), SourceLayer::Base)
        .unwrap();
    let mut analysis = AnalysisDatabase::default();
    analysis.set_native_modules(
        modules
            .into_iter()
            .map(|module| Arc::new(module.to_declaration()))
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
    lower_program_to_bytecode(&mir, &Default::default())
        .map(|program| program.into_unverified())
        .unwrap()
}

fn compile(text: &str, module: Option<&NativeModule>) -> (Runtime, InstalledProgram) {
    let program = compile_program(text, module);
    let mut runtime = Runtime::default();
    NativeModule::install_all(&crate::modules().unwrap(), &mut runtime).unwrap();
    if let Some(module) = module {
        module.install(&mut runtime).unwrap();
    }
    let loaded = load_bytecode(&mut runtime, "boundary", program).unwrap();
    (runtime, loaded)
}

// Constant caches belong to a published/pinned program, independently of host
// value roots. Finish version-specific checks and drop bindings before retiring it.
fn publish_unused_version(vm: &mut Runtime, active: &InstalledProgram) {
    let selected = vm.select_program(active).unwrap();
    let candidate = vm
        .stage_reload_verified_program(active, None, selected.verified_program().clone())
        .unwrap();
    vm.publish_staged_reload(candidate).unwrap();
}

#[test]
fn scalar_function_executes_through_the_checked_registration() {
    let mut module = ModuleBuilder::new(
        "example::math",
        &StandardDeclarations::default()
            .catalog()
            .expect("explicit standard providers"),
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
            |_cx: &mut NativeCall<'_>, a: i32, b: i32| -> NativeResult<i32> { Ok(a + b) },
        )
        .unwrap();
    let module = module.finish().unwrap();
    let (mut vm, loaded) = compile(
        include_str!("../../../tests/fixtures/native/scalar_registration.kgr"),
        Some(&module),
    );
    let _loaded_program = loaded;
    let loaded = vm.select_program(&_loaded_program).unwrap();
    assert_eq!(
        vm.execute_module(&loaded, ProgramEntry::Main, vm.execution_options())
            .unwrap()
            .return_value
            .as_i32(),
        Some(42)
    );
}

#[test]
fn raw_result_contract_failure_releases_the_execution_scope() {
    let mut module = ModuleBuilder::new(
        "example::bad",
        &StandardDeclarations::default()
            .catalog()
            .expect("explicit standard providers"),
    )
    .unwrap();
    let function = module
        .define_function(FunctionDecl::new("broken").returns(Type::i32()))
        .unwrap();
    module
        .bind(
            function,
            NativeBinding::new(vec![], Codec::Exact(Type::i32()), |_cx| {
                Ok(ValueView::from(true))
            }),
        )
        .unwrap();
    let module = module.finish().unwrap();
    let (mut vm, loaded) = compile(
        include_str!("../../../tests/fixtures/native/wrong_raw_result.kgr"),
        Some(&module),
    );
    let _loaded_program = loaded;
    let loaded = vm.select_program(&_loaded_program).unwrap();
    assert!(
        vm.execute_module(&loaded, ProgramEntry::Main, vm.execution_options())
            .is_err()
    );
    assert_eq!(
        vm.execute_module(&loaded, "healthy", vm.execution_options())
            .unwrap()
            .return_value
            .as_i32(),
        Some(42)
    );
}

#[test]
fn foundation_defaults_execute_without_optional_modules() {
    let (mut vm, loaded) = compile(
        include_str!("../../../tests/fixtures/native/foundation_defaults.kgr"),
        None,
    );
    let _loaded_program = loaded;
    let loaded = vm.select_program(&_loaded_program).unwrap();
    assert_eq!(
        vm.execute_module(&loaded, ProgramEntry::Main, vm.execution_options())
            .unwrap()
            .return_value
            .as_i32(),
        Some(42)
    );
}

#[test]
fn custom_hash_collisions_use_selected_script_methods() {
    let (mut vm, loaded) = compile(
        include_str!("../../../tests/fixtures/native/hash_collisions.kgr"),
        None,
    );
    let _loaded_program = loaded;
    let loaded = vm.select_program(&_loaded_program).unwrap();
    assert_eq!(
        vm.execute_module(&loaded, ProgramEntry::Main, vm.execution_options())
            .unwrap()
            .return_value
            .as_i32(),
        Some(42)
    );
}

#[test]
fn selected_script_callbacks_return_directly_to_a_rust_loop() {
    let mut module = ModuleBuilder::new(
        "example::calls",
        &StandardDeclarations::default()
            .catalog()
            .expect("explicit standard providers"),
    )
    .unwrap();
    let mut step = module.define_trait("Step").unwrap();
    step.define_method(
        MethodDecl::instance("step")
            .parameter("value", Type::i32())
            .returns(Type::i32()),
    )
    .unwrap();
    let step = step.finish().unwrap();
    let repeat = module
        .define_function(FunctionDecl::new("repeat").returns(Type::i32()))
        .unwrap();
    let selected = module
        .function(&repeat, |function| {
            let item = function.type_parameter("T")?;
            function.parameter("item", item.ty())?;
            function.parameter("count", Type::i32())?;
            function.bound(item.ty(), step.apply([])?)?;
            function.requires(CallableRequirement::method(
                item.ty(),
                step.method("step")?,
            )?)
        })
        .unwrap();
    module
        .bind(
            repeat,
            NativeBinding::new(
                vec![Codec::Value, Codec::Exact(Type::i32())],
                Codec::Exact(Type::i32()),
                move |cx| {
                    let receiver = cx.argument(0)?;
                    let count = i32::decode(cx.argument(1)?)?;
                    let target = cx.selected(&selected)?;
                    let mut value = ValueView::from(0i32);
                    for _ in 0..count {
                        cx.poll()?;
                        value = cx.call_values(target, &[receiver, value])?;
                    }
                    Ok(value)
                },
            ),
        )
        .unwrap();
    let module = module.finish().unwrap();
    let (mut vm, loaded) = compile(
        include_str!("../../../tests/fixtures/native/selected_loop.kgr"),
        Some(&module),
    );
    let _loaded_program = loaded;
    let loaded = vm.select_program(&_loaded_program).unwrap();
    for entry in [ProgramEntry::Main, ProgramEntry::Export("shared")] {
        assert_eq!(
            vm.execute_module(&loaded, entry, vm.execution_options())
                .unwrap()
                .return_value
                .as_i32(),
            Some(42)
        );
    }
}

#[derive(Debug)]
struct TracedCounter {
    values: StoredValue,
    dropped: Arc<AtomicUsize>,
}

impl NativePayload for TracedCounter {
    fn registration_storage() -> NativeStorage {
        NativeStorage::payload::<Self>()
    }
}

// SAFETY: the sole script field is initialized in the provider's GC brand and
// completely scanned. Drop touches only an inert Rust counter, never a peer.
unsafe impl GcObject for TracedCounter {
    type ScanState = bool;
    const SCAN_KIND: ScanKind = ScanKind::Bounded { max_work: 1 };

    fn begin_scan(&self) -> bool {
        false
    }

    fn scan(
        &self,
        visited: &mut bool,
        budget: &mut ScanBudget,
        visitor: &mut EdgeVisitor<'_>,
    ) -> Result<ScanStatus, GcError> {
        if !*visited {
            if !budget.take(1) {
                return Ok(ScanStatus::Pending);
            }
            // SAFETY: this provider retains its initialized same-heap cell.
            if unsafe { self.values.scan(budget, visitor)? } == ScanStatus::Pending {
                return Ok(ScanStatus::Pending);
            }
            *visited = true;
        }
        Ok(ScanStatus::Complete)
    }

    fn charge(&self) -> ObjectCharge {
        ObjectCharge { units: 1, bytes: 0 }
    }
}

impl Drop for TracedCounter {
    fn drop(&mut self) {
        self.dropped.fetch_add(1, Ordering::SeqCst);
    }
}

#[test]
fn array_literals_repeats_and_empty_arrays_preserve_native_access_and_aliases() {
    let language = StandardDeclarations::default();
    let mut builder = ModuleBuilder::new(
        "example::arrays",
        &language.catalog().expect("explicit standard providers"),
    )
    .unwrap();
    let array = Type::array(Type::i32()).unwrap();
    let probe = builder
        .define_function(
            FunctionDecl::new("probe")
                .parameter("values", array)
                .returns(Type::i32()),
        )
        .unwrap();
    builder
        .bind(
            probe,
            |cx: &mut NativeContext<'_>, (values,): (ScriptValue,)| -> NativeResult<i32> {
                let owner = cx
                    .runtime()
                    .execution_root()
                    .ok_or_else(|| RuntimeError::module_validation("active script owner"))?;
                let ty = values.type_argument().clone();
                let mut conversion = ConversionContext::new(cx.runtime_mut(), &owner)?;
                let array = values.into_kagari(&mut conversion, &ty)?;
                let mut values = (0..conversion.array_len(&array)?)
                    .map(|index| {
                        i32::decode(
                            conversion
                                .array_element(&array, index)?
                                .expect("array cell"),
                        )
                    })
                    .collect::<NativeResult<Vec<_>>>()?;
                values.reverse();
                let mut total = 0;
                for (index, value) in values.into_iter().enumerate() {
                    conversion.array_set(&array, index, &ValueView::from(value))?;
                    total += value;
                }
                Ok(total)
            },
        )
        .unwrap();
    let module = builder.finish().unwrap();
    let (mut vm, loaded) = compile(
        include_str!("../../../tests/fixtures/native/array_aliases.kgr"),
        Some(&module),
    );
    let _loaded_program = loaded;
    let loaded = vm.select_program(&_loaded_program).unwrap();
    assert_eq!(
        vm.execute_module(&loaded, ProgramEntry::Main, vm.execution_options())
            .unwrap()
            .return_value
            .as_i32(),
        Some(42)
    );
}

#[test]
fn typed_array_bulk_changes_validate_before_committing_and_trace_reference_elements() {
    let (mut vm, loaded) = compile(
        include_str!("../../../tests/fixtures/native/bulk_arrays.kgr"),
        None,
    );
    let _loaded_program = loaded;
    let loaded = vm.select_program(&_loaded_program).unwrap();
    let value = vm
        .execute_module(&loaded, "values", vm.execution_options())
        .unwrap()
        .return_value;
    let wrong = vm
        .alloc_array(
            &loaded,
            Ty::Builtin(BuiltinType::Bool),
            &vec![RootedValue::from(true); 4],
        )
        .unwrap();
    {
        let mut conversion = ConversionContext::new(&mut vm, &loaded).unwrap();
        let array = conversion.import(&value).unwrap();
        assert!(
            conversion
                .array_fill(&array, &ValueView::from(true))
                .is_err()
        );
        assert_i32_array(&mut conversion, &array, &[1, 2, 3, 4]);
        conversion
            .array_copy_within(&array, Bound::Included(0), Bound::Excluded(3), 1)
            .unwrap();
        assert_i32_array(&mut conversion, &array, &[1, 1, 2, 3]);
        let wrong = conversion.import(&wrong).unwrap();
        assert!(conversion.array_copy_from(&array, &wrong).is_err());
        assert_i32_array(&mut conversion, &array, &[1, 1, 2, 3]);
        conversion
            .array_fill(&array, &ValueView::from(7i32))
            .unwrap();
        conversion
            .array_set(&array, 0, &ValueView::from(2i32))
            .unwrap();
        assert_i32_array(&mut conversion, &array, &[2, 7, 7, 7]);
    }
    drop(wrong);
    drop(value);
    let nodes = vm
        .execute_module(&loaded, "nodes", vm.execution_options())
        .unwrap()
        .return_value;
    vm.collect_garbage().unwrap();
    let child = {
        let mut conversion = ConversionContext::new(&mut vm, &loaded).unwrap();
        let array = conversion.import(&nodes).unwrap();
        let child = conversion
            .array_element(&array, 0)
            .unwrap()
            .expect("traced node");
        conversion.retain(&child).unwrap()
    };
    assert_eq!(
        vm.reflect_get_field(&child, "value").unwrap().as_i32(),
        Some(42)
    );
}

fn assert_i32_array(cx: &mut ConversionContext<'_>, array: &ValueView<'_>, expected: &[i32]) {
    assert_eq!(cx.array_len(array).unwrap(), expected.len());
    for (index, expected) in expected.iter().enumerate() {
        assert_eq!(
            cx.array_element(array, index).unwrap().unwrap().as_i32(),
            Some(*expected)
        );
    }
}

#[test]
fn every_scalar_array_preserves_its_declared_element_and_values() {
    let language = StandardDeclarations::default();
    let mut builder = ModuleBuilder::new(
        "example::scalar_arrays",
        &language.catalog().expect("explicit standard providers"),
    )
    .unwrap();
    let calls = Arc::new(AtomicUsize::new(0));
    let mut expressions = Vec::new();
    macro_rules! check {
        ($kind:ident, $rust:ty, $expected:expr, $literal:literal) => {{
            let name = concat!("check_", stringify!($kind));
            let function = builder
                .define_function(
                    FunctionDecl::new(name)
                        .parameter(
                            "values",
                            Type::array(Type::scalar(BuiltinType::$kind)).unwrap(),
                        )
                        .returns(Type::bool()),
                )
                .unwrap();
            let count = calls.clone();
            builder
                .bind(
                    function,
                    NativeBinding::new(vec![Codec::Value], Codec::Exact(Type::bool()), move |cx| {
                        let array = cx.argument(0)?;
                        assert_eq!(
                            cx.argument_type_parameter(0, 0)?.ty(),
                            &Ty::Builtin(BuiltinType::$kind)
                        );
                        let len = cx.array_len(&array)?;
                        assert!(len <= 2);
                        for index in 0..len {
                            let value = cx.array_element(&array, index)?.expect("checked cell");
                            assert_eq!(<$rust as NativeScalar>::decode(value)?, $expected);
                        }
                        count.fetch_add(1, Ordering::SeqCst);
                        Ok(ValueView::from(true))
                    }),
                )
                .unwrap();
            expressions.push(format!(
                "{name}([]) && {name}([{0}]) && {name}([{0}; 2])",
                $literal
            ));
        }};
    }

    check!(Unit, (), (), "()");
    check!(Bool, bool, true, "true");
    check!(I8, i8, 1, "1 as i8");
    check!(I16, i16, 1, "1 as i16");
    check!(I32, i32, 1, "1");
    check!(I64, i64, i64::MIN, "-9223372036854775808i64");
    check!(U8, u8, 1, "1 as u8");
    check!(U16, u16, 1, "1 as u16");
    check!(U32, u32, 1, "1 as u32");
    check!(U64, u64, u64::MAX, "18446744073709551615u64");
    check!(F32, f32, 1.0, "1.0 as f32");
    check!(F64, f64, 1.0, "1.0");
    let module = builder.finish().unwrap();
    let statements = expressions
        .iter()
        .map(|expression| format!("if !({expression}) {{ return false; }}"))
        .collect::<Vec<_>>()
        .join(" ");
    let source =
        format!("use example::scalar_arrays::*; fn main() -> bool {{ {statements} true }}");
    let (mut vm, loaded) = compile(&source, Some(&module));
    let _loaded_program = loaded;
    let loaded = vm.select_program(&_loaded_program).unwrap();
    assert_eq!(
        vm.execute_module(&loaded, ProgramEntry::Main, vm.execution_options())
            .unwrap()
            .return_value
            .as_bool(),
        Some(true)
    );
    assert_eq!(calls.load(Ordering::SeqCst), 36);
}

#[test]
fn hash_callbacks_can_collect_and_trap_without_losing_keys_or_lookup_guards() {
    let mut builder = ModuleBuilder::new(
        "example::hash_gc",
        &StandardDeclarations::default()
            .catalog()
            .expect("explicit standard providers"),
    )
    .unwrap();
    let collect = builder
        .define_function(FunctionDecl::new("collect").returns(Type::i64()))
        .unwrap();
    let count = Arc::new(AtomicUsize::new(0));
    let capture = count.clone();
    builder
        .bind(
            collect,
            move |cx: &mut NativeCall<'_>| -> NativeResult<i64> {
                cx.collect_garbage()?;
                capture.fetch_add(1, Ordering::SeqCst);
                Ok(7)
            },
        )
        .unwrap();
    let module = builder.finish().unwrap();
    let (mut vm, loaded) = compile(
        include_str!("../../../tests/fixtures/native/hash-gc-and-trap.kgr"),
        Some(&module),
    );
    let _loaded_program = loaded;
    let loaded = vm.select_program(&_loaded_program).unwrap();
    vm.collect_garbage().unwrap();
    let baseline = vm.gc_stats();
    assert!(
        vm.execute_module(&loaded, "failing", vm.execution_options())
            .is_err()
    );
    vm.collect_garbage().unwrap();
    assert_eq!(
        vm.execute_module(&loaded, "healthy", vm.execution_options())
            .unwrap()
            .return_value
            .as_i32(),
        Some(42)
    );
    assert!(count.load(Ordering::SeqCst) > 10);
    vm.collect_garbage().unwrap();
    assert_eq!(vm.gc_stats().objects, baseline.objects);
}

#[test]
fn native_constructor_supplies_a_traced_payload_without_a_default_factory() {
    let language = StandardDeclarations::default();
    let mut builder = ModuleBuilder::new(
        "example::provided",
        &language.catalog().expect("explicit standard providers"),
    )
    .unwrap();
    let mut declaration = builder.define_type("Counter").unwrap();
    declaration.type_parameter("T").unwrap();
    declaration
        .native_storage(NativeStorage::payload::<TracedCounter>())
        .unwrap();
    let counter = declaration.finish().unwrap();
    let ty = counter.apply([Type::i32()]).unwrap();
    let dropped = Arc::new(AtomicUsize::new(0));
    let new = builder
        .define_function(
            FunctionDecl::new("new")
                .parameter("values", Type::array(Type::i32()).unwrap())
                .returns(ty.clone()),
        )
        .unwrap();
    let capture = dropped.clone();
    builder
        .bind(
            new,
            NativeBinding::new(vec![Codec::Value], counter.codec(), move |cx| {
                let values = cx.argument(0)?;
                cx.allocate_result_with(&[], |native| {
                    let values = native.value(&values)?;
                    // SAFETY: the returned provider owns the initialized field and
                    // its GcObject scanner visits it throughout the same heap life.
                    unsafe {
                        native.scope().alloc_with(|mut init| {
                            Ok(TracedCounter {
                                values: StoredValue::initialize(&mut init, values)?,
                                dropped: capture.clone(),
                            })
                        })
                    }
                })
            }),
        )
        .unwrap();
    let wrong = builder
        .define_function(FunctionDecl::new("missing_payload").returns(ty.clone()))
        .unwrap();
    builder
        .bind(
            wrong,
            NativeBinding::new(vec![], counter.codec(), |cx| cx.allocate_result()),
        )
        .unwrap();
    let read = builder
        .define_function(
            FunctionDecl::new("read")
                .parameter("counter", ty)
                .returns(Type::i32()),
        )
        .unwrap();
    builder
        .bind(
            read,
            NativeBinding::new(vec![counter.codec()], Codec::Exact(Type::i32()), |cx| {
                let receiver = cx.argument(0)?;
                let values = cx.value_payload::<TracedCounter>(&receiver, |scope, payload| {
                    StoredValue::get_field(scope, payload, |payload| Some(&payload.values))
                })?;
                cx.array_element(&values, 0)?
                    .ok_or_else(|| RuntimeError::module_validation("counter element"))
            }),
        )
        .unwrap();
    let module = builder.finish().unwrap();
    let (mut vm, loaded) = compile(
        include_str!("../../../tests/fixtures/native/traced_constructor.kgr"),
        Some(&module),
    );
    let _loaded_program = loaded;
    let loaded = vm.select_program(&_loaded_program).unwrap();
    let error = vm
        .execute_module(&loaded, "invalid", vm.execution_options())
        .unwrap_err();
    assert!(error.to_string().contains("explicitly supplied payload"));
    vm.collect_garbage().unwrap();
    let baseline = vm.gc_stats();
    let root = vm
        .execute_module(&loaded, ProgramEntry::Main, vm.execution_options())
        .unwrap()
        .return_value;
    vm.collect_garbage().unwrap();
    assert_eq!(dropped.load(Ordering::SeqCst), 0);
    assert_eq!(
        vm.execute(
            &_loaded_program,
            "read_retained",
            std::slice::from_ref(&root),
            &Default::default()
        )
        .unwrap()
        .return_value
        .as_i32(),
        Some(42)
    );
    assert_eq!(
        vm.execute_module(&loaded, "healthy", vm.execution_options())
            .unwrap()
            .return_value
            .as_i32(),
        Some(42)
    );
    drop(root);
    vm.collect_garbage().unwrap();
    assert_eq!(vm.gc_stats().objects, baseline.objects);
    assert_eq!(
        vm.gc_stats().root_registrations,
        baseline.root_registrations
    );
    assert_eq!(dropped.load(Ordering::SeqCst), 2);
}
