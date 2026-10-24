use super::run;

#[test]
fn invalid_string_slices_trap_and_release_execution_state() {
    for (start, end) in [
        (2usize, 3usize),
        (1, 2),
        (7, 3),
        (0, 9),
        (9, 9),
        (0, usize::MAX),
    ] {
        let source = format!(
            "fn main() -> bool {{ val value = \"aé🙂z\".slice({start}u64, {end}u64); false }}"
        );
        let expected = format!(
            "String slice {start}..{end} requires ordered byte offsets within bounds and on UTF-8 boundaries"
        );
        run(&source, Some(&expected));
    }
}
