//! Iteration API prose and examples, shared by native registration contributors.
use crate::foundation::iteration::cursor::Operation;

pub(in crate::foundation) const MODULE: (&str, &str) = (
    r#"Iteration over collections and user-defined sequences.

`Iterable::iter` obtains a cursor; `Iterator::next` advances it. Collections
normally create a fresh cursor on each `iter` call. Calling `iter` on an existing
iterator preserves that cursor and its progress. Assigning a cursor to another
variable also shares its progress.

Adapters such as `map`, `filter` and `take` are lazy: construction obtains the
source cursor but does not read items or invoke the item callback. Methods such
as `count`, `find` and `fold` start consuming immediately. A consumer stops at the
first `None`; a custom iterator may produce more items on a later `next` call.

Built-in collection cursors reject reads after structural source mutation.
Completed cursor advances, callback effects and source edits are not rolled back
when a later operation traps or is cancelled.

Lazy adapters retain the source operations and callbacks selected when created.
Hot reload does not reset their progress or replace those retained selections.
Ordinary calls made inside those operations still follow normal reload rules."#,
    r#"fn main() -> bool {
    val values = Vec::from([1, -2, 3]);
    val result: i32 = values.filter(|item| item > 0).map(|item| item * 2).sum();
    result == 8
}"#,
);

pub(in crate::foundation) const ITERATOR: (&str, &str) = (
    r#"A cursor that yields items one at a time.

Implement `next` to return `Some(item)` for an item or `None` to end the current
consumption. Returning `None` does not require a custom implementation to remain
exhausted forever. Consumers stop at the first `None` unless they document a
different rule.

Cursor values share progress when copied. They also implement `Iterable`, so
lazy adapters and terminal methods can continue from their current position."#,
    r#"fn main() -> bool {
    val cursor = Vec::from([10, 20]).iter();
    val alias = cursor;
    cursor.next() == Some(10) && alias.next() == Some(20) && cursor.next() == None
}"#,
);

pub(in crate::foundation) const ITERABLE: (&str, &str) = (
    r#"A value that can provide an iterator over its items.

`Item` is the yielded element type and `Iter` is the cursor returned by `iter`.
Collection implementations normally return a fresh cursor. An existing iterator
returns itself and shares its current progress.

The provided adapters obtain source cursors once when constructed. They do not
read an item until advanced. Terminal methods start consumption when called;
short-circuiting methods can leave items available through an existing cursor.
See each method for its stopping rule."#,
    r#"fn main() -> bool {
    val values = Vec::from([1, 2, 3]);
    val cursor = values.iter();
    cursor.next();
    cursor.count() == 2u64 && values.count() == 3u64
}"#,
);

pub(in crate::foundation) const FROM_ITERATOR: (&str, &str) = (
    r#"Constructs a value from an iterable source.

Implement `from_iter` for the destination type. `Iterable::collect` delegates to
that implementation, so the destination determines construction and stopping
behavior. The standard `Vec` implementation collects items in source order until
the first `None`. Already completed source effects survive a construction trap."#,
    r#"fn main() -> bool {
    val result = Vec<i32>::from_iter(Vec::from([1, 2, 3]).map(|item| item * 2));
    result.len() == 3u64 && result.get(1u64) == Some(4)
}"#,
);

pub(in crate::foundation) const SUM: (&str, &str) = (
    r#"Constructs a sum from an iterable source.

`Iterable::sum` delegates to this implementation. Built-in numeric implementations
add items in source order and return zero for an empty input. Integer addition
is checked and traps on overflow; floating-point addition follows the declared
floating-point type's arithmetic. Custom implementations define their own rules."#,
    r#"fn main() -> bool {
    i32::sum(Vec::from([2, 3, 4])) == 9
}"#,
);

pub(in crate::foundation) const PRODUCT: (&str, &str) = (
    r#"Constructs a product from an iterable source.

`Iterable::product` delegates to this implementation. Built-in numeric
implementations multiply items in source order and return one for an empty input.
Integer multiplication is checked and traps on overflow; floating-point
multiplication follows the declared type's arithmetic. Custom implementations
define their own rules."#,
    r#"fn main() -> bool {
    i32::product(Vec::from([2, 3, 4])) == 24
}"#,
);

pub(super) const CURSOR: (&str, &str) = (
    r#"A shared iterator over a built-in collection.

Each `next` reads the next item and advances the cursor. Copies of the cursor
share that position. Keeping a cursor does not prevent editing its collection.

# Traps

Reading after a structural edit of the source traps. The edit itself remains
committed. Exhaustion returns `None`; it is not a trap."#,
    ITERATOR.1,
);

pub(super) fn adapter_type(operation: Operation) -> (&'static str, &'static str) {
    match operation {
        Operation::Map => (
            "An iterator that transforms source items with a callback.\n\nCreated by `Iterable::map`. `T1` is the source item type and `T2` is the\ncallback result type. The callback runs when an item is requested. Copies of\nthis adapter share source progress. See `Iterable::map` for an example.",
            MAP.1,
        ),
        Operation::Filter => (
            "An iterator that yields source items accepted by a predicate.\n\nCreated by `Iterable::filter`. One `next` may inspect several items before\nfinding a match. Copies share progress. See `Iterable::filter` for an example.",
            FILTER.1,
        ),
        Operation::Take => (
            "An iterator that yields at most a specified number of items.\n\nCreated by `Iterable::take`. Copies share the remaining limit and source\nprogress. See `Iterable::take` for its stopping rule and an example.",
            TAKE.1,
        ),
        Operation::FilterMap => (
            "An iterator that yields the `Some` results of a callback.\n\nCreated by `Iterable::filter_map`. `T1` is the source item type and `T2` is the\nunwrapped callback result type. Copies share progress. See `Iterable::filter_map`\nfor an example and the distinction between callback and source exhaustion.",
            FILTER_MAP.1,
        ),
        Operation::Skip => (
            "An iterator that discards an initial number of source items.\n\nCreated by `Iterable::skip`. Discarding happens on advancement, not during\nconstruction. Copies share the remaining skip count and source progress.",
            SKIP.1,
        ),
        Operation::Enumerate => (
            "An iterator that pairs source items with zero-based `u64` positions.\n\nCreated by `Iterable::enumerate`. Copies share the next position and source\nprogress. See that method for the checked counter's overflow behavior.",
            ENUMERATE.1,
        ),
        Operation::Zip => (
            "An iterator that pairs items from two sources.\n\nCreated by `Iterable::zip`. Each advance reads the left source before the right.\nCopies share both cursors. See `Iterable::zip` for unequal-length behavior.",
            ZIP.1,
        ),
        Operation::Chain => (
            "An iterator that visits one source and then another.\n\nCreated by `Iterable::chain`. It switches permanently to the second cursor when\nthe first returns `None`. Copies share both progress and the switch state.",
            CHAIN.1,
        ),
    }
}

pub(in crate::foundation) const NEXT: (&str, &str) = (
    r#"Advances the cursor and returns its next item.

Returns `Some(item)` when an item is available, or `None` to end the current
consumption. A custom cursor may resume after `None`. Copies of the cursor
observe the same progress.

# Traps

Built-in collection cursors trap after structural source mutation. Lazy adapters
also trap on recursive advancement of the same adapter. Source and callback
traps propagate; previously completed effects remain committed."#,
    ITERATOR.1,
);

pub(in crate::foundation) const ITER: (&str, &str) = (
    r#"Returns a cursor over this value's items.

The cursor's `Item` type equals this iterable's `Item`. Collection implementations
normally return a fresh cursor at the beginning. Calling `iter` on an iterator
returns that iterator with its current progress; it does not rewind or clone it.
Custom implementations choose how to obtain their cursor."#,
    ITERABLE.1,
);

const ADAPTER_NEXT: &str = r#"Advances this lazy adapter and returns its next output item.

Reads the retained source cursors and runs callbacks according to the adapter's
creation method. One output may require several source items. Copies share
progress and adapter state. The source is not automatically made permanently
exhausted after `None`; `chain` and `take` have their own stopping rules.

# Traps

Recursive `next` on the same adapter traps. Source, callback and checked-counter
traps propagate without rolling back completed effects."#;

pub(super) fn adapter_next(operation: Operation) -> (&'static str, &'static str) {
    let example = match operation {
        Operation::Map => {
            "fn main() -> bool {\n    Vec::from([1, 2]).map(|item| item * 2).next() == Some(2)\n}"
        }
        Operation::Filter => {
            "fn main() -> bool {\n    Vec::from([1, 2, 3]).filter(|item| item > 1).next() == Some(2)\n}"
        }
        Operation::Take => {
            "fn main() -> bool {\n    val cursor = Vec::from([1, 2]).iter();\n    cursor.take(0u64).next() == None && cursor.next() == Some(1)\n}"
        }
        Operation::FilterMap => {
            "fn main() -> bool {\n    Vec::from([1, 2]).filter_map(|item| {\n        if item > 1 { Some(item * 10) } else { None }\n    }).next() == Some(20)\n}"
        }
        Operation::Skip => {
            "fn main() -> bool {\n    Vec::from([1, 2, 3]).skip(2u64).next() == Some(3)\n}"
        }
        Operation::Enumerate => {
            "fn main() -> bool {\n    Vec::from([10, 20]).enumerate().next() == Some((0u64, 10))\n}"
        }
        Operation::Zip => {
            "fn main() -> bool {\n    Vec::from([1, 2]).zip(Vec::from([10, 20])).next() == Some((1, 10))\n}"
        }
        Operation::Chain => {
            "fn main() -> bool {\n    val empty: Vec<i32> = Vec::new();\n    empty.chain(Vec::from([3, 4])).next() == Some(3)\n}"
        }
    };
    (ADAPTER_NEXT, example)
}

pub(super) const MAP: (&str, &str) = (
    r#"Returns a lazy iterator that transforms each item with `callback`.

Construction obtains the source cursor once without reading items or calling
`callback`. Each advance reads one item, invokes `callback(item)` once and yields
its result. Source order is preserved. Copies share progress.

# Traps

Source or callback traps propagate. The source item has already been consumed
if its callback traps; earlier effects are retained."#,
    r#"fn main() -> bool {
    val result: Vec<i32> = Vec::from([1, 2, 3]).map(|item| item * 2).collect();
    result.len() == 3u64 && result.get(0u64) == Some(2) && result.get(2u64) == Some(6)
}"#,
);

pub(super) const FILTER: (&str, &str) = (
    r#"Returns a lazy iterator over items for which `callback` returns `true`.

The predicate receives each inspected item once, in source order. Rejected items
are consumed. One output may therefore require several source advances. Creating
the adapter does not inspect any item or call the predicate.

# Traps

Source and predicate traps propagate without undoing completed effects."#,
    r#"fn main() -> bool {
    val result: Vec<i32> = Vec::from([1, 2, 3, 4]).filter(|item| item % 2 == 0).collect();
    result.len() == 2u64 && result.get(0u64) == Some(2) && result.get(1u64) == Some(4)
}"#,
);

pub(super) const TAKE: (&str, &str) = (
    r#"Returns a lazy iterator that yields at most `limit` items.

After yielding `limit` items it returns `None` without advancing the source.
A zero limit never advances the source. Construction still obtains its cursor.
A source `None` does not reduce the remaining limit, so a custom source may
resume on a later call. Copies share the limit and source progress."#,
    r#"fn main() -> bool {
    val cursor = Vec::from([1, 2, 3]).iter();
    val prefix: Vec<i32> = cursor.take(2u64).collect();
    prefix.len() == 2u64 && prefix.get(1u64) == Some(2) && cursor.next() == Some(3)
}"#,
);

pub(super) const FILTER_MAP: (&str, &str) = (
    r#"Returns a lazy iterator over the `Some` results of `callback`.

For each source item, calls `callback(item)` once. `Some(value)` yields `value`;
callback `None` skips that item and continues searching. A source `None` ends the
current advance instead. It does not permanently exhaust a resumable custom
source. Creating the adapter runs no item callback.

# Traps

Source and callback traps propagate without undoing completed effects."#,
    r#"fn main() -> bool {
    val result: Vec<i32> = Vec::from([1, 2, 3]).filter_map(|item| {
        if item > 1 { Some(item * 10) } else { None }
    }).collect();
    result.len() == 2u64 && result.get(0u64) == Some(20) && result.get(1u64) == Some(30)
}"#,
);

pub(super) const SKIP: (&str, &str) = (
    r#"Returns a lazy iterator that discards the first `count` items.

Discarding starts on advancement. Once the requested number has been discarded,
remaining items are yielded unchanged. A zero count skips nothing. If a custom
source returns `None` before the count is reached, later advances retain the
remaining skip count. Copies share that count and source progress."#,
    r#"fn main() -> bool {
    val result: Vec<i32> = Vec::from([1, 2, 3, 4]).skip(2u64).collect();
    result.len() == 2u64 && result.get(0u64) == Some(3)
}"#,
);

pub(super) const ENUMERATE: (&str, &str) = (
    r#"Returns a lazy iterator of `(position, item)` pairs, starting at zero.

The position is a `u64`. Each yielded item advances the shared counter once;
copies share both the counter and source progress.

# Traps

Advancing the counter past `u64`'s maximum traps. The source item has already
been consumed when this overflow is detected."#,
    r#"fn main() -> bool {
    val result: Vec<(u64, i32)> = Vec::from([10, 20]).enumerate().collect();
    result.get(0u64) == Some((0u64, 10)) && result.get(1u64) == Some((1u64, 20))
}"#,
);

pub(super) const ZIP: (&str, &str) = (
    r#"Returns a lazy iterator that pairs this source with `other`.

Construction obtains both cursors without reading their items. Each advance
reads the left cursor first, then the right. If the left returns `None`, the
right is not advanced. If only the right returns `None`, the left item is still
consumed. A `None` result does not permanently exhaust resumable custom sources.
Copies share both cursors."#,
    r#"fn main() -> bool {
    val result: Vec<(i32, i32)> = Vec::from([1, 2, 3]).zip(Vec::from([10, 20])).collect();
    result.len() == 2u64 && result.get(0u64) == Some((1, 10)) && result.get(1u64) == Some((2, 20))
}"#,
);

pub(super) const CHAIN: (&str, &str) = (
    r#"Returns a lazy iterator over this source followed by `other`.

Construction obtains both cursors without reading items. The first `None` from
the left cursor switches permanently to the right, including within the same
advance. The left cursor is never read again even if it could later resume.
Copies share the cursors and switch state. Both sources must yield the same
item type."#,
    r#"fn main() -> bool {
    val result: Vec<i32> = Vec::from([1, 2]).chain(Vec::from([3, 4])).collect();
    result.len() == 4u64 && result.get(0u64) == Some(1) && result.get(3u64) == Some(4)
}"#,
);

pub(super) const COLLECT: (&str, &str) = (
    r#"Constructs a destination value from this iterable.

Delegates to the destination's `FromIterator::from_iter` implementation. The
destination type is selected by the expected result type or an explicit type
argument. Its implementation determines how items are consumed and stored.
Standard `Vec` collection preserves source order and stops at the first `None`.

# Traps

Source or destination construction traps propagate. Completed source and
callback effects remain committed."#,
    MAP.1,
);

pub(super) const SUM_ITEMS: (&str, &str) = (
    r#"Sums this iterable using the result type's `Sum` implementation.

The expected result type selects the implementation. Built-in numeric sums
consume in source order until the first `None` and return zero for empty input.
Custom implementations determine their own arithmetic and stopping rules.

# Traps

Built-in integer sums trap on overflow. Source traps propagate and completed
effects remain committed."#,
    r#"fn main() -> bool {
    val result: i32 = Vec::from([2, 3, 4]).sum();
    val empty: Vec<i32> = Vec::new();
    val identity: i32 = empty.sum();
    result == 9 && identity == 0
}"#,
);

pub(super) const PRODUCT_ITEMS: (&str, &str) = (
    r#"Multiplies this iterable using the result type's `Product` implementation.

The expected result type selects the implementation. Built-in numeric products
consume in source order until the first `None` and return one for empty input.
They continue reading after a zero result. Custom implementations choose their
own arithmetic and stopping rules.

# Traps

Built-in integer products trap on overflow. Source traps propagate and completed
effects remain committed."#,
    r#"fn main() -> bool {
    val result: i32 = Vec::from([2, 3, 4]).product();
    val empty: Vec<i32> = Vec::new();
    val identity: i32 = empty.product();
    result == 24 && identity == 1
}"#,
);

pub(super) const FOLD: (&str, &str) = (
    r#"Combines items into an accumulator, starting with `initial`.

Calls `callback(accumulator, item)` once per item in source order and uses each
result as the next accumulator. Stops at the first `None` and returns the final
accumulator. Empty input returns `initial` without calling the callback.

# Traps

Source and callback traps propagate. Cursor advances and callback effects
completed before the failure remain committed."#,
    r#"fn main() -> bool {
    Vec::from([1, 2, 3]).fold(10, |total, item| total + item) == 16
}"#,
);

pub(super) const COUNT: (&str, &str) = (
    r#"Consumes the remaining items and returns their count as a `u64`.

Obtains a cursor once and advances it until the first `None`. Each `Some` adds
one to the result. Empty input returns zero. Calling this on an existing cursor
counts only its remaining items; other aliases observe the same advancement.
An input that never returns `None` does not terminate normally.

# Traps

Traps if the count would exceed `u64`'s maximum or a source advance traps.
Already completed advances and source effects remain committed."#,
    r#"fn main() -> bool {
    val cursor = Vec::from([10, 20, 30]).iter();
    cursor.next();
    cursor.count() == 2u64 && cursor.next() == None
}"#,
);

pub(super) const FIND: (&str, &str) = (
    r#"Returns the first item for which `callback` returns `true`.

Inspects items in source order and calls `callback(item)` once per inspected
item. Stops immediately after the first match and returns `Some(item)`. The
matching item is consumed too; later items remain available through an existing
cursor. Returns `None` at the first source `None`, including for empty input.

# Traps

Source and predicate traps propagate. The inspected item has already been
consumed if its predicate traps; completed effects are not rolled back."#,
    r#"fn main() -> bool {
    val cursor = Vec::from([1, 2, 3]).iter();
    val found = cursor.find(|item| item % 2 == 0);
    found == Some(2) && cursor.next() == Some(3)
}"#,
);

pub(super) const ANY: (&str, &str) = (
    r#"Returns whether any item makes `callback` return `true`.

Tests items once each in source order. Returns `true` immediately after the
first successful test. Returns `false` at the first source `None`; empty input
therefore returns `false` without calling the predicate. The decisive item is
consumed, while later items remain available through an existing cursor.

# Traps

Source and predicate traps propagate without undoing completed effects."#,
    r#"fn main() -> bool {
    val cursor = Vec::from([1, 2, 3]).iter();
    val found = cursor.any(|item| item == 2);
    found && cursor.next() == Some(3)
}"#,
);

pub(super) const ALL: (&str, &str) = (
    r#"Returns whether every inspected item makes `callback` return `true`.

Tests items once each in source order. Returns `false` immediately after the
first failed test. Returns `true` at the first source `None`; empty input
therefore returns `true` without calling the predicate. The decisive item is
consumed, while later items remain available through an existing cursor.

# Traps

Source and predicate traps propagate without undoing completed effects."#,
    r#"fn main() -> bool {
    val cursor = Vec::from([1, 0, 3]).iter();
    val positive = cursor.all(|item| item > 0);
    !positive && cursor.next() == Some(3)
}"#,
);

pub(super) const FOR_EACH: (&str, &str) = (
    r#"Calls `callback` once for each item, in source order.

Consumes until the first source `None` and returns unit. Empty input does not
call the callback. Use this method for effects rather than to build a new value.

# Traps

Source and callback traps stop consumption. Earlier cursor advances and callback
effects remain committed; the failing item's advance is not rolled back."#,
    r#"fn main() -> bool {
    val output: Vec<i32> = Vec::new();
    Vec::from([1, 2, 3]).for_each(|item| { output.push(item * 2); });
    output.len() == 3u64 && output.get(0u64) == Some(2) && output.get(2u64) == Some(6)
}"#,
);
