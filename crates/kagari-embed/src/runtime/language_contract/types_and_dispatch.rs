use super::{Case, Expected, Value, run_routes};

#[test]
fn generic_aggregate_layout_instances() {
    let generic_aggregates = Case::new(
        "generic-aggregate-layout-instances",
        "pub struct Cell<T> { var value: T } pub enum Packet<T> { Data(T) } pub enum Unused<T> { Data(T) } fn get<T>(x: Cell<T>) -> T { x.value } fn main() -> (i32, bool, bool) { val a = Cell { value: 7 }; val b = Cell { value: true }; a.value = 8; (get(a), get(b), Packet::Data(7) == Packet::Data(7)) }",
        Expected::Value(|heap| {
            heap.alloc_tuple(vec![Value::I32(8), Value::Bool(true), Value::Bool(true)])
                .unwrap()
        }),
    );
    run_routes(&generic_aggregates);
}

#[test]
fn checked_where_bounds_through_forwarding() {
    let checked_where_bounds = Case::new(
        "checked-where-bounds-through-forwarding",
        "use std::hash::{Hash};\ntrait Get { fn get(self) -> i32; } struct P {} impl Get for P { fn get(self) -> i32 { 42 } } fn read<T>(value: T) -> i32 where T: Get { value.get() } fn wrap<U>(value: U) -> i32 where U: Get { read(value) } fn pass<T>(value: T) -> T where T: Eq + Hash { value } fn main() -> (i32, i32) { (wrap(P {}), pass(7)) }",
        Expected::Value(|heap| {
            heap.alloc_tuple(vec![Value::I32(42), Value::I32(7)])
                .unwrap()
        }),
    );
    run_routes(&checked_where_bounds);
}

#[test]
fn nominal_trait_methods_on_one_receiver() {
    let distinct_trait_methods = Case::new(
        "nominal-trait-methods-on-one-receiver",
        "trait Left { fn get(self) -> i32; } trait Right { fn get(self) -> i32; } struct Point {} impl Left for Point { fn get(self) -> i32 { 11 } } impl Right for Point { fn get(self) -> i32 { 22 } } fn left<T: Left>(x: T) -> i32 { x.get() } fn right<T: Right>(x: T) -> i32 { x.get() } fn main() -> (i32, i32) { val p = Point {}; (left(p), right(p)) }",
        Expected::Value(|heap| {
            heap.alloc_tuple(vec![Value::I32(11), Value::I32(22)])
                .unwrap()
        }),
    );
    run_routes(&distinct_trait_methods);
}

#[test]
fn empty_type_application_is_not_erased() {
    run_routes(&Case::new(
        "empty-type-application-is-not-erased",
        "fn unused(x: i32<>) {} fn main() {}",
        Expected::Diagnostic("KG_TYPE_UNKNOWN_TYPE"),
    ));
}

#[test]
fn local_type_shadows_standard_constructor() {
    run_routes(&Case::new(
        "local-type-shadows-standard-constructor",
        "struct HashMap {} fn unused(x: HashMap<i32, String>) {} fn main() {}",
        Expected::Diagnostic("KG_TYPE_UNKNOWN_TYPE"),
    ));
}

#[test]
fn binder_shadows_standard_type_constructor() {
    run_routes(&Case::new(
        "binder-shadows-standard-type-constructor",
        "fn unused<Option>(x: Option<i32>) {} fn main() {}",
        Expected::Diagnostic("KG_TYPE_UNKNOWN_TYPE"),
    ));
}

#[test]
fn ambiguous_bound_method() {
    run_routes(&Case::new(
        "ambiguous-bound-method",
        "trait Left { fn get(self) -> i32; } trait Right { fn get(self) -> i32; } fn read<T: Left + Right>(x: T) -> i32 { x.get() } fn main() {}",
        Expected::Diagnostic("KG_TYPE_AMBIGUOUS_METHOD"),
    ));
}

#[test]
fn duplicate_trait_method() {
    run_routes(&Case::new(
        "duplicate-trait-method",
        "trait View { fn get(self) -> i32; fn get(self) -> i32; } fn main() {}",
        Expected::Diagnostic("KG_RESOLVE_DUPLICATE_METHOD"),
    ));
}

#[test]
fn duplicate_impl_method() {
    run_routes(&Case::new(
        "duplicate-impl-method",
        "struct Point {} impl Point { fn get(self) -> i32 { 1 } fn get(self) -> i32 { 2 } } fn main() {}",
        Expected::Diagnostic("KG_RESOLVE_DUPLICATE_METHOD"),
    ));
}

#[test]
fn applied_impl_trait_preserves_arguments() {
    run_routes(&Case::new(
        "applied-impl-trait-preserves-arguments",
        "trait View<T> {} struct Point {} impl View<i32> for Point {} fn main() {}",
        Expected::Value(|_| Value::Unit),
    ));
}

#[test]
fn generic_binder_shadows_trait() {
    run_routes(&Case::new(
        "generic-binder-shadows-trait",
        "trait View {} struct Point {} impl<View> View for Point {} fn main() {}",
        Expected::Diagnostic("KG_TYPE_INVALID_TRAIT_REFERENCE"),
    ));
}

#[test]
fn standard_constraint_cannot_be_implemented() {
    run_routes(&Case::new(
        "standard-constraint-cannot-be-implemented",
        "struct Point {} impl Eq for Point {} fn main() {}",
        Expected::Diagnostic("KG_TYPE_INVALID_TRAIT_IMPL"),
    ));
}

#[test]
fn ambiguous_module_name() {
    run_routes(&Case::new(
        "ambiguous-module-name",
        "const pick: i32 = 1; fn pick() -> i32 { 2 } fn main() -> i32 { pick() }",
        Expected::Diagnostic("KG_RESOLVE_DUPLICATE_DECLARATION"),
    ));
}

#[test]
fn standard_namespace_independent_of_value_parameter() {
    run_routes(&Case::new(
        "standard-namespace-independent-of-value-parameter",
        "use core::option as api; fn pick(api: i32) -> i32 { val n = match api::Option::Some(1) { Some(x) => x, None => 0 }; n + api } fn main() -> i32 { pick(2) }",
        Expected::Value(|_| Value::I32(3)),
    ));
}

#[test]
fn resolved_language_constructor() {
    run_routes(&Case::new(
        "resolved-language-constructor",
        "use core::option as api; fn main() -> i32 { match api::Option::Some(5) { Some(x) => x, None => 0 } }",
        Expected::Value(|_| Value::I32(5)),
    ));
}

#[test]
fn enum_unit_call_rejected() {
    run_routes(&Case::new(
        "enum-unit-call-rejected",
        "enum Event { Empty } fn main() -> Event { Event::Empty() }",
        Expected::Diagnostic("KG_TYPE_INVALID_CALL_TARGET"),
    ));
}

#[test]
fn enum_value_members() {
    run_routes(&Case::new(
        "enum-value-members",
        "enum Event { Empty, Data(i32, String) } fn main() -> bool { Event::Empty == Event::Empty && Event::Data(7, \"x\") == Event::Data(7, \"x\") && Event::Data(7, \"x\") != Event::Data(8, \"x\") }",
        Expected::Value(|_| Value::Bool(true)),
    ));
}

#[test]
fn enum_alias_members() {
    run_routes(&Case::new(
        "enum-alias-members",
        "enum Event { Data([i32]) } fn main() -> bool { val a = [1]; val x = Event::Data(a); a[0] = 2; x == Event::Data(a) && x != Event::Data([2]) }",
        Expected::Value(|_| Value::Bool(true)),
    ));
}

#[test]
fn enum_evaluation_order() {
    run_routes(&Case::new("enum-evaluation-order", "enum Event { Data(i32, i32) } fn first() -> i32 { print(\"first\"); 1 } fn second() -> i32 { print(\"second\"); 2 } fn main() -> bool { Event::Data(first(), second()) == Event::Data(1, 2) }", Expected::Value(|_| Value::Bool(true))).effects(&["first", "second"], &["first", "second"]));
}

#[test]
fn heap_out_of_bounds_preserves_earlier_write() {
    run_routes(
        &Case::new(
            "heap-out-of-bounds-preserves-earlier-write",
            "use observe as test; fn main() { val a = test::vector(); a[0] = 42; a[9] = 2; }",
            Expected::IndexTrap,
        )
        .vector(&[1], &[42]),
    );
}

#[test]
fn diamond_calls_shared_dependency() {
    let mut diamond = Case::new(
        "diamond-calls-shared-dependency",
        "use contract::left::left; use contract::right::right; fn main() -> i32 { left() + right() }",
        Expected::Value(|_| Value::I32(42)),
    ).modules(&[
        ("leaf", "pub fn leaf() -> i32 { 21 }"),
        ("left", "use contract::leaf::leaf; pub fn left() -> i32 { leaf() }"),
        ("right", "use contract::leaf::leaf; pub fn right() -> i32 { leaf() }"),
    ]);
    diamond.repeat = 2;
    run_routes(&diamond);
}

#[test]
fn cyclic_imports_allow_nonrecursive_calls() {
    let cycle = Case::new(
        "cyclic-imports-allow-nonrecursive-calls",
        "use contract::dependency::answer; pub fn main() -> i32 { answer() }",
        Expected::Value(|_| Value::I32(42)),
    )
    .modules(&[(
        "dependency",
        "use contract::root::main; pub fn answer() -> i32 { 42 }",
    )]);
    run_routes(&cycle);
}

#[test]
fn publication_pins_old_dependencies_and_rejects_stale_candidate() {
    let candidate = Case::new(
        "published-dependency-version",
        "use contract::dependency::answer; fn main() -> i32 { answer() }",
        Expected::Value(|_| Value::I32(99)),
    )
    .modules(&[("dependency", "pub fn answer() -> i32 { 99 }")]);
    let mut versioned = Case::new(
        "publication-pins-old-dependencies-and-rejects-stale-candidate",
        "use contract::dependency::answer; fn main() -> i32 { answer() }",
        Expected::Value(|_| Value::I32(42)),
    )
    .modules(&[("dependency", "pub fn answer() -> i32 { 42 }")]);
    versioned.published_reload = Some(&candidate);
    run_routes(&versioned);
}

#[test]
fn unknown_inherent_impl_target() {
    run_routes(&Case::new(
        "unknown-inherent-impl-target",
        "impl Missing {} fn main() {}",
        Expected::Diagnostic("KG_TYPE_UNKNOWN_ANNOTATION"),
    ));
}

#[test]
fn unknown_where_target() {
    run_routes(&Case::new(
        "unknown-where-target",
        "fn bad<T>(value: T) where Missing: PartialEq {} fn main() {}",
        Expected::Diagnostic("KG_TYPE_INVALID_BOUND_TARGET"),
    ));
}

#[test]
fn shadowed_generic_return() {
    run_routes(&Case::new(
        "shadowed-generic-return",
        "impl<T> [T] { fn wrong<T>(self, value: T) -> T { self[0] } } fn main() {}",
        Expected::Diagnostic("KG_TYPE_RETURN_TYPE_MISMATCH"),
    ));
}

#[test]
fn generic_values() {
    run_routes(&Case::new(
        "generic-values",
        "fn echo<T>(x: T) -> T { x } fn pass<U>(x: U) -> U { echo(x) } fn main() -> (i32, bool, String) { (pass(7), pass(true), echo(\"ok\")) }",
        Expected::Value(|heap| {
            heap.alloc_tuple(vec![
                Value::I32(7),
                Value::Bool(true),
                heap.alloc_string("ok".into()).unwrap(),
            ])
            .unwrap()
        }),
    ));
}

#[test]
fn generic_recursion() {
    run_routes(&Case::new(
        "generic-recursion",
        "fn repeat<T>(x: T, n: i32) -> T { if n == 0 { x } else { repeat(x, n - 1) } } fn main() -> i32 { repeat(7, 3) }",
        Expected::Value(|_| Value::I32(7)),
    ));
}

#[test]
fn generic_array_elements() {
    run_routes(&Case::new(
        "generic-array-elements",
        "fn first<T>(xs: [T]) -> T { xs[0] } fn main() -> (i32, String) { (first([7]), first([\"ok\"])) }",
        Expected::Value(|heap| {
            heap.alloc_tuple(vec![Value::I32(7), heap.alloc_string("ok".into()).unwrap()])
                .unwrap()
        }),
    ));
}

#[test]
fn generic_equality() {
    run_routes(&Case::new(
        "generic-equality",
        "fn same<T: PartialEq>(a: T, b: T) -> bool { a == b } fn main() -> (bool, bool) { (same(1, 1), same(\"a\", \"b\")) }",
        Expected::Value(|heap| {
            heap.alloc_tuple(vec![Value::Bool(true), Value::Bool(false)])
                .unwrap()
        }),
    ));
}

#[test]
fn generic_static_trait() {
    run_routes(&Case::new(
        "generic-static-trait",
        "trait Get { fn get(self) -> i32; } struct P { val n: i32 } impl Get for P { fn get(self) -> i32 { self.n } } fn read<T: Get>(value: T) -> i32 { value.get() } fn wrap<U: Get>(value: U) -> i32 { read(value) } fn main() -> i32 { wrap(P { n: 42 }) }",
        Expected::Value(|_| Value::I32(42)),
    ));
}

#[test]
fn interface_dynamic_call() {
    run_routes(&Case::new(
        "interface-dynamic-call",
        "trait Get { fn get(self) -> i32; } impl Get for i32 { fn get(self) -> i32 { self + 1 } } fn read(value: Get) -> i32 { value.get() } fn main() -> i32 { read(41) }",
        Expected::Value(|_| Value::I32(42)),
    ));
}

#[test]
fn interface_dynamic_argument() {
    run_routes(&Case::new(
        "interface-dynamic-argument",
        "trait Add { fn add(self, value: i32) -> i32; } impl Add for i32 { fn add(self, value: i32) -> i32 { self + value } } fn read(value: Add) -> i32 { value.add(2) } fn main() -> i32 { read(40) }",
        Expected::Value(|_| Value::I32(42)),
    ));
}

#[test]
fn applied_interface_dynamic_call() {
    run_routes(&Case::new(
        "applied-interface-dynamic-call",
        "trait Echo<T> { fn get(self) -> T; } impl Echo<i32> for i32 { fn get(self) -> i32 { self } } fn read(value: Echo<i32>) -> i32 { value.get() } fn main() -> i32 { read(42) }",
        Expected::Value(|_| Value::I32(42)),
    ));
}

#[test]
fn interface_method_declaration_order() {
    run_routes(&Case::new(
        "interface-method-declaration-order",
        "trait Get { fn one(self) -> i32; fn two(self) -> i32; } impl Get for i32 { fn two(self) -> i32 { 2 } fn one(self) -> i32 { 40 } } fn read(value: Get) -> i32 { value.one() + value.two() } fn main() -> i32 { read(0) }",
        Expected::Value(|_| Value::I32(42)),
    ));
}

#[test]
fn imported_interface_dynamic_call() {
    run_routes(&Case::new("imported-interface-dynamic-call", "use contract::dependency::Get; fn read(value: Get) -> i32 { value.get() } fn main() -> i32 { read(41) }", Expected::Value(|_| Value::I32(42))).modules(&[("dependency", "pub trait Get { fn get(self) -> i32; } impl Get for i32 { fn get(self) -> i32 { self + 1 } }")]));
}

#[test]
fn generic_impl_specialization() {
    run_routes(&Case::new(
        "generic-impl-specialization",
        "trait Get { fn get(self) -> i32; } struct Holder<T> { val value: T } impl<T> Get for Holder<T> { fn get(self) -> i32 { 42 } } fn read<U: Get>(x: U) -> i32 { x.get() } fn main() -> (i32, i32) { (read(Holder { value: 1 }), read(Holder { value: \"a\" })) }",
        Expected::Value(|heap| {
            heap.alloc_tuple(vec![Value::I32(42), Value::I32(42)])
                .unwrap()
        }),
    ));
}

#[test]
fn generic_impl_bound() {
    run_routes(&Case::new(
        "generic-impl-bound",
        "use std::hash::{Hash};\ntrait Get { fn get(self) -> i32; } struct Holder<T> { val value: T } impl<T: Eq + Hash> Get for Holder<T> { fn get(self) -> i32 { 42 } } fn read<U: Get>(x: U) -> i32 { x.get() } fn main() -> i32 { read(Holder { value: 1 }) }",
        Expected::Value(|_| Value::I32(42)),
    ));
}

#[test]
fn generic_impl_bound_rejected() {
    run_routes(&Case::new(
        "generic-impl-bound-rejected",
        "use std::hash::{Hash};\ntrait Get { fn get(self) -> i32; } struct Holder<T> { val value: T } impl<T: Eq + Hash> Get for Holder<T> { fn get(self) -> i32 { 42 } } fn read<U: Get>(x: U) -> i32 { x.get() } fn main() -> i32 { read(Holder { value: 1.5 }) }",
        Expected::Diagnostic("KG_TYPE_GENERIC_BOUND_NOT_SATISFIED"),
    ));
}

#[test]
fn generic_impl_trait_bound() {
    run_routes(&Case::new(
        "generic-impl-trait-bound",
        "trait Key {} impl Key for i32 {} trait Get { fn get(self) -> i32; } struct Holder<T> { val value: T } impl<T: Key> Get for Holder<T> { fn get(self) -> i32 { 42 } } fn read<U: Get>(x: U) -> i32 { x.get() } fn main() -> i32 { read(Holder { value: 1 }) }",
        Expected::Value(|_| Value::I32(42)),
    ));
}

#[test]
fn generic_impl_trait_bound_rejected() {
    run_routes(&Case::new(
        "generic-impl-trait-bound-rejected",
        "trait Key {} impl Key for i32 {} trait Get { fn get(self) -> i32; } struct Holder<T> { val value: T } impl<T: Key> Get for Holder<T> { fn get(self) -> i32 { 42 } } fn read<U: Get>(x: U) -> i32 { x.get() } fn main() -> i32 { read(Holder { value: \"a\" }) }",
        Expected::Diagnostic("KG_TYPE_GENERIC_BOUND_NOT_SATISFIED"),
    ));
}

#[test]
fn generic_impl_repeated_binder_rejected() {
    run_routes(&Case::new(
        "generic-impl-repeated-binder-rejected",
        "trait Get { fn get(self) -> i32; } struct Pair<T, U> { val left: T, val right: U } impl<T> Get for Pair<T, T> { fn get(self) -> i32 { 1 } } fn read<V: Get>(x: V) -> i32 { x.get() } fn main() -> i32 { read(Pair { left: 1, right: true }) }",
        Expected::Diagnostic("KG_TYPE_GENERIC_BOUND_NOT_SATISFIED"),
    ));
}

#[test]
fn generic_impl_overlap_rejected() {
    run_routes(&Case::new(
        "generic-impl-overlap-rejected",
        "trait Get { fn get(self) -> i32; } struct Holder<T> { val value: T } impl<T> Get for Holder<T> { fn get(self) -> i32 { 1 } } impl Get for Holder<i32> { fn get(self) -> i32 { 2 } } fn main() {}",
        Expected::Diagnostic("KG_TYPE_INVALID_TRAIT_IMPL"),
    ));
}

#[test]
fn applied_trait_impl_signature_rejected() {
    run_routes(&Case::new(
        "applied-trait-impl-signature-rejected",
        "trait Echo<T> { fn get(self) -> T; } struct Pair { val number: i32 } impl Echo<i32> for Pair { fn get(self) -> String { \"bad\" } } fn main() {}",
        Expected::Diagnostic("KG_TYPE_TRAIT_METHOD_MISMATCH"),
    ));
}

#[test]
fn applied_trait_impl_arity_rejected() {
    run_routes(&Case::new(
        "applied-trait-impl-arity-rejected",
        "trait Echo<T> {} struct Pair {} impl Echo<i32, bool> for Pair {} fn main() {}",
        Expected::Diagnostic("KG_TYPE_INVALID_TRAIT_REFERENCE"),
    ));
}

#[test]
fn applied_trait_impl_unknown_argument() {
    run_routes(&Case::new(
        "applied-trait-impl-unknown-argument",
        "trait Echo<T> {} struct Pair {} impl Echo<Missing> for Pair {} fn main() {}",
        Expected::Diagnostic("KG_TYPE_UNKNOWN_ANNOTATION"),
    ));
}

#[test]
fn applied_trait_impl_bound_rejected() {
    run_routes(&Case::new(
        "applied-trait-impl-bound-rejected",
        "use std::hash::{Hash};\ntrait Echo<T: Eq + Hash> {} struct Pair {} impl Echo<f32> for Pair {} fn main() {}",
        Expected::Diagnostic("KG_TYPE_GENERIC_BOUND_NOT_SATISFIED"),
    ));
}

#[test]
fn applied_trait_impl_bound_accepted() {
    run_routes(&Case::new(
        "applied-trait-impl-bound-accepted",
        "use std::hash::{Hash};\ntrait Echo<T: Eq + Hash> {} struct Pair {} impl Echo<i32> for Pair {} fn main() {}",
        Expected::Value(|_| Value::Unit),
    ));
}

#[test]
fn applied_trait_bound_dispatch() {
    run_routes(&Case::new(
        "applied-trait-bound-dispatch",
        "trait Echo<T> { fn get(self) -> T; } struct Holder { val n: i32 } impl Echo<i32> for Holder { fn get(self) -> i32 { self.n } } fn read<U: Echo<i32>>(x: U) -> i32 { x.get() } fn main() -> i32 { read(Holder { n: 42 }) }",
        Expected::Value(|_| Value::I32(42)),
    ));
}

#[test]
fn applied_trait_where_bound_dispatch() {
    run_routes(&Case::new(
        "applied-trait-where-bound-dispatch",
        "trait Echo<T> { fn get(self) -> T; } struct Holder { val n: i32 } impl Echo<i32> for Holder { fn get(self) -> i32 { self.n } } fn read<U>(x: U) -> i32 where U: Echo<i32> { x.get() } fn main() -> i32 { read(Holder { n: 42 }) }",
        Expected::Value(|_| Value::I32(42)),
    ));
}

#[test]
fn applied_trait_bound_distinguishes_arguments() {
    run_routes(&Case::new(
        "applied-trait-bound-distinguishes-arguments",
        "trait Echo<T> { fn get(self) -> T; } struct Holder { val n: i32 } impl Echo<i32> for Holder { fn get(self) -> i32 { self.n } } fn read<U: Echo<String>>(x: U) -> String { x.get() } fn main() -> String { read(Holder { n: 42 }) }",
        Expected::Diagnostic("KG_TYPE_GENERIC_BOUND_NOT_SATISFIED"),
    ));
}

#[test]
fn applied_trait_impls_share_receiver() {
    run_routes(&Case::new(
        "applied-trait-impls-share-receiver",
        "trait Echo<T> { fn get(self) -> T; } struct Holder {} impl Echo<i32> for Holder { fn get(self) -> i32 { 42 } } impl Echo<String> for Holder { fn get(self) -> String { \"ok\" } } fn read_int<U: Echo<i32>>(x: U) -> i32 { x.get() } fn read_string<U: Echo<String>>(x: U) -> String { x.get() } fn main() -> (i32, String) { (read_int(Holder {}), read_string(Holder {})) }",
        Expected::Value(|heap| {
            heap.alloc_tuple(vec![
                Value::I32(42),
                heap.alloc_string("ok".into()).unwrap(),
            ])
            .unwrap()
        }),
    ));
}

#[test]
fn applied_trait_template_specialization() {
    run_routes(&Case::new(
        "applied-trait-template-specialization",
        "trait Echo<T> { fn get(self) -> T; } struct Holder<T> { val value: T } impl<T> Echo<T> for Holder<T> { fn get(self) -> T { self.value } } fn read<U: Echo<i32>>(x: U) -> i32 { x.get() } fn main() -> i32 { read(Holder { value: 42 }) }",
        Expected::Value(|_| Value::I32(42)),
    ));
}

#[test]
fn applied_trait_caller_binder() {
    run_routes(&Case::new(
        "applied-trait-caller-binder",
        "trait Echo<T> { fn get(self) -> T; } struct Holder<T> { val value: T } impl<T> Echo<T> for Holder<T> { fn get(self) -> T { self.value } } fn read<T, U: Echo<T>>(x: U, fallback: T) -> T { x.get() } fn main() -> i32 { read(Holder { value: 42 }, 0) }",
        Expected::Value(|_| Value::I32(42)),
    ));
}

#[test]
fn applied_trait_template_overlap_rejected() {
    run_routes(&Case::new(
        "applied-trait-template-overlap-rejected",
        "trait Echo<T> { fn get(self) -> T; } struct Holder<T> { val value: T } impl<T> Echo<T> for Holder<T> { fn get(self) -> T { self.value } } impl Echo<i32> for Holder<i32> { fn get(self) -> i32 { 1 } } fn main() {}",
        Expected::Diagnostic("KG_TYPE_INVALID_TRAIT_IMPL"),
    ));
}

#[test]
fn generic_conflicting_arguments() {
    run_routes(&Case::new(
        "generic-conflicting-arguments",
        "fn choose<T>(a: T, b: T) -> T { a } fn main() -> i32 { choose(1, true) }",
        Expected::Diagnostic("KG_TYPE_ARGUMENT_TYPE_MISMATCH"),
    ));
}

#[test]
fn generic_missing_argument() {
    run_routes(&Case::new(
        "generic-missing-argument",
        "fn unused<T>() {} fn main() { unused(); }",
        Expected::Diagnostic("KG_TYPE_CANNOT_INFER_GENERIC_ARGUMENT"),
    ));
}

#[test]
fn generic_public_entry() {
    run_routes(&Case::new(
        "generic-public-entry",
        "pub fn echo<T>(value: T) -> T { value } fn main() {}",
        Expected::Diagnostic("KG_TYPE_PUBLIC_GENERIC_FUNCTION"),
    ));
}

#[test]
fn generic_parameter_order() {
    run_routes(&Case::new(
        "generic-parameter-order",
        "fn reverse<T, U>(first: U, second: T) -> (T, U) { (second, first) } fn main() -> (i32, bool) { reverse(true, 7) }",
        Expected::Value(|heap| {
            heap.alloc_tuple(vec![Value::I32(7), Value::Bool(true)])
                .unwrap()
        }),
    ));
}

#[test]
fn generic_numeric_instances() {
    run_routes(&Case::new(
        "generic-numeric-instances",
        "fn add<T: OrderedNumber>(a: T, b: T) -> T { a + b } fn main() -> (i32, f32) { (add(1, 2), add(1.5, 2.5)) }",
        Expected::Value(|heap| {
            heap.alloc_tuple(vec![Value::I32(3), Value::F32(4.0)])
                .unwrap()
        }),
    ));
}

#[test]
fn generic_same_storage_distinct_types() {
    run_routes(&Case::new(
        "generic-same-storage-distinct-types",
        "trait Get { fn get(self) -> i32; } struct P { val n: i32 } struct Q { val n: i32 } impl Get for P { fn get(self) -> i32 { self.n } } impl Get for Q { fn get(self) -> i32 { self.n + 1 } } fn read<T: Get>(value: T) -> i32 { value.get() } fn main() -> (i32, i32) { (read(P { n: 42 }), read(Q { n: 42 })) }",
        Expected::Value(|heap| {
            heap.alloc_tuple(vec![Value::I32(42), Value::I32(43)])
                .unwrap()
        }),
    ));
}

#[test]
fn generic_numeric_overflow() {
    run_routes(&Case::new("generic-numeric-overflow", "fn add<T: OrderedNumber>(a: T, b: T) -> T { a + b } fn main() -> i32 { print(\"before\"); add(2147483647, 1) }", Expected::ScriptTrap("integer overflow")).effects(&["before"], &["before"]));
}

#[test]
fn generic_missing_bound() {
    run_routes(&Case::new(
        "generic-missing-bound",
        "trait Get { fn get(self) -> i32; } fn read<T: Get>(value: T) -> i32 { value.get() } fn main() -> i32 { read(1) }",
        Expected::Diagnostic("KG_TYPE_GENERIC_BOUND_NOT_SATISFIED"),
    ));
}

#[test]
fn user_type_of_is_direct_call() {
    run_routes(&Case::new(
        "user-type-of-is-direct-call",
        "fn type_of(n: i32) -> i32 { n + 2 } fn main() -> i32 { type_of(40) }",
        Expected::Value(|_| Value::I32(42)),
    ));
}

#[test]
fn method_receiver_before_argument() {
    run_routes(&Case::new("method-receiver-before-argument", "fn receiver() -> Vec<i32> { print(\"receiver\"); Vec::from([1]) } fn value() -> i32 { print(\"argument\"); 2 } fn main() { receiver().push(value()); }", Expected::Value(|_| Value::Unit)).effects(&["receiver", "argument"], &["receiver", "argument"]));
}

#[test]
fn reject_assignment_index_type() {
    run_routes(&Case::new(
        "reject-assignment-index-type",
        "fn main() -> i32 { val a = [1]; a[true] += 1; a[0] }",
        Expected::Diagnostic("KG_TYPE_INVALID_ASSIGNMENT_TARGET"),
    ));
}

#[test]
fn const_dependency() {
    run_routes(
        &Case::new(
            "const-dependency",
            "const BASE: i32 = 6 * 7; const NEXT: i32 = BASE + 1; fn main() -> i32 { NEXT }",
            Expected::Value(|_| Value::I32(43)),
        )
        .native(),
    );
}

#[test]
fn enum_value() {
    run_routes(&Case::new(
        "enum_value",
        "fn main() -> bool { val a = Vec::from([1]); val b = Vec::from([1]); a.pop() == b.pop() }",
        Expected::Value(|_| Value::Bool(true)),
    ));
}

#[test]
fn enum_different_members() {
    run_routes(&Case::new(
        "enum_different_members",
        "fn main() -> bool { val a = Vec::from([1, 2]); a.pop() != a.pop() }",
        Expected::Value(|_| Value::Bool(true)),
    ));
}

#[test]
fn enum_object_identity() {
    run_routes(&Case::new(
        "enum_object_identity",
        "fn main() -> bool { val a = Vec::from([[1]]); val b = Vec::from([[1]]); a.pop() != b.pop() }",
        Expected::Value(|_| Value::Bool(true)),
    ));
}

#[test]
fn enum_tuple_members_keep_map_identity() {
    run_routes(&Case::new(
        "enum_tuple_members_keep_map_identity",
        "use std::collections::{HashMap};\nenum Packet { Data((HashMap<String, i32>, i32)) } fn main() -> bool { val a: HashMap<String, i32> = HashMap::new(); val b: HashMap<String, i32> = HashMap::new(); val x = Packet::Data((a, 7)); val y = x; a.insert(\"key\", 42); b.insert(\"key\", 42); x == y && x == Packet::Data((a, 7)) && x != Packet::Data((b, 7)) }",
        Expected::Value(|_| Value::Bool(true)),
    ));
}

#[test]
fn interface_equality_rejected() {
    run_routes(&Case::new(
        "interface_equality_rejected",
        "trait Marker {} fn same(a: Marker, b: Marker) -> bool { a == b }",
        Expected::Diagnostic("KG_TYPE_BINARY_OPERAND_TYPE_MISMATCH"),
    ));
}

#[test]
fn tuple_interface_equality_rejected() {
    run_routes(&Case::new(
        "tuple_interface_equality_rejected",
        "trait Marker {} fn same(a: Marker, b: Marker) -> bool { (1, a) == (1, b) }",
        Expected::Diagnostic("KG_TYPE_BINARY_OPERAND_TYPE_MISMATCH"),
    ));
}
