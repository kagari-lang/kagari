# Values and Expression Evaluation

This is the authoritative v1 contract for observable value behavior. Physical
allocation, Rust `Clone`/`PartialEq`, and backend choices do not define semantics.

## Values and identity

Scalars, strings, tuples, and enums (including Option and Result) have value
semantics. Tuple and enum copies preserve the semantics of each member; copying
a member that refers to a mutable object does not copy that object. Enum payloads
cannot be reassigned in place.

Structs, arrays, maps, and sets are mutable identity-bearing objects. Assignment,
argument passing, and returns share their identity. `val` prevents rebinding a
slot, not mutation of the referenced object. Container copy operations are
shallow. There is no generic deep-copy or deep-freeze operation in v1.

Equality compares scalars and strings by value, tuples by corresponding members,
and enums by nominal type, variant, and corresponding members. Mutable objects
compare by identity. Interface values and host handles/path views do not support
general equality. A tuple/enum supports equality only if its members do.
Floating-point equality follows IEEE comparisons (`NaN != NaN`, `-0 == +0`).
Only bool, integer, and String keys are admitted to maps and sets in v1.

Iteration prevents structural mutation of the iterated collection through any
alias: insert, remove, clear, reorder, and length-changing operations fail before
changing it. Replacing an existing element without changing structure is allowed.
Mutating an object referenced by an element is allowed. Iteration protection ends
on exhaustion, break, return, or failure, including nested iteration.

String lengths and slices use byte offsets; slices validate UTF-8 boundaries.
Unicode scalar counting is a separately named operation. Integer arithmetic is
checked in every backend and build mode. Explicit wrapping operations are the
only exception. Floating-point optimization must preserve specified results.

## Evaluation and assignment

Operands, receivers, arguments, and dynamic indexes evaluate left to right,
exactly once. Short-circuit operators evaluate only the selected operand.

For `root()[index()].field += rhs()`, evaluation is:

1. Evaluate the root and indexes and retain a location description.
2. Evaluate the right-hand side.
3. Validate the location and read its current value.
4. Compute and validate the replacement, including overflow and resource checks.
5. Commit one modification.

Plain assignment follows the same order without reading the old value. A path
denotes the current location, not a detached Rust reference or a pinned element.
If RHS execution removes the location, the final write fails. RHS effects already
performed remain visible. No mutable host borrow spans RHS execution.

## Acceptance examples

- Mutating `b` after `val b = a` changes the object observed through `a`.
- Separately allocated mutable objects with identical fields compare unequal.
- Separately constructed equal enum values compare equal.
- A shallow copy has new container identity but shares referenced elements.
- Structural mutation through an alias during iteration fails without mutation.
- A compound assignment evaluates root/index/RHS once, reads after RHS, and never
  writes after a failed RHS, invalidated location, or overflow.
