use crate::engine::KagariEngine;
use kagari_source::source::SourceFile;

#[test]
fn readonly_operations_cannot_recover_write_access() {
    let cases = [
        "fn main() { val a: std::collections::List<i32> = Vec::from([1]); a.push(2); }",
        "fn main() { var a: std::collections::List<i32> = Vec::from([1]); a[0] = 2; }",
        "fn main() { val a: std::collections::List<i32> = Vec::from([1]); a[0] += 2; }",
        "fn main() { val a: std::collections::List<i32> = Vec::from([1]); Vec::push(a, 2); }",
        "fn main() { val a: std::collections::List<i32> = Vec::from([1]); set_index(a, 0, 2); }",
        "use std::collections::{HashMap, Map};\nfn main() { val a: Map<i32, i32> = HashMap::new(); a.insert(3, 4); }",
        "use std::collections::{HashMap, Map};\nfn main() { val a: Map<i32, i32> = HashMap::new(); HashMap::clear(a); }",
        "use std::collections::{HashSet, Set};\nfn main() { val a: Set<i32> = HashSet::new(); a.remove(1); }",
        "use std::collections::{HashSet, Set};\nfn main() { val a: Set<i32> = HashSet::new(); HashSet::clear(a); }",
        "fn main() { val a: std::collections::List<i32> = Vec::from([1]); val b: Vec<i32> = a; }",
        "use std::collections::{HashMap, Map};\nfn main() { val a: Map<i32, i32> = HashMap::new(); val b: HashMap<i32, i32> = a; }",
        "use std::collections::{HashSet, Set};\nfn main() { val a: Set<i32> = HashSet::new(); val b: HashSet<i32> = a; }",
        "fn change<T>(a: Vec<T>, v: T) { a.push(v); } fn main() { val a: std::collections::List<i32> = Vec::from([1]); change(a, 2); }",
        "fn bad(a: std::collections::List<i32>) -> Vec<i32> { a }",
        "struct Box { val a: Vec<i32> } fn main() { val a: std::collections::List<i32> = Vec::from([1]); Box { a } }",
        "use std::collections::{List};\nfn main() { val a = Vec::from([Vec::from([1])]); val b: Vec<List<i32>> = a; }",
        "use std::collections::{List};\nfn main() { val a = Vec::from([Vec::from([1])]); val b: List<List<i32>> = a; }",
        "fn main() { val a: std::collections::List<i32> = Vec::from([1]); val change = || a.push(2); change(); }",
        "fn main() { val a: std::collections::List<i32> = Vec::from([1]); val b = if true { a } else { Vec::from([2]) }; b.push(3); }",
        "fn main() { val a: std::collections::List<i32> = Vec::from([1]); val b = match true { true => Vec::from([2]), false => a }; b.push(3); }",
        "fn main() { val a = Vec::from([1]); a = Vec::from([2]); }",
        "use std::collections::{Map};\nfn main() { val a: Map<i32, i32> = std::map::new(); }",
        "use std::collections::{Set};\nfn main() { val a: Set<i32> = std::set::new(); }",
    ];
    for source in cases {
        let result = KagariEngine::default().compile_to_artifact(
            SourceFile::new("readonly-negative.kgr", source),
            Default::default(),
        );
        assert!(result.is_err(), "must reject: {source}");
    }
}
