use crate::engine::KagariEngine;
use kagari_source::source::SourceFile;

#[test]
fn byte_literals_reject_type_coercion_and_array_constant_contexts() {
    let engine = KagariEngine::default();
    for source in [
        "fn main() { val value: u16 = b'A'; }",
        "fn main() { val value: char = b'A'; }",
        "fn main() { val value: [u16] = b\"A\"; }",
        "fn main() { val value: [u16] = b\"\"; }",
        "fn main() { val value: [u16] = br\"\"; }",
        "const VALUE: [u8] = b\"A\"; fn main() {}",
        "fn main() { match b'A' { b'A'..=b'Z' => 1, _ => 0 }; }",
        "fn main() { match b'A' { b'A'..b'Z' => 1, _ => 0 }; }",
        "fn main() { match b'A' { 65u8..=90u8 => 1, _ => 0 }; }",
        "const FIRST: u8 = b'A'; const LAST: u8 = b'Z'; fn main() { match b'A' { FIRST..=LAST => 1, _ => 0 }; }",
        "fn main() { match b\"A\" { b\"A\" => 1, _ => 0 }; }",
    ] {
        assert!(
            engine
                .compile_to_artifact(
                    SourceFile::new("invalid-byte.kgr", source),
                    Default::default(),
                )
                .is_err(),
            "{source}"
        );
    }
}

#[test]
fn invalid_repeat_counts_and_read_only_mutations_are_compile_errors() {
    let engine = KagariEngine::default();
    for source in [
        "fn main() { val a = [0; -1]; }",
        "fn main() { val a: Vec<i32> = [1]; }",
        "fn main() { val a: [i32] = Vec::from([1]); }",
        "fn main() { val a: std::collections::List<i32> = [1]; }",
        "fn main() { val a = []; }",
        "fn main() { val a = [1]; a.push(2); }",
        "fn main() { val a = [1]; a.clear(); }",
        "fn main() { val a = [1]; a.pop(); }",
        "fn main() { val a = [0; true]; }",
        "fn main() { val a = [0; 1i32]; }",
        "fn main() { val a = [1, 2; 3]; }",
        "use std::collections::{List};\nfn main() { val a: List<i32> = Vec::from([0; 2]); a.push(1); }",
        "use std::collections::{List};\nfn main() { val a: List<i32> = Vec::from([0; 2]); a[0] = 1; }",
        "fn main() { val r = true..false; }",
        "fn main() { val r = 1.0..2.0; }",
        "use std::ops::{Range};\nfn wrong(r: Range<bool>) {} fn main() {}",
        "fn main() { val r = ..=; }",
        "fn main() { for n in ..3 {} }",
        "fn main() { for n in .. {} }",
        "fn main() { val r: Vec<i32> = 0..3; }",
    ] {
        assert!(
            engine
                .compile_to_artifact(SourceFile::new("invalid.kgr", source), Default::default())
                .is_err(),
            "{source}"
        );
    }
}

#[test]
fn repeated_arrays_reject_mutable_identity_even_when_nested_or_empty() {
    let engine = KagariEngine::default();
    for body in [
        "val a = [Cell { value: 1 }; 2];",
        "val a = [Cell { value: 1 }; 0];",
        "val a = [[1, 2]; 2];",
        "val a = [(Cell { value: 1 }, 7); 2];",
        "val a: [Option<Cell>] = [None; 2];",
        "val a = [Wrapped::Data(Cell { value: 1 }); 2];",
        "val a: [Wrapped<Cell>] = [Wrapped::Empty; 2];",
        "val a = [|| 1; 2];",
    ] {
        let source = format!(
            "struct Cell {{ var value: i32 }} enum Wrapped<T> {{ Empty, Data(T) }} fn main() {{ {body} }}"
        );
        let error = engine
            .compile_to_artifact(
                SourceFile::new("invalid-repeat.kgr", source),
                Default::default(),
            )
            .unwrap_err();
        assert!(
            format!("{error:?}").contains("initialize each element separately"),
            "{body}: {error:?}"
        );
    }
}
