# Kagari standard library

The native-provider restoration currently installs these optional registered modules:

- [array](array.kgr): ArrayList new/len/is_empty/push/from_fn/sort_by/sort,
  List len/is_empty/get, MutableList.set and indexed views.
- [math](math.kgr): all eleven numeric helpers with closed OrderedNumber/SignedNumber
  bounds, width checks and finite/domain/overflow traps. Applications can install
  its actual math_api::math provider independently of default library selection.
- [ops](ops.kgr): operator/index protocols, six range shapes and Bound.
- [cmp](cmp.kgr): PartialEq/Eq/PartialOrd/Ord, Ordering and checked scalar
  implementations. Floating-point values implement only the partial protocols.
- [numeric](numeric.kgr): 175 integer methods across all ten widths, including
  wrapping, checked, overflowing and saturating arithmetic, rotations and radix parsing.
- [string](string.kgr): String, ParseError and thirteen actual primitive FromStr
  implementations, plus all 27 direct String helpers for UTF-8 queries, slicing,
  case mapping, trimming, concatenation, repetition and replacement. String.parse
  and lazy traversal remain open under their planned composition/state steps.
- [option](option.kgr) and [result](result.kgr): rooted is_some/is_none/is_ok/is_err
  and unwrap_or queries, with explicit constructor exports and preserved error origins.
- Other modules retain required protocols and native representations; their old
  public implementations are temporarily unavailable.

Rust `#[native_module]` registrations own declarations and handlers. HIR imports
registered records directly; generated `.kgr` files provide tooling syntax,
documentation and navigation. Registered package text is never lowered to establish
native signatures. Remaining modules still use the legacy source package during
migration. MIR/bytecode carry checked imports, and the common frame driver owns
invocation roots and callbacks. Source annotations do not grant native authority.

Engine installs the ordinary native packages by default; applications can disable
that selection and install their own packages through the same checked API.

Native packages can declare an installed package alias independently of their
canonical identity. Direct registered references resolve from actual installed
type providers, including application-owned names. Option, Result, String and
numeric declarations now come from actual registrations and have been removed
from the legacy source manifest. `#[native_type(export_variants)]` explicitly
exports an enum's constructors at module scope. Remaining namespace/prelude
migration belongs to the retirement step. Generated views are refreshed by the
native provider artifact example; they are never executable declaration inputs.

The [active plan](../docs/native-provider-refactor.md#full-library-restoration-sequence-2026-10-01)
records restoration order, acceptance conditions and carried errors. Existing
behavior tests remain required for final acceptance.

Sorting accepts either a supplied comparator or a checked `Ord::cmp` target.
Both use the same bounded native continuation and stable bottom-up merge. Working
buffers remain rooted GC storage until one checked replacement; failed comparison
leaves the original slots intact while preserving completed payload effects.
Application packages can use the same sorting helper or implement another prepared
reorder with `native_value::reorder::NativeReorder`. Manual array registration
requires the actual ops and cmp catalog declarations; it does not read array.kgr.
