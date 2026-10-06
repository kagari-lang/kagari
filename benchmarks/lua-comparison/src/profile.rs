//! Execution-only sampling window and an independent instruction-counting pass.
use std::{
    cell::{Cell, RefCell},
    cmp::Reverse,
    collections::HashMap,
    hint::black_box,
    io::{self, Write},
    mem::{Discriminant, discriminant, size_of},
    rc::Rc,
    time::{Duration, Instant},
};

use kagari_bytecode::instruction::BytecodeInstruction;
use kagari_common::identity::table::DefinitionId;
use kagari_embed::{context::ExecutionContext, runtime::KagariRuntime};
use kagari_runtime::{
    Runtime,
    error::RuntimeError,
    frame::ExecutionFrame,
    module::{LoadedModule, execution::ExecutionInstruction},
    session::{ExecutionEvent, ExecutionObserver},
    value::Value,
};
use mlua::{Function, HookTriggers, Lua, VmState};

pub(super) fn count_lua(lua: &Lua, entry: &Function, expected: i32) {
    let count = Rc::new(Cell::new(0_u64));
    let observed = count.clone();
    lua.set_hook(HookTriggers::new().every_nth_instruction(1), move |_, _| {
        observed.set(observed.get() + 1);
        Ok(VmState::Continue)
    })
    .unwrap();
    let result = entry.call::<i32>(());
    lua.remove_hook();
    assert_eq!(result.unwrap(), expected);
    println!("LUA_INSTRUCTION_TOTAL,{}", count.get());
}

#[derive(Debug, Default)]
struct InstructionCounts {
    counts: RefCell<InstructionCountMap>,
    layouts: HashMap<String, (u16, usize, u16, usize, usize)>,
}

type InstructionCountMap = HashMap<Discriminant<BytecodeInstruction<DefinitionId>>, (String, u64)>;

impl ExecutionObserver for InstructionCounts {
    fn observe(
        &mut self,
        runtime: &Runtime,
        event: ExecutionEvent,
        frames: &[ExecutionFrame],
    ) -> Result<(), RuntimeError> {
        if event != ExecutionEvent::BeforeInstruction {
            return Ok(());
        }
        if let Some(frame) = frames.last()
            && let Some(instruction) = frame
                .function()
                .and_then(|function| function.instructions.get(frame.instruction_offset()))
        {
            let function = frame.function().expect("script instruction");
            let (scalars, managed) = frame.storage_counts(runtime)?;
            self.layouts
                .entry(format!("{}::{}", frame.loaded().name, function.name))
                .or_insert((
                    function.register_count,
                    frame.physical_register_count(),
                    function.local_count,
                    scalars,
                    managed,
                ));
            let mut counts = self.counts.borrow_mut();
            let (_, count) = counts.entry(discriminant(instruction)).or_insert_with(|| {
                let text = format!("{instruction:?}");
                (text.split([' ', '(', '{']).next().unwrap().to_owned(), 0)
            });
            *count += 1;
        }
        Ok(())
    }
}

fn execute(
    runtime: &KagariRuntime,
    module: &LoadedModule,
    context: &ExecutionContext,
    expected: i32,
) {
    let result = runtime
        .execute(module, "main", &[], context)
        .expect("profile execution");
    assert_eq!(
        black_box(
            result
                .return_value
                .value(runtime.runtime().gc())
                .expect("retained execution result")
        ),
        Value::I32(expected)
    );
}

pub(super) fn run(
    runtime: &KagariRuntime,
    module: &LoadedModule,
    context: &ExecutionContext,
    name: &str,
    expected: i32,
) {
    for _ in 0..3 {
        execute(runtime, module, context, expected);
    }
    let before = runtime.runtime().gc().stats();
    println!(
        "PROFILE_LAYOUT,value_bytes={},instruction_bytes={},execution_bytes={}",
        size_of::<Value>(),
        size_of::<BytecodeInstruction<DefinitionId>>(),
        size_of::<ExecutionInstruction>()
    );
    println!("PROFILE_READY,{name}");
    io::stdout().flush().unwrap();
    let start = Instant::now();
    let mut calls = 0_u64;
    while start.elapsed() < Duration::from_secs(10) {
        execute(runtime, module, context, expected);
        calls += 1;
    }
    let elapsed = start.elapsed();
    let after = runtime.runtime().gc().stats();
    // allocated_objects counts live objects, so include reclaimed objects to
    // recover cumulative allocations during the window.
    println!(
        "PROFILE_DONE,{name},calls={calls},ns={},gc_collections={},heap_object_allocations={},live_objects_delta={}",
        elapsed.as_nanos(),
        after.collections - before.collections,
        after.allocated_objects as i128 - before.allocated_objects as i128
            + (after.reclaimed_objects - before.reclaimed_objects) as i128,
        after.allocated_objects as i128 - before.allocated_objects as i128
    );
    io::stdout().flush().unwrap();
    // Counting is deliberately outside the sampling window; observer callbacks
    // change dispatch cost and must never be mixed into the throughput baseline.
    runtime
        .runtime()
        .set_execution_observer(InstructionCounts::default())
        .unwrap();
    {
        let session = runtime
            .runtime()
            .begin_execution(module, Default::default())
            .unwrap();
        runtime.runtime().attach_execution_observer().unwrap();
        execute(runtime, module, context, expected);
        drop(session);
    }
    let mut counts: Vec<_> = runtime
        .runtime()
        .execution_observer::<InstructionCounts>()
        .unwrap()
        .counts
        .borrow()
        .values()
        .cloned()
        .collect();
    {
        let observer = runtime
            .runtime()
            .execution_observer::<InstructionCounts>()
            .unwrap();
        let mut layouts = observer.layouts.iter().collect::<Vec<_>>();
        layouts.sort_by_key(|(name, _)| *name);
        for (name, (logical, physical, locals, scalars, managed)) in layouts {
            println!(
                "FRAME_LAYOUT,{name},logical_registers={logical},physical_registers={physical},locals={locals},scalar_slots={scalars},managed_slots={managed},scalar_payload_bytes={},initialization_bytes={scalars},managed_value_bytes={}",
                scalars * size_of::<u64>(),
                managed * size_of::<Value>(),
            );
        }
    }
    runtime.runtime().clear_execution_observer().unwrap();
    counts.sort_by_key(|(_, count)| Reverse(*count));
    println!(
        "INSTRUCTION_TOTAL,{}",
        counts.iter().map(|(_, count)| count).sum::<u64>()
    );
    for (kind, count) in counts {
        println!("INSTRUCTION,{kind},{count}");
    }
}
