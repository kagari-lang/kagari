//! Host retention and a repeatable baseline for nonmoving mark-sweep pauses.
use kagari_bytecode::{
    module::BytecodeModule,
    program::{BytecodeProgram, ModuleRef},
};
use kagari_runtime::{Runtime, value::Value};
use kagari_types::{collection::CollectionAccess, scalar::BuiltinType, ty::Ty};

fn main() {
    const LEAVES: usize = 10_000;
    const OBJECTS: usize = LEAVES + 1;
    println!("repeat,objects,live_mark_ns,dead_sweep_ns,reclaimed_units");
    for repeat in 0..5 {
        let mut runtime = Runtime::default();
        let owner = runtime
            .load_program(
                "arrays",
                BytecodeProgram {
                    root: ModuleRef::new(0),
                    modules: vec![BytecodeModule::default()],
                },
            )
            .unwrap();
        // A typed array of scalar arrays makes the graph shape and storage cost explicit.
        let element = Ty::Builtin(BuiltinType::I32);
        let mut leaves = Vec::with_capacity(LEAVES);
        for _ in 0..LEAVES {
            leaves.push(Value::Array(
                runtime
                    .alloc_array(&owner, element.clone(), vec![Value::I32(1)])
                    .unwrap(),
            ));
        }
        let value = Value::Array(
            runtime
                .alloc_array(
                    &owner,
                    Ty::Array(Box::new(element), CollectionAccess::Mutable),
                    leaves,
                )
                .unwrap(),
        );
        // A cloned naked Value is not a root. Host state retains this handle.
        let retained = runtime.root_value(value).unwrap();
        let live = runtime.collect_garbage().unwrap();
        assert_eq!(live.live_objects, OBJECTS);
        drop(retained);
        let dead = runtime.collect_garbage().unwrap();
        assert_eq!(dead.reclaimed_objects, OBJECTS);
        assert_eq!(runtime.gc().allocated_objects(), 0);
        // Collection releases live occupancy used by the collector.
        let counters = runtime.resources().counters();
        assert_eq!(counters.current_heap_units, 0);

        println!(
            "{repeat},{OBJECTS},{},{},{}",
            live.pause.as_nanos(),
            dead.pause.as_nanos(),
            dead.reclaimed_units
        );
    }
}
