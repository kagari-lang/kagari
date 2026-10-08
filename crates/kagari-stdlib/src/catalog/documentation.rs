//! Standard-library Markdown lives with the registrations that define the API.
use kagari_common::identity::{DefinitionKind, DefinitionPathSegment};
use kagari_types::{
    declaration::{TypeDefKind, module::ModuleDecl},
    scalar::BuiltinType,
    ty::Ty,
};
use std::collections::BTreeMap;

fn example(description: &str, code: &str) -> String {
    format!("{description}\n\n# Examples\n\n```kgr\n{code}\n```")
}

fn trait_docs(name: &str) -> (&'static str, &'static str) {
    match name {
        "Iterator" => (
            "A shared cursor. `next` advances once and returns `Some(item)`, or `None` when exhausted. Copies share progress; previously completed effects survive a failure.",
            "fn first(values: Vec<i32>) -> Option<i32> { values.iter().next() }",
        ),
        "Iterable" => (
            "Produces an iterator whose `Item` matches the sequence's item type. Creating an iterator does not consume an item. Structural mutation of an active built-in iteration source traps.",
            "fn total(values: Vec<i32>) -> i32 { var sum = 0; for value in values { sum += value; } sum }",
        ),
        "PartialEq" => (
            "Equality comparison. Implementations should be symmetric and transitive where defined. Equal keys must also have equal hashes when used in a hash collection.",
            "fn same(left: i32, right: i32) -> bool { left == right }",
        ),
        "Eq" => (
            "Marks reflexive equality in addition to `PartialEq`. It has no additional methods. Implementations must satisfy the equality laws relied on by collection algorithms.",
            "fn unique(values: Vec<i32>) -> std::collections::List<i32> { values.distinct() }",
        ),
        "Hash" => (
            "Computes a stable hash for the duration of a collection operation. Equal keys must hash equally. Hash and equality callbacks run synchronously; recursive structural mutation during lookup traps.",
            "fn lookup(values: std::collections::HashMap<i32, i32>) -> Option<i32> { values.get(7) }",
        ),
        "PartialOrd" => (
            "A partial ordering consistent with equality. `partial_cmp` returns `None` for incomparable values; floating-point NaN is incomparable.",
            "fn before(left: i32, right: i32) -> bool { left < right }",
        ),
        "Ord" => (
            "A total ordering consistent with `Eq` and `PartialOrd`. `cmp` returns Less, Equal or Greater. Sorting relies on a consistent comparator.",
            "fn ordered(values: Vec<i32>) -> std::collections::List<i32> { values.sorted() }",
        ),
        "Add" => (
            "Addition with an explicitly declared output type. Built-in integer arithmetic traps on overflow; a user implementation defines its own behavior.",
            "fn plus(left: i32, right: i32) -> i32 { left + right }",
        ),
        "Sub" => (
            "Subtraction with an explicitly declared output type. Built-in integer subtraction traps on overflow.",
            "fn difference(left: i32, right: i32) -> i32 { left - right }",
        ),
        "Mul" => (
            "Multiplication with an explicitly declared output type. Built-in integer multiplication traps on overflow.",
            "fn doubled(value: i32) -> i32 { value * 2 }",
        ),
        "Div" => (
            "Division. Built-in integer division traps for zero divisors and signed overflow; floating-point division follows its specified numeric semantics.",
            "fn half(value: i32) -> i32 { value / 2 }",
        ),
        "Rem" => (
            "The remainder of division. Built-in integer remainder traps for zero divisors and signed overflow.",
            "fn parity(value: i32) -> i32 { value % 2 }",
        ),
        "BitAnd" => (
            "Bitwise conjunction, with an explicitly declared output type.",
            "fn low_bits(value: i32) -> i32 { value & 15 }",
        ),
        "BitOr" => (
            "Bitwise disjunction, with an explicitly declared output type.",
            "fn set_bit(value: i32) -> i32 { value | 1 }",
        ),
        "BitXor" => (
            "Bitwise exclusive disjunction, with an explicitly declared output type.",
            "fn toggle(value: i32) -> i32 { value ^ 1 }",
        ),
        "Shl" => (
            "Left shift. Built-in integer shifts validate the shift count and checked result.",
            "fn shift(value: i32) -> i32 { value << 1 }",
        ),
        "Shr" => (
            "Right shift. Built-in integer shifts validate the shift count.",
            "fn shift(value: i32) -> i32 { value >> 1 }",
        ),
        "Neg" => (
            "Arithmetic negation. Negating the minimum signed integer traps on overflow.",
            "fn negative(value: i32) -> i32 { -value }",
        ),
        "Not" => (
            "Logical or bitwise inversion, depending on the receiver type. The output is declared by the implementation.",
            "fn invert(value: bool) -> bool { !value }",
        ),
        "Index" => (
            "Indexed read access. Built-in sequence indexing traps when the index is out of bounds; `get` provides an optional result instead.",
            "fn head(values: Vec<i32>) -> i32 { values[0] }",
        ),
        "Fn" => (
            "A checked callable signature. Calls evaluate arguments left to right exactly once. Stored callable values retain their selected generation across reload.",
            "fn apply<F: Fn(i32) -> i32>(f: F, value: i32) -> i32 { f(value) }",
        ),
        "RangeBounds" => (
            "Exposes the start and end bounds of a range as Included, Excluded or Unbounded. Constructing bounds does not iterate the range.",
            "fn bounds() -> core::ops::Bound<i32> { (1..4).start_bound() }",
        ),
        "Debug" => (
            "Produces a diagnostic string representation through `debug`. Formatting does not permit reflective access outside declared members.",
            "fn render<T: core::fmt::Debug>(value: T) -> String { value.debug() }",
        ),
        "Display" => (
            "Produces a user-facing string representation through `display`. Implementations choose their formatting policy.",
            "fn render<T: core::fmt::Display>(value: T) -> String { value.display() }",
        ),
        "From" => (
            "Constructs a target value from a source. The conversion is infallible in its declared signature. Result propagation can use `From` to convert an error type.",
            "fn widen(value: i32) -> i64 { i64::from(value) }",
        ),
        "Try" => (
            "Defines the successful Output and distinct Residual of a carrier. Question-mark evaluates the value and branch once: Continue produces Output, while Break passes Residual to the enclosing return type's FromResidual implementation. Traps are not converted to residuals.",
            "fn unwrap(value: Option<i32>) -> Option<i32> { Some(value?) }",
        ),
        "FromResidual" => (
            "Constructs the enclosing return value on early propagation. Residual types distinguish Option, Result and ControlFlow. Result converts only the error through one infallible From implementation, preserving its original failure trace.",
            "fn forward(value: Result<i32, String>) -> Result<i64, String> { Ok(i64::from(value?)) }",
        ),
        "Into" => (
            "Converts the receiver into a target. A matching `From` implementation supplies the reverse adapter without another body.",
            "fn widen(value: i32) -> i64 { value.into() }",
        ),
        "TryFrom" => (
            "A checked conversion returning `Result<Self, Error>`. An unrepresentable numeric value returns an error; it does not silently truncate.",
            "fn narrow(value: i64) -> Result<i32, core::num::TryFromIntError> { i32::try_from(value) }",
        ),
        "TryInto" => (
            "A checked conversion of the receiver into a target. The reverse adapter uses the corresponding `TryFrom` declaration and error type.",
            "fn narrow(value: i64) -> Result<i32, core::num::TryFromIntError> { value.try_into() }",
        ),
        "FromStr" => (
            "Parses a string into a value, returning the declared `Err` on invalid input. Built-in numeric parsing validates the complete input and representable range.",
            "fn parse(text: String) -> Result<i32, core::num::ParseError> { i32::from_str(text) }",
        ),
        "FromIterator" => (
            "Constructs a value by consuming an iterable in order. Each item is read once. A failure retains already completed source and callback effects.",
            "fn collect(values: Vec<i32>) -> Vec<i32> { Vec<i32>::from_iter(values) }",
        ),
        "Sum" => (
            "Aggregates an iterable using addition. Built-in numeric sums use the additive identity for an empty input and checked arithmetic where required.",
            "fn total(values: Vec<i32>) -> i32 { i32::sum(values) }",
        ),
        "Product" => (
            "Aggregates an iterable using multiplication. Built-in numeric products use the multiplicative identity for an empty input and checked arithmetic where required.",
            "fn multiply(values: Vec<i32>) -> i32 { i32::product(values) }",
        ),
        "List" => (
            "A readonly sequence interface. Element types are invariant. A readonly view retains shared identity, so another alias can mutate the underlying sequence.",
            "fn first(values: std::collections::List<i32>) -> Option<i32> { values.get(0) }",
        ),
        "MutableList" => (
            "A mutable sequence interface. Structural edits validate active borrows and iteration guards. Completed writes and callback effects survive later failures.",
            "fn append(values: std::collections::MutableList<i32>) { values.push(3); }",
        ),
        "Map" => (
            "A readonly key/value interface. Lookup returns `None` for a missing key. Readonly views share storage with other aliases.",
            "fn find(values: std::collections::Map<i32, String>) -> Option<String> { values.get(1) }",
        ),
        "MutableMap" => (
            "A mutable key/value interface. Insert replaces the value of an equal key. Hash and equality callbacks must not structurally mutate a collection under lookup.",
            "fn put(values: std::collections::MutableMap<i32, String>) { values.insert(1, \"one\"); }",
        ),
        "Set" => (
            "A readonly membership interface. Equal values represent the same member. Readonly views share storage with other aliases.",
            "fn has(values: std::collections::Set<i32>) -> bool { values.contains(1) }",
        ),
        "MutableSet" => (
            "A mutable membership interface. Insertion retains one representative of each equal value. Structural edits honor active borrow, iteration and lookup guards.",
            "fn add(values: std::collections::MutableSet<i32>) { values.insert(1); }",
        ),
        _ => unreachable!("standard trait documentation inventory: {name}"),
    }
}

fn member_docs(trait_name: &str, name: &str, overview: &str) -> String {
    let behavior = match name {
        "len" => "Return the number of entries.",
        "is_empty" => "Return whether the collection has no entries.",
        "get" => "Return the requested value, or None if the index or key is absent.",
        "contains" | "contains_key" => "Test membership without changing collection storage.",
        "push" => "Append one item. A guarded structural edit traps before committing the edit.",
        "pop" => "Remove and return the last item, or None for an empty sequence.",
        "insert" if trait_name == "MutableList" => {
            "Insert at the given position and shift later elements. An index beyond the length traps."
        }
        "insert" if trait_name == "MutableMap" => {
            "Insert a key/value entry, replacing the value of an equal key."
        }
        "insert" => "Insert a member, preserving the uniqueness of equal members.",
        "remove" if trait_name == "MutableSet" => {
            "Remove a member and report whether it was present."
        }
        "remove" => "Remove and return the entry, or None if it is absent.",
        "set" => "Replace the item at an existing index. An out-of-bounds index traps.",
        "clear" => "Remove all entries after validating structural mutation guards.",
        "start_bound" => "Return the lower bound with its inclusion policy.",
        "end_bound" => "Return the upper bound with its inclusion policy.",
        "branch" => "Consume the carrier once and return Continue(Output) or Break(Residual).",
        "from_output" => "Construct a successful carrier from Output.",
        "from_residual" => "Construct the enclosing early-return value from a compatible Residual.",
        _ => overview,
    };
    format!("{behavior}\n\nSee `{trait_name}` for the behavioral contract and examples.")
}

fn inherent_example(receiver: &Ty, name: &str) -> String {
    let (ty, arguments) = if *receiver == Ty::Builtin(BuiltinType::String) {
        (
            "String",
            match name {
                "contains" | "starts_with" | "ends_with" | "find" | "split" => "\"a\"",
                "slice" => "0, 1",
                "replace" => "\"a\", \"b\"",
                _ => "",
            },
        )
    } else {
        let ty = match receiver {
            Ty::Array(..) => "Vec<i32>",
            Ty::Map { .. } => "std::collections::HashMap<i32, i32>",
            Ty::Set(..) => "std::collections::HashSet<i32>",
            _ => unreachable!("standard inherent documentation receiver"),
        };
        let arguments = match name {
            "insert" | "insert_fluent" | "set" | "set_fluent" => "0, 1",
            "push" | "push_fluent" | "contains" | "contains_key" => "1",
            "get" | "remove" | "index" => "0",
            "sort_by" | "sorted_by" => "|left, right| left.cmp(right)",
            "sort_by_key" | "sorted_by_key" => "|value| value",
            "retain" => "|value| value > 0",
            _ => "",
        };
        (
            ty,
            if matches!(receiver, Ty::Set(..)) && matches!(name, "insert" | "insert_fluent") {
                "1"
            } else {
                arguments
            },
        )
    };
    if name == "new" {
        format!(
            "fn create() -> {ty} {{ {}::new() }}",
            ty.split('<').next().unwrap()
        )
    } else {
        format!("fn example(values: {ty}) {{ val result = values.{name}({arguments}); }}")
    }
}

pub(super) fn complete(module: &mut ModuleDecl) {
    let path = module.identity.path.join("::");
    let overview = match path.as_str() {
        "iter" => {
            "Shared iteration and iterable construction. Iterator values retain progress and obey guarded structural mutation."
        }
        "ops" => {
            "Operator contracts, range values and boundary policies. Operators dispatch through checked implementations and preserve evaluation order."
        }
        "cmp" => {
            "Equality and ordering contracts. Comparisons must obey the laws required by collection algorithms."
        }
        "hash" => {
            "Hash contracts used by key-based collections. Equal keys must hash equally, and key lookup blocks recursive structural mutation."
        }
        "fmt" => "Diagnostic and display formatting through declared interfaces.",
        "future" => {
            "Cold asynchronous operations. Creation captures inputs without starting work; first await drives the operation once. Aliases share that single-drive state."
        }
        "task" => {
            "Scope-owned asynchronous jobs with retained terminal results and host-controlled scheduling."
        }
        "convert" => {
            "Explicit infallible and checked conversions, including reverse adapters and error conversion."
        }
        "num" => {
            "Numeric parsing and aggregation. Numeric failures use checked traps or the declared Result error."
        }
        "option" => {
            "Optional values: Some contains a value; None represents absence. Question-mark propagation preserves the enclosing Option result."
        }
        "result" => {
            "Success and failure values. Question-mark propagation returns an error early and uses a declared From conversion where needed."
        }
        "vec" => {
            "Shared mutable sequences with checked indexing, iteration and collection algorithms. Readonly views do not freeze aliases."
        }
        "str" => {
            "String parsing contracts with an explicit error type and complete input validation."
        }
        "string" => {
            "Immutable UTF-8 strings. Length and offsets count bytes; slicing validates UTF-8 boundaries."
        }
        "collections" => {
            "Sequence, map and set interfaces, concrete hash collections and lazy mapping. Callbacks run synchronously with explicit borrow, root and mutation guards."
        }
        "prelude" => {
            "Automatically imported standard names. Re-exports retain the canonical declaration identity and documentation of their owner."
        }
        _ => unreachable!("standard module documentation inventory: {path}"),
    };
    let code = match path.as_str() {
        "iter" => trait_docs("Iterable").1,
        "ops" => trait_docs("Add").1,
        "cmp" => trait_docs("PartialEq").1,
        "hash" => trait_docs("Hash").1,
        "fmt" => trait_docs("Debug").1,
        "future" => "fn retain(value: Future<i32>) -> core::future::Future<i32> { value }",
        "task" => "fn retain(value: Task<i32>) -> core::task::Task<i32> { value }",
        "convert" => trait_docs("From").1,
        "num" | "str" => trait_docs("FromStr").1,
        "string" => "fn clean(text: String) -> String { text.trim() }",
        "option" => "fn first(values: Vec<i32>) -> Option<i32> { values.get(0) }",
        "result" => trait_docs("FromStr").1,
        _ => "fn length(values: Vec<i32>) -> usize { values.len() }",
    };
    module.module_documentation = example(overview, code);
    for contract in &module.traits {
        let id = module.definition(DefinitionKind::Trait, &contract.name);
        let (overview, code) = trait_docs(&contract.name);
        module
            .documentation
            .insert(id.clone(), example(overview, code));
        for method in &contract.methods {
            module
                .documentation
                .entry(ModuleDecl::method_id(&id, &method.name))
                .or_insert_with(|| {
                    example(&member_docs(&contract.name, &method.name, overview), code)
                });
        }
        for member in &contract.associated_types {
            let name = &member
                .declaration
                .path
                .last()
                .expect("associated type")
                .name;
            let text = match name.as_str() {
                "Item" => "The item yielded by this iterator or iterable.",
                "Iter" => "The iterator produced by iter; its Item equals this iterable's Item.",
                "Output" => "The result type of the declared operation.",
                "Error" | "Err" => "The error returned when this conversion or parse fails.",
                "Residual" => {
                    "The distinct early-return type passed to FromResidual; successful Output values are not residuals."
                }
                _ => unreachable!("standard associated type documentation: {name}"),
            };
            module
                .documentation
                .insert(member.declaration.clone(), text.into());
        }
    }
    for ty in &module.types {
        let kind = match ty.kind {
            TypeDefKind::Native(constructor) => constructor.declaration_kind(),
            _ => DefinitionKind::AssociatedType,
        };
        let id = module.definition(kind, &ty.name);
        let (description, code) = match ty.name.as_str() {
            "Task" => (
                "A scope-owned execution handle. Dropping a handle does not cancel the task. Completed values are retained by live handles and pending host reports.",
                "fn retain(value: Task<i32>) -> core::task::Task<i32> { value }",
            ),
            "TaskScope" => (
                "A host-created capability for admitting independent tasks into one host-owned scope. Only the host owner controls driving and scope closure.",
                "fn retain(value: TaskScope) -> core::task::TaskScope { value }",
            ),
            "Future" => (
                "A cold runtime-local operation with one invariant result type. Creation does not submit work. Await drives once and returns the result; repeated awaiting through any alias traps. Cancellation terminates execution rather than wrapping the result in Result.",
                "fn retain(value: Future<i32>) -> core::future::Future<i32> { value }",
            ),
            "Vec" => (
                "A shared mutable sequence. Copies retain the same storage identity. Indexing traps out of bounds; get returns None. Structural mutation honors active borrows and iteration guards.",
                "fn append(values: Vec<i32>) -> usize { values.push(3); values.len() }",
            ),
            "String" => (
                "An immutable UTF-8 string. len counts bytes and slice requires valid byte boundaries. Transformations produce strings without changing the receiver.",
                "fn greeting() -> String { \"  hello  \".trim() }",
            ),
            "HashMap" => (
                "A shared hash map with invariant key and value types. Equal keys must hash equally. Insert replaces an existing value; get returns None for an absent key.",
                "fn find(values: std::collections::HashMap<i32, String>) -> Option<String> { values.get(1) }",
            ),
            "HashSet" => (
                "A shared hash set. Equal members occupy one entry, and must hash equally. Lookup and structural mutation use checked synchronous guards.",
                "fn has(values: std::collections::HashSet<i32>) -> bool { values.contains(1) }",
            ),
            "CollectionCursor" => (
                "A shared built-in cursor. Copies share progress. next returns None when exhausted; active iteration guards its source against structural edits.",
                "fn first(values: Vec<i32>) -> Option<i32> { values.iter().next() }",
            ),
            "Option" => (
                "An optional value: Some carries one value and None carries none. Question-mark propagation returns None early.",
                "fn first(values: Vec<i32>) -> Option<i32> { values.get(0) }",
            ),
            "ControlFlow" => (
                "Continue contains a successful value; Break contains an early-exit value. Try uses ControlFlow<B, Infallible> as its distinct residual, preserving the break value without conversion.",
                "fn success() -> core::ops::ControlFlow<String, i32> { core::ops::ControlFlow::Continue(42) }",
            ),
            "Result" => (
                "A checked outcome: Ok carries the successful value and Err carries the error. Question-mark propagation returns a converted error early.",
                "fn read(text: String) -> Result<i32, core::num::ParseError> { i32::from_str(text) }",
            ),
            "Ordering" => (
                "The result of a total comparison: Less, Equal or Greater.",
                "fn compare(left: i32, right: i32) -> core::cmp::Ordering { left.cmp(right) }",
            ),
            "Bound" => (
                "A range endpoint: Included(value), Excluded(value) or Unbounded.",
                "fn lower() -> core::ops::Bound<i32> { (1..4).start_bound() }",
            ),
            "TryFromIntError" => (
                "A checked numeric conversion error. Its variants distinguish the declared failure conditions.",
                "fn narrow(value: i64) -> Result<i32, core::num::TryFromIntError> { i32::try_from(value) }",
            ),
            "Infallible" => (
                "An uninhabited error type for conversions that cannot fail. It has no constructible variants.",
                "fn widen(value: i32) -> i64 { i64::from(value) }",
            ),
            "ParseError" => (
                "A numeric parsing error. Invalid input and out-of-range values are reported through Result.",
                "fn number(text: String) -> Result<i32, core::num::ParseError> { i32::from_str(text) }",
            ),
            name if name.starts_with("Range") => (
                "A range value carrying its declared endpoints and inclusion policy. Finite integer ranges implement Iterable; open bounds are available through RangeBounds.",
                "fn lower() -> core::ops::Bound<i32> { (1..4).start_bound() }",
            ),
            name => unreachable!("standard type documentation inventory: {name}"),
        };
        module
            .documentation
            .insert(id.clone(), example(description, code));
        for variant in &ty.variants {
            let mut member = id.clone();
            member.path.push(DefinitionPathSegment {
                kind: DefinitionKind::Variant,
                name: variant.name.clone(),
                occurrence: 0,
            });
            module.documentation.insert(
                member,
                format!(
                    "The `{}` case of `{}`. {description}",
                    variant.name, ty.name
                ),
            );
        }
    }
    for (index, implementation) in module.implementations.iter().enumerate() {
        let owner = module.implementation_id(index);
        for method in &implementation.methods {
            let id = ModuleDecl::method_id(&owner, &method.name);
            if let Some(interface) = &implementation.trait_type {
                let source = ModuleDecl::method_id(&interface.declaration, &method.name);
                // Cross-module trait documentation is attached after all modules are complete.
                if let Some(text) = module.documentation.get(&source).cloned() {
                    module.documentation.entry(id).or_insert(text);
                }
            } else {
                let text = module.documentation.entry(id).or_insert_with(|| member_docs("collection", &method.name, "Creates or accesses checked shared collection storage. Structural mutation honors active borrow and iteration guards."));
                if !text.contains("```kgr") {
                    *text = example(
                        text,
                        &inherent_example(&implementation.for_type, &method.name),
                    );
                }
            }
        }
    }
}

pub(super) fn inherit(modules: &mut [ModuleDecl]) {
    let documents = modules
        .iter()
        .flat_map(|module| &module.documentation)
        .map(|(id, text)| (id.clone(), text.clone()))
        .collect::<BTreeMap<_, _>>();
    for module in modules {
        for (index, implementation) in module.implementations.iter().enumerate() {
            let Some(interface) = &implementation.trait_type else {
                continue;
            };
            let owner = module.implementation_id(index);
            for method in &implementation.methods {
                let source = ModuleDecl::method_id(&interface.declaration, &method.name);
                if let Some(text) = documents.get(&source) {
                    module
                        .documentation
                        .entry(ModuleDecl::method_id(&owner, &method.name))
                        .or_insert_with(|| text.clone());
                }
            }
        }
    }
}
