# Native declarations and library modules

Kagari declarations describe Kagari types, functions, traits and implementations.
Rust entries implement those declarations. Registration is explicit: a Rust
function's signature does not define a Kagari API, and no declaration attribute
macro or compiled standard-library binary is involved.

## Ownership and installation

The compiler-owned `core::language` catalog contains complete language contracts:
operators, comparisons, hashing, Index, Fn, iteration, collection interfaces,
formatting and error conversion. It also declares Option/Result/Ordering/Bound,
range forms and the default ArrayList/HashMap/HashSet types. Their basic native
operations are always available. Array literals construct ArrayList.

Map/Set interfaces do not prescribe storage or traversal order. Default HashMap
and HashSet use Rust `std::collections` hash tables and require Eq + Hash keys.
They promise neither insertion nor sorted order. LinkedHashMap/LinkedHashSet and
other additional containers belong to optional modules; none are installed by
the current bounded library proof.

Optional native modules use the same registration mechanism as application-owned
modules. `KagariEngine::builder().default_modules(false).build()` disables bundled
algorithms while preserving language contracts and default collections. An engine
builder's `install(module)` adds an application module. A finished module contains
both the portable declarations and runtime-local Rust entries/storage descriptors.

Currently the bundled `std::collections` module provides these ordinary functions:

```kagari
use std::collections;
fn main() -> i32 {
    val values = [22, 20];
    collections::sort(values);
    val mapped = collections::map(values, |value| value + 1);
    match mapped.next() { Some(value) => value + values[1], None => 0 }
}
```

`sort<T: Ord>` and `sort_by<T>` stably reorder an ArrayList in place. `sort_by`
accepts `fn(T, T) -> Ordering`. `map<T, U>` accepts an ArrayList and `fn(T) -> U`,
returning the library-owned `MapIterator<T, U>`. These are free functions; optional
algorithms are not methods silently added to language collection traits.

The predecessor string, parse, enum-combinator, iterator-terminal, snapshot,
window, grouping and set-algebra APIs are withdrawn. They are not compatibility
requirements. New libraries may implement such APIs through ordinary registrations.

## Explicit registration

`ModuleBuilder` owns a module's declarations. `FunctionBuilder` introduces fresh
Kagari generic parameters, parameter/result types, bounds and selected callable
requirements. `TypeBuilder` declares a nominal native type and its storage.
`implement` introduces scoped implementation parameters; `inherent_impl` and
`trait_impl` declare the appropriate method groups. Native implementations
reference the compiler's complete trait contracts instead of redeclaring them.

For example, a scalar application entry can be declared and bound as follows:

```rust
use kagari_runtime::native::{
    binding::NativeResult,
    builder::ModuleBuilder,
    context::CallContext,
    declarations::FunctionDecl,
    language::LanguageContracts,
    module::NativeModule,
    types::Type,
};

fn module() -> NativeResult<NativeModule> {
    let language = LanguageContracts::default();
    let mut module = ModuleBuilder::new("demo::numbers", &language);
    let maximum = module.define_function(FunctionDecl::new("maximum"))?;
    module.function(&maximum, |function| {
        function.parameter("left", Type::i32());
        function.parameter("right", Type::i32());
        function.returns(Type::i32());
        Ok(())
    })?;
    module.bind(maximum, |_cx: &mut CallContext<'_>, left: i32, right: i32| -> NativeResult<i32> {
        Ok(left.max(right))
    })?;
    module.finish()
}
```

`bind` checks supported argument/result codecs against the declared Kagari
signature. `bind_with` accepts explicit codecs for cases requiring additional
control. Both produce the same prepared native binding; neither infers Kagari
traits, impls or generic constraints from Rust definitions.

Registration checks ownership, generic scope, complete trait method coverage,
substituted signatures and storage compatibility before publishing a module.
Unbound functions, foreign declaration handles and incompatible Rust conversions
are rejected. The application fixture at
`crates/kagari-embed/tests/support/native_provider.rs` demonstrates a generic
factory and a non-sequence object with an inherent method and retained callback.

## Checked executable contracts

Source analysis consumes registered declaration records directly. Lowering carries
provider-qualified native imports with concrete types, signatures, bounds and
selected callable dependencies. Portable validation proves those facts against
the executable dependency closure. Artifact loading checks the installed entry
against the complete carried declaration; matching names alone are insufficient.

An ordinary generic bound states applicability. A selected callable requirement
additionally states that the Rust body invokes a particular trait member. Linking
resolves that member once for the concrete native application. `cx.selected(slot)`
borrows the prepared callable; invocation does not repeat source lookup or trait
resolution. Implicit language operations and explicit script implementations
retain distinct checked targets.

Runtime bindings and Rust storage factories are local installation data, not
serialized function pointers. Artifacts retain portable identities and contracts.
Executable loading works without HIR, source parsing or generated declaration text.
Retained callables pin their defining code generation across reload.

## Calls, roots and storage

Ordinary native functions and their script callbacks execute synchronously.
`CallContext` exposes checked arguments, selected calls, allocation and execution
checks. `CallableHandle` borrows a callback during its invocation. A payload that
retains a callback uses `StoredCallable` and traces it. Returned script heap values
must be rooted before further allocation or reentry; callback argument packs do
not create a script tuple merely to carry Rust arguments.

Scoped sequence views borrow rooted existing storage. Primitive layouts use
contiguous typed buffers, including empty arrays whose layout is determined by
the declared element type. Heap-reference elements use traced Values. Bulk slice
access does not copy the entire array or allocate a per-call scratch buffer.
Mutable access respects alias/iteration guards and storage revisions.

`SequenceEdit` provides an isolated working copy when a fallible operation needs
atomic publication. Traced values may be reordered only by a validated bijection;
primitive working slices may be mutated. Commit validates the original object's
identity and revision. Failure leaves original slots unchanged while preserving
completed effects on objects referenced by those slots.

`NativePayload` supplies tracing, logical units and ordinary Rust destruction.
Every retained script Value and stored callable must be traced. `NativeStorage`
registers the payload layout and either a checked factory or an explicitly
constructed payload. New opaque types use the generic NativeObject representation;
they do not require new compiler, ABI, verifier or VM type variants.

## Sorting and lazy iteration

Primitive infallible ordering sorts the borrowed Rust slice directly. Script Ord
and supplied comparators run synchronously over an isolated permutation and
publish once on success. A failed comparison prevents further user comparisons.
Rust's infallible sort may finish internal bookkeeping, but no partial order is
committed. Comparisons have Rust stable-sort ordering, not a fixed call schedule.

Only genuinely retained computations need state. MapIterator retains a
`NativeCursor`, a stored mapper and a reentry guard. Construction performs no map
calls. Aliases share progress. Its next consumes one source item before invoking
the mapper; a mapper failure does not cause that item to be processed again.
The public Option is allocated once for a successful next result.

A payload's `iteration_sources` exposes managed cursor dependencies. Those are also
GC edges. A `for` scope walks this graph, protects source collection structure and
releases its guards on break, return, failure or cancellation. Dropping a scope
permits later mutation while a retained cursor may resume if its source remains
valid. Reentrant next on the same adapter is rejected. The generic execution loop
does not identify an adapter by its library name.

## Tooling

Generated `.kgr` is a read-only projection for navigation, completion, signatures
and documentation. It is not executable source, a second signature authority or
an installation trigger. Declaration locations and docs come from the same
records used by compilation. The generated views include library-owned types,
inherent methods and trait impl methods, including MapIterator.next.

Behavioral and tooling coverage lives in native_provider_reset,
VM library_collections/library_mapping and the independent artifact_features
consumer. The [active plan](../native-provider-refactor.md) records measured costs,
source-free feature checks, deferred APIs and final integration status.
