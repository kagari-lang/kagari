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

use kagari::{context::ExecutionContext, runtime::Runtime};
use kagari_bytecode::instruction::BytecodeInstruction;
use kagari_runtime::{
    Runtime as CoreRuntime,
    error::RuntimeError,
    frame::{ExecutionFrame, transfer::ReturnValue},
    module::{InstalledProgram, entry::ProgramEntry, execution::instruction::ExecutionInstruction},
    session::{ExecutionEvent, ExecutionObserver},
    value::{
        carriers::{RootedValue, StoredValue},
        view::ValueView,
    },
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
    layouts: HashMap<String, (u32, usize, usize, usize)>,
}

type InstructionCountMap = HashMap<Discriminant<BytecodeInstruction>, (String, u64)>;

impl ExecutionObserver for InstructionCounts {
    fn observe(
        &mut self,
        runtime: &CoreRuntime,
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
                    function.slot_count,
                    frame.physical_slot_count(),
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
    runtime: &mut Runtime,
    module: &InstalledProgram,
    context: &ExecutionContext,
    entry: ProgramEntry<'_>,
    expected: i32,
) {
    let result = runtime
        .execute(module, entry, &[], context)
        .expect("profile execution");
    assert_eq!(black_box(result.return_value.as_i32()), Some(expected));
}

pub(super) fn run(
    runtime: &mut Runtime,
    module: &InstalledProgram,
    context: &ExecutionContext,
    entry: ProgramEntry<'_>,
    name: &str,
    expected: i32,
) {
    for _ in 0..3 {
        execute(runtime, module, context, entry, expected);
    }
    let before = runtime.gc_stats();
    println!(
        "PROFILE_LAYOUT,stored_value_bytes={},host_value_bytes={},view_bytes={},instruction_bytes={},execution_bytes={},return_packet_bytes={}",
        size_of::<StoredValue>(),
        size_of::<RootedValue>(),
        size_of::<ValueView<'_>>(),
        size_of::<BytecodeInstruction>(),
        size_of::<ExecutionInstruction>(),
        size_of::<ReturnValue>()
    );
    println!("PROFILE_READY,{name}");
    io::stdout().flush().unwrap();
    let start = Instant::now();
    let mut calls = 0_u64;
    while start.elapsed() < Duration::from_secs(10) {
        execute(runtime, module, context, entry, expected);
        calls += 1;
    }
    let elapsed = start.elapsed();
    let after = runtime.gc_stats();
    // These are retained GC observations, not cumulative allocation counts or
    // the total Runtime/control/source footprint. No allocation total is inferred.
    println!(
        "PROFILE_DONE,{name},calls={calls},ns={},gc_cycles={},live_objects_delta={},root_registrations_delta={},gc_backing_bytes_delta={},provider_units_before={},provider_units_after={}",
        elapsed.as_nanos(),
        after.completed_cycles - before.completed_cycles,
        after.objects as i128 - before.objects as i128,
        after.root_registrations as i128 - before.root_registrations as i128,
        after.backing_bytes as i128 - before.backing_bytes as i128,
        before.provider_units,
        after.provider_units,
    );
    io::stdout().flush().unwrap();
    // Counting is deliberately outside the sampling window; observer callbacks
    // change dispatch cost and must never be mixed into the throughput baseline.
    count(runtime, module, context, entry, expected);
}

pub(super) fn count(
    runtime: &mut Runtime,
    module: &InstalledProgram,
    context: &ExecutionContext,
    entry: ProgramEntry<'_>,
    expected: i32,
) {
    runtime
        .set_execution_observer(InstructionCounts::default())
        .unwrap();
    {
        let selected = runtime.select_program(module).unwrap();
        let mut session = runtime
            .begin_execution(&selected, Default::default())
            .unwrap();
        session.runtime_mut().attach_execution_observer().unwrap();
        let report = session
            .runtime_mut()
            .execute(module, entry, &[], context)
            .unwrap();
        assert_eq!(report.return_value.as_i32(), Some(expected));
        drop(report);
        drop(session);
    }
    let mut counts: Vec<_> = runtime
        .execution_observer::<InstructionCounts>()
        .unwrap()
        .counts
        .borrow()
        .values()
        .cloned()
        .collect();
    {
        let observer = runtime.execution_observer::<InstructionCounts>().unwrap();
        let mut layouts = observer.layouts.iter().collect::<Vec<_>>();
        layouts.sort_by_key(|(name, _)| *name);
        for (name, (logical, physical, scalars, managed)) in layouts {
            println!(
                "FRAME_LAYOUT,{name},logical_slots={logical},physical_slots={physical},scalar_slots={scalars},managed_slots={managed},scalar_payload_bytes={},initialization_bytes={scalars},managed_value_bytes={}",
                scalars * size_of::<u64>(),
                managed * size_of::<StoredValue>(),
            );
        }
    }
    runtime.clear_execution_observer().unwrap();
    counts.sort_by_key(|(_, count)| Reverse(*count));
    println!(
        "INSTRUCTION_TOTAL,{}",
        counts.iter().map(|(_, count)| count).sum::<u64>()
    );
    for (kind, count) in counts {
        println!("INSTRUCTION,{kind},{count}");
    }
}
