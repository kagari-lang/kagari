//! Required-method examples demonstrate the specific capability being documented.
use crate::catalog::documentation::example;

pub(super) fn method(owner: &str, name: &str, description: &str, overview: &str) -> String {
    let description = format!("{description}{}", collection_details(owner, name));
    let code = match (owner, name) {
        ("List", "len") => {
            "fn main() -> bool {\n    val values: std::collections::List<i32> = Vec::from([10, 20]);\n    values.len() == 2u64\n}"
        }
        ("List", "is_empty") => {
            "fn main() -> bool {\n    val storage: Vec<i32> = Vec::new();\n    val empty: std::collections::List<i32> = storage;\n    val one: std::collections::List<i32> = Vec::from([1]);\n    empty.is_empty() && !one.is_empty()\n}"
        }
        ("List", "get") => {
            "fn main() -> bool {\n    val values: std::collections::List<i32> = Vec::from([10, 20]);\n    values.get(1u64) == Some(20) && values.get(2u64) == None\n}"
        }
        ("MutableList", "push") => {
            "fn main() -> bool {\n    val values = Vec::from([1, 2]);\n    val writable: std::collections::MutableList<i32> = values;\n    writable.push(3);\n    values.len() == 3u64 && values.get(2u64) == Some(3)\n}"
        }
        ("MutableList", "pop") => {
            "fn main() -> bool {\n    val values: std::collections::MutableList<i32> = Vec::from([10, 20]);\n    values.pop() == Some(20) && values.pop() == Some(10) && values.pop() == None\n}"
        }
        ("MutableList", "insert") => {
            "fn main() -> bool {\n    val values: std::collections::MutableList<i32> = Vec::from([10, 30]);\n    values.insert(1u64, 20);\n    values.len() == 3u64 && values.get(1u64) == Some(20) && values.get(2u64) == Some(30)\n}"
        }
        ("MutableList", "remove") => {
            "fn main() -> bool {\n    val values: std::collections::MutableList<i32> = Vec::from([10, 20, 30]);\n    val removed = values.remove(1u64);\n    removed == Some(20) && values.get(1u64) == Some(30) && values.remove(9u64) == None\n}"
        }
        ("MutableList", "clear") => {
            "fn main() -> bool {\n    val values: std::collections::MutableList<i32> = Vec::from([10, 20]);\n    values.clear();\n    values.is_empty() && values.get(0u64) == None\n}"
        }
        ("MutableList", "set") => {
            "fn main() -> bool {\n    val values: std::collections::MutableList<i32> = Vec::from([10, 20]);\n    values.set(1u64, 99);\n    values.len() == 2u64 && values.get(0u64) == Some(10) && values.get(1u64) == Some(99)\n}"
        }
        ("Map", "len") => {
            "fn main() -> bool {\n    val data: std::collections::HashMap<i32, i32> = std::collections::HashMap::new();\n    data.insert(1, 10).insert(1, 20);\n    val view: std::collections::Map<i32, i32> = data;\n    view.len() == 1u64\n}"
        }
        ("Map", "is_empty") => {
            "fn main() -> bool {\n    val data: std::collections::HashMap<i32, i32> = std::collections::HashMap::new();\n    val view: std::collections::Map<i32, i32> = data;\n    val before = view.is_empty();\n    data.insert(7, 10);\n    before && !view.is_empty()\n}"
        }
        ("Map", "get") => {
            "fn main() -> bool {\n    val data: std::collections::HashMap<i32, String> = std::collections::HashMap::new();\n    data.insert(1, \"one\");\n    val view: std::collections::Map<i32, String> = data;\n    view.get(1) == Some(\"one\") && view.get(2) == None\n}"
        }
        ("Map", "contains_key") => {
            "fn main() -> bool {\n    val data: std::collections::HashMap<i32, i32> = std::collections::HashMap::new();\n    data.insert(7, 42);\n    val view: std::collections::Map<i32, i32> = data;\n    view.contains_key(7) && !view.contains_key(42)\n}"
        }
        ("MutableMap", "insert") => {
            "fn main() -> bool {\n    val storage: std::collections::HashMap<i32, String> = std::collections::HashMap::new();\n    val values: std::collections::MutableMap<i32, String> = storage;\n    values.insert(1, \"first\");\n    values.insert(1, \"replacement\");\n    values.len() == 1u64 && values.get(1) == Some(\"replacement\")\n}"
        }
        ("MutableMap", "remove") => {
            "fn main() -> bool {\n    val storage: std::collections::HashMap<i32, i32> = std::collections::HashMap::new();\n    val values: std::collections::MutableMap<i32, i32> = storage;\n    values.insert(7, 42);\n    values.remove(7) == Some(42) && values.remove(7) == None && values.is_empty()\n}"
        }
        ("MutableMap", "clear") => {
            "fn main() -> bool {\n    val storage: std::collections::HashMap<i32, i32> = std::collections::HashMap::new();\n    val values: std::collections::MutableMap<i32, i32> = storage;\n    values.insert(7, 42);\n    values.clear();\n    values.is_empty() && values.get(7) == None\n}"
        }
        ("Set", "len") => {
            "fn main() -> bool {\n    val values: std::collections::Set<i32> = std::collections::HashSet::from([1, 1, 2]);\n    values.len() == 2u64\n}"
        }
        ("Set", "is_empty") => {
            "fn main() -> bool {\n    val storage: std::collections::HashSet<i32> = std::collections::HashSet::new();\n    val values: std::collections::Set<i32> = storage;\n    values.is_empty()\n}"
        }
        ("Set", "contains") => {
            "fn main() -> bool {\n    val values: std::collections::Set<i32> = std::collections::HashSet::from([1, 2]);\n    values.contains(1) && !values.contains(3)\n}"
        }
        ("MutableSet", "insert") => {
            "fn main() -> bool {\n    val storage: std::collections::HashSet<i32> = std::collections::HashSet::new();\n    val values: std::collections::MutableSet<i32> = storage;\n    values.insert(7);\n    values.insert(7);\n    values.len() == 1u64 && values.contains(7)\n}"
        }
        ("MutableSet", "remove") => {
            "fn main() -> bool {\n    val values: std::collections::MutableSet<i32> = std::collections::HashSet::from([7]);\n    values.remove(7) && !values.remove(7) && values.is_empty()\n}"
        }
        ("MutableSet", "clear") => {
            "fn main() -> bool {\n    val values: std::collections::MutableSet<i32> = std::collections::HashSet::from([1, 2]);\n    values.clear();\n    values.is_empty() && !values.contains(1)\n}"
        }
        ("PartialEq", "eq") => "fn main() -> bool {\n    7.eq(7) && !7.eq(8)\n}",
        ("PartialOrd", "partial_cmp") => {
            "use core::cmp::Ordering;\nfn main() -> bool {\n    val nan = f64::from_bits(9221120237041090561u64);\n    2.partial_cmp(3) == Some(Ordering::Less) && nan.partial_cmp(1.0f64) == None\n}"
        }
        ("Ord", "cmp") => {
            "use core::cmp::Ordering;\nfn main() -> bool {\n    2.cmp(3) == Ordering::Less && 3.cmp(3) == Ordering::Equal && 4.cmp(3) == Ordering::Greater\n}"
        }
        ("Hash", "hash") => {
            "fn main() -> bool {\n    val original = \"café\";\n    val copy = original.replace(\"fé\", \"fé\");\n    original == copy && original.hash() == copy.hash()\n}"
        }
        ("Debug", "debug") => {
            "fn main() -> bool {\n    42.debug() == \"42\" && true.debug() == \"true\" && 'é'.debug() == \"'é'\"\n}"
        }
        ("Display", "display") => {
            "fn main() -> bool {\n    42.display() == \"42\" && 'é'.display() == \"é\"\n}"
        }
        ("From", "from") => {
            "fn main() -> bool {\n    i64::from(42i32) == 42i64 && i64::from(-7i32) == -7i64\n}"
        }
        ("Into", "into") => {
            "fn main() -> bool {\n    val wide: i64 = 42i32.into();\n    wide == 42i64\n}"
        }
        ("TryFrom", "try_from") => {
            "use core::num::TryFromIntError;\n\nfn main() -> bool {\n    (\n        match i8::try_from(127i32) {\n            Ok(value) => value == 127i8,\n            Err(_) => false,\n        }\n    ) &&\n        (\n        match i8::try_from(128i32) {\n            Err(TryFromIntError::OutOfRange) => true,\n            _ => false,\n        }\n    )\n}"
        }
        ("TryInto", "try_into") => {
            "use core::num::TryFromIntError;\n\nfn main() -> bool {\n    val good: Result<i8, TryFromIntError> = 127i32.try_into();\n    val bad: Result<i8, TryFromIntError> = 128i32.try_into();\n    (\n        match good {\n            Ok(value) => value == 127i8,\n            Err(_) => false,\n        }\n    ) &&\n        (\n        match bad {\n            Err(TryFromIntError::OutOfRange) => true,\n            _ => false,\n        }\n    )\n}"
        }
        ("FromStr", "from_str") => {
            "use core::num::ParseError;\n\nfn main() -> bool {\n    (\n        match i32::from_str(\"42\") {\n            Ok(value) => value == 42,\n            Err(_) => false,\n        }\n    ) &&\n        (\n        match i32::from_str(\"\") {\n            Err(ParseError::Empty) => true,\n            _ => false,\n        }\n    ) &&\n        (\n        match i32::from_str(\"12x\") {\n            Err(ParseError::InvalidDigit) => true,\n            _ => false,\n        }\n    )\n}"
        }
        ("Default", "default") | ("Fn", "call") => overview,
        ("RangeBounds", "start_bound") => {
            "use core::ops::Bound;\nfn main() -> bool {\n    (1..4).start_bound() == Bound::Included(1) && (..4).start_bound() == Bound::Unbounded\n}"
        }
        ("RangeBounds", "end_bound") => {
            "use core::ops::Bound;\nfn main() -> bool {\n    (1..4).end_bound() == Bound::Excluded(4) && (1..=4).end_bound() == Bound::Included(4)\n}"
        }
        ("Try", "from_output") => {
            "fn main() -> bool {\n    Option<i32>::from_output(42) == Some(42)\n}"
        }
        ("Try", "branch") => {
            "use core::ops::ControlFlow;\nfn main() -> bool {\n    val value: Option<i32> = Some(42);\n    match value.branch() {\n        ControlFlow::Continue(item) => item == 42,\n        ControlFlow::Break(_) => false,\n    }\n}"
        }
        ("FromResidual", "from_residual") => {
            "fn main() -> bool {\n    val residual: Option<core::convert::Infallible> = None;\n    val result: Option<i32> = Option<i32>::from_residual(residual);\n    result == None\n}"
        }
        _ => return example(&description, operator(owner)),
    };
    example(&description, code)
}

fn collection_details(owner: &str, name: &str) -> &'static str {
    match (owner, name) {
        ("List", "len") => {
            "\n\nCounts elements as u64, not bytes. Reading through this view does not freeze\nthe storage; mutations through other aliases can change a later result."
        }
        ("List", "get") => {
            "\n\nThe argument is a zero-based element index. Returns Some(value) below len and\nNone otherwise, including on empty input. In contrast, indexing with [] traps\nfor an invalid index. Referenced element values retain their shared identity."
        }
        ("Map", "get") => {
            "\n\nReturns the value associated with an equal key. None means the key is absent;\nit does not remove an entry. HashMap invokes the key's Hash and Eq operations\nsynchronously and rejects structural mutation during that lookup."
        }
        ("MutableList", "insert") => {
            "\n\nIndex len appends; smaller indices insert before the existing element.\nReturns Unit. Vec aliases observe the edit and existing cursors reject later\nreads after a structural change."
        }
        ("MutableList", "remove") => {
            "\n\nThe argument is a zero-based element index. Later elements shift left when\nan item is removed. An out-of-range index returns None and leaves slots unchanged."
        }
        ("MutableList", "set") => {
            "\n\nThe zero-based index must be below len. Returns Unit without changing the\nlength. Existing aliases observe the replacement in the same sequence."
        }
        ("MutableList", "push" | "pop" | "clear") => {
            "\n\nMutates the shared sequence storage. Vec aliases observe completed edits.\nA committed structural change invalidates existing cursors; active guarded\noperations such as sorting reject conflicting structural edits."
        }
        ("MutableMap", "insert") => {
            "\n\nThis interface method returns Unit; HashMap's inherent insert instead returns\nthe same map for chaining. Inserting an equal key replaces its value without\nincreasing len. HashMap iteration order is unspecified."
        }
        ("MutableMap", "remove") => {
            "\n\nReturns the removed value in Some, or None when no equal key exists. Other\nentries remain. HashMap aliases observe the removal; entry order is unspecified."
        }
        ("MutableSet", "insert") => {
            "\n\nThis interface method returns Unit; HashSet's inherent insert returns the\nsame set for chaining. Repeated insertion of an equal member does not increase\nlen. HashSet iteration order is unspecified."
        }
        ("MutableSet", "remove") => {
            "\n\nReturns true only when an equal member was present and removed. Removing an\nabsent member returns false and leaves membership unchanged."
        }
        ("MutableMap" | "MutableSet", "clear") => {
            "\n\nThis interface method returns Unit; the concrete hash collection's inherent\nclear returns the same storage. Aliases observe the empty collection and live\ncursors reject reads after a committed structural change."
        }
        ("Map" | "Set", "len" | "is_empty" | "contains" | "contains_key") => {
            "\n\nThe view retains shared storage, so another alias can change later results.\nHash collection key equality and hashing must remain consistent while stored.\nIteration order is unspecified for HashMap and HashSet."
        }
        _ => "",
    }
}

fn operator(owner: &str) -> &'static str {
    match owner {
        "Add" => "fn main() -> bool {\n    2 + 3 == 5 && -2 + 3 == 1\n}",
        "Sub" => "fn main() -> bool {\n    5 - 3 == 2 && 3 - 5 == -2\n}",
        "Mul" => "fn main() -> bool {\n    3 * 4 == 12 && -3 * 4 == -12\n}",
        "Div" => "fn main() -> bool {\n    7 / 2 == 3 && -7 / 2 == -3\n}",
        "Rem" => "fn main() -> bool {\n    7 % 3 == 1 && -7 % 3 == -1\n}",
        "BitAnd" => "fn main() -> bool {\n    (0b1010 & 0b1100) == 0b1000\n}",
        "BitOr" => "fn main() -> bool {\n    (0b1010 | 0b0001) == 0b1011\n}",
        "BitXor" => "fn main() -> bool {\n    (0b1010 ^ 0b0011) == 0b1001\n}",
        "Shl" => "fn main() -> bool {\n    (3 << 2) == 12\n}",
        "Shr" => "fn main() -> bool {\n    (12 >> 2) == 3\n}",
        "Neg" => "fn main() -> bool {\n    val value = 7;\n    -value == -7\n}",
        "Not" => "fn main() -> bool {\n    !false && !!true\n}",
        "Index" => {
            "fn main() -> bool {\n    val values = Vec::from([10, 20]);\n    values[0u64] == 10 && values[1u64] == 20\n}"
        }
        _ => unreachable!("required operation needs authored documentation"),
    }
}
