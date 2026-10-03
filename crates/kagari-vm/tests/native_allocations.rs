//! Allocation accounting isolates prepared native invocation from frame setup and compilation.
mod native_allocations_counter;

use kagari_abi::{ids::FunctionRef, scalar::BuiltinType, types::AbiType};
use kagari_bytecode::instruction::{BytecodeInstruction, CallTarget, NativeImportId, Register};
use kagari_common::source_database::{SourceDatabase, SourceLayer};
use kagari_compiler::{bytecode::lower_program_to_bytecode, source::program::lower_program_to_mir};
use kagari_hir::analysis::AnalysisDatabase;
use kagari_runtime::{
    Runtime,
    module::LoadedModule,
    native::{
        binding::NativeResult,
        builder::ModuleBuilder,
        context::{CallContext, ScriptCall},
        declarations::FunctionDecl,
        language::LanguageContracts,
        module::NativeModule,
        types::Type,
        views::SequenceHandle,
    },
    value::Value,
};
use native_allocations_counter::{Counts, measured, verify_counter};
use std::{hint::black_box, time::Duration};

fn module() -> NativeModule {
    let language = LanguageContracts::default();
    let mut module = ModuleBuilder::new("measure::native", &language);
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
            |_cx: &mut CallContext<'_>, a: i32, b: i32| -> NativeResult<i32> { Ok(a + b) },
        )
        .unwrap();
    let sum = module
        .define_function(
            FunctionDecl::new("sum")
                .parameter("values", language.array_list(Type::i32()))
                .returns(Type::i32()),
        )
        .unwrap();
    module
        .bind(
            sum,
            |_cx: &mut CallContext<'_>, values: SequenceHandle<'_>| -> NativeResult<i32> {
                values.with_slice::<i32, _>(|values| Ok(values.iter().copied().sum()))
            },
        )
        .unwrap();
    module.finish().unwrap()
}

fn load() -> (Runtime, LoadedModule) {
    let module = module();
    let mut sources = SourceDatabase::default();
    let root = sources
        .set(
            "allocation.kgr",
            r#"
        use measure::native::{add, sum};
        fn scalar(a: i32, b: i32) -> i32 { add(a, b) }
        fn bulk(values: ArrayList<i32>) -> i32 { sum(values) }
    "#
            .into(),
            SourceLayer::Base,
        )
        .unwrap();
    let mut analysis = AnalysisDatabase::default();
    analysis.set_native_modules(vec![module.declaration().clone()]);
    let snapshot = analysis
        .snapshot(sources.snapshot(), &Default::default())
        .unwrap();
    let checked = snapshot.check_program(root, &Default::default()).unwrap();
    let mir = lower_program_to_mir(&checked, &Default::default()).unwrap();
    let program = lower_program_to_bytecode(&mir).unwrap();
    let mut runtime = Runtime::default();
    module.install(&mut runtime).unwrap();
    let loaded = runtime.load_program("allocation", program).unwrap();
    (runtime, loaded)
}

struct CallSite {
    function: FunctionRef,
    import: NativeImportId,
    arguments: Vec<Register>,
    destination: Option<Register>,
}

impl CallSite {
    fn find(loaded: &LoadedModule, name: &str) -> Self {
        let function = loaded
            .bytecode
            .functions
            .iter()
            .find(|function| function.name == name)
            .unwrap();
        let (import, arguments, destination) = function
            .instructions
            .iter()
            .find_map(|instruction| match instruction {
                BytecodeInstruction::Call {
                    callee: CallTarget::Native(import),
                    args,
                    dst,
                } => Some((*import, args.clone(), *dst)),
                _ => None,
            })
            .expect("ordinary direct native call");
        Self {
            function: function.id,
            import,
            arguments,
            destination,
        }
    }
}

fn unexpected_callback(
    _: &Runtime,
    _: &LoadedModule,
    _: ScriptCall<'_>,
    _: &[Value],
) -> NativeResult<Value> {
    panic!("the measured scalar and bulk entries have no script callbacks")
}

fn sample(
    runtime: &Runtime,
    loaded: &LoadedModule,
    site: &CallSite,
    arguments: &[Value],
    calls: usize,
    expected: i32,
) -> (Counts, Duration) {
    let stack = runtime.enter_execution_stack(loaded).unwrap();
    stack
        .push(loaded.slot(), site.function, arguments, None)
        .unwrap();
    for (register, value) in site.arguments.iter().zip(arguments) {
        stack
            .current_mut()
            .unwrap()
            .write_register(*register, value.clone())
            .unwrap();
    }
    let invoke = || {
        stack
            .invoke_native(
                runtime,
                site.import,
                &site.arguments,
                site.destination,
                unexpected_callback,
            )
            .unwrap()
    };
    // Warm the boundary and initialize every thread-local/cache outside accounting.
    for _ in 0..128 {
        invoke();
    }
    let roots = runtime.gc().active_roots();
    let occupancy = runtime.gc().stats().current_heap_units;
    let sample = measured(|| {
        for _ in 0..calls {
            black_box(&invoke)();
        }
    });
    assert_eq!(
        sample.0,
        Counts::default(),
        "prepared boundary must not allocate or release Rust heap storage"
    );
    assert_eq!(runtime.gc().active_roots(), roots);
    assert_eq!(runtime.gc().stats().current_heap_units, occupancy);
    assert_eq!(
        stack
            .current()
            .unwrap()
            .read_register(site.destination.unwrap())
            .unwrap(),
        Value::I32(expected)
    );
    sample
}

#[test]
fn warmed_scalar_and_contiguous_i32_boundaries_add_no_allocations() {
    verify_counter();
    let (runtime, loaded) = load();
    let scalar = CallSite::find(&loaded, "scalar");
    let (counts, elapsed) = sample(
        &runtime,
        &loaded,
        &scalar,
        &[Value::I32(20), Value::I32(22)],
        100_000,
        42,
    );
    println!(
        "scalar,calls=100000,{counts:?},elapsed_ns={}",
        elapsed.as_nanos()
    );
    assert_eq!(runtime.gc().active_roots(), 0);
    let bulk = CallSite::find(&loaded, "bulk");
    for length in [0, 16, 16_384] {
        let array = runtime
            .alloc_array(
                &loaded,
                AbiType::Builtin(BuiltinType::I32),
                vec![Value::I32(1); length],
            )
            .unwrap();
        let (counts, elapsed) = sample(
            &runtime,
            &loaded,
            &bulk,
            &[Value::Array(array)],
            1_000,
            length as i32,
        );
        println!(
            "bulk_i32,elements={length},calls=1000,{counts:?},elapsed_ns={}",
            elapsed.as_nanos()
        );
        assert_eq!(runtime.gc().active_roots(), 0);
        assert_eq!(runtime.collect_garbage().unwrap().reclaimed_objects, 1);
    }
}
