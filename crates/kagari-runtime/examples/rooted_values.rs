//! Host retention and a repeatable baseline for nonmoving mark-sweep pauses.
use kagari_runtime::{Runtime, value::Value};

fn main() {
    const OBJECTS: usize = 10_000;
    println!("repeat,objects,live_mark_ns,dead_sweep_ns,reclaimed_units");
    for repeat in 0..5 {
        let runtime = Runtime::default();
        let mut value = Value::Unit;
        for _ in 0..OBJECTS {
            value = Value::Array(runtime.alloc_array(vec![value]).unwrap());
        }
        // A cloned naked Value is not a root. Host state retains this handle.
        let retained = runtime.root_value(value).unwrap();
        let live = runtime.collect_garbage().unwrap();
        assert_eq!(live.live_objects, OBJECTS);
        drop(retained);
        let dead = runtime.collect_garbage().unwrap();
        assert_eq!(dead.reclaimed_objects, OBJECTS);
        assert_eq!(runtime.gc().allocated_objects(), 0);
        // Collection releases live occupancy, not the execution allocation budget.
        let counters = runtime.resources().counters();
        assert_eq!(counters.current_heap_units, 0);
        assert_eq!(counters.allocation_units, OBJECTS * 2);
        println!(
            "{repeat},{OBJECTS},{},{},{}",
            live.pause.as_nanos(),
            dead.pause.as_nanos(),
            dead.reclaimed_units
        );
    }
}
