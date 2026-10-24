use super::execute_entries;
use kagari_runtime::module::entry::ProgramEntry;

// One source program per contract group, with an independently observed entry for
// each evaluation position. `STOP` increments the counter once before returning;
// writes or later operands would change the entry's expected result.
fn returning_expressions(cases: &[(&str, &str, i32)]) {
    let mut source = String::from(
        r#"
use std::collections;
struct Count { var value: i32 }
struct Item<T> { val value: T, val flag: bool }
enum Packet<T> { Value(T, bool) }
trait Take { fn take(self, input: bool) -> i32; }
impl Take for Count { fn take(self, input: bool) -> i32 { self.value += 100; 0 } }
fn tick(count: Count) -> bool { count.value += 1; true }
fn later(count: Count) -> i32 { count.value += 100; 99 }
fn take(count: Count, value: i32) -> i32 { count.value += 100; value }
fn generic_take<T: Take>(count: Count, value: T) -> i32 { count.value += 100; value.take(true) }
fn bound_take<T: Take>(value: T, count: Count) -> i32 {
    value.take(if tick(count) { return 40; } else { return 0; })
}
fn grow<T>(value: T) -> i32 { grow((value, value)) }
fn index(value: ()) -> i32 { 0 }
fn matrix(value: ()) -> [[i32]] { [[0]] }
"#,
    );
    for &(name, body, _) in cases {
        let body = body.replace("STOP", "(if tick(count) { return 40; } else { return 0; })");
        source.push_str(&format!(
            "fn run_{name}(count: Count, array: [i32]) -> i32 {{
                var tuple = (1, true); {body}
             }}
             pub fn {name}() -> i32 {{
                val count = Count {{ value: 1 }}; val array = [10];
                run_{name}(count, array) + count.value + array[0]
             }}\n"
        ));
    }
    let entries: Vec<_> = cases
        .iter()
        .map(|&(name, _, expected)| (ProgramEntry::Export(name), expected))
        .collect();
    execute_entries(&source, &entries);
}

#[test]
fn returning_expressions_preserve_control_flow_and_short_circuiting() {
    returning_expressions(&[
        ("nested_return", "return STOP;", 52),
        ("if_condition", "if STOP { tick(count); } else { 7 }; 0", 52),
        ("while_condition", "while STOP { tick(count); } 0", 52),
        (
            "match_scrutinee",
            "match STOP { 1 => tick(count), _ => 7 }; 0",
            52,
        ),
        ("negation", "-STOP; 0", 52),
        ("not", "!STOP; 0", 52),
        ("binary_left", "STOP + later(count); 0", 52),
        ("binary_right", "later(count) + STOP; 0", 152),
        ("comparison", "STOP < later(count); 0", 52),
        ("equality", "STOP == false; 0", 52),
        ("and_taken", "true && STOP; 0", 52),
        ("or_taken", "false || STOP; 0", 52),
        ("and_skipped", "false && STOP; 0", 11),
        ("or_skipped", "true || STOP; 0", 11),
        ("helper_skipped", "false && (type_of(STOP) == \"\"); 0", 11),
    ]);
}

#[test]
fn returning_expressions_stop_calls_and_generic_instantiation() {
    returning_expressions(&[
        ("function_argument", "take(count, STOP)", 52),
        ("generic_argument", "generic_take(count, STOP)", 52),
        ("bound_argument", "bound_take(count, count)", 52),
        ("helper_argument", "type_of(STOP); 0", 52),
        ("native_receiver", "STOP.sort(); 0", 52),
        ("native_argument", "Vec::from([1]).sort_by(STOP); 0", 52),
        (
            "native_source",
            "STOP.map(|n:i32| { tick(count); n }); 0",
            52,
        ),
        ("callee", "STOP(tick(count)); 0", 52),
        ("member_callee", "STOP.missing(tick(count)); 0", 52),
        ("unreachable_instance", "take(count, STOP) + grow(1)", 52),
    ]);
}

#[test]
fn returning_expressions_stop_aggregate_construction() {
    returning_expressions(&[
        (
            "array_element",
            "val values = [later(count), STOP, true, later(count)]; 0",
            152,
        ),
        ("tuple_element", "(later(count), STOP, tick(count)); 0", 152),
        ("enum_payload", "Packet::Value(STOP, tick(count)); 0", 52),
        (
            "struct_field",
            "Item { value: STOP, flag: tick(count) }; 0",
            52,
        ),
    ]);
}

#[test]
fn returning_expressions_skip_accesses_and_uncommitted_writes() {
    returning_expressions(&[
        ("initializer", "val value: i32 = STOP; later(count)", 52),
        ("assignment", "count.value = STOP; 0", 52),
        ("compound_value", "count.value += STOP; 0", 52),
        ("array_index", "array[STOP] = later(count); 0", 52),
        ("compound_index", "array[STOP] += later(count); 0", 52),
        ("tuple_index", "tuple[STOP] = later(count); 0", 52),
        ("array_receiver", "STOP[later(count)]; 0", 52),
        ("field_receiver", "STOP.value; 0", 52),
        (
            "nested_target",
            "matrix(index(STOP))[grow(1)][0] += grow(2); 0",
            52,
        ),
        (
            "reflect_field_value",
            "set_field(count, \"value\", STOP); 0",
            52,
        ),
        ("reflect_index_value", "set_index(array, 0, STOP); 0", 52),
        (
            "reflect_index",
            "set_index(array, STOP, later(count)); 0",
            52,
        ),
        ("reflect_read_receiver", "get_field(STOP, \"value\"); 0", 52),
        (
            "reflect_write_receiver",
            "set_index(STOP, tick(count), tick(count)); 0",
            52,
        ),
    ]);
}
