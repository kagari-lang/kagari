use kagari_common::SourceFile;
use kagari_embed::{ArtifactOptions, EmbeddingError, KagariEngine};

#[test]
fn generic_trait_methods_infer_concrete_arguments_across_execution_routes() {
    execute_contextual_source(
        include_str!("../../../examples/generic-trait-methods.kgr"),
        42,
    );
}

#[test]
fn generic_trait_method_bounds_reject_invalid_arguments() {
    let source = include_str!("../../../examples/generic-trait-methods.kgr")
        .replace("fn echo<U>(", "fn echo<U: HashKey>(")
        .replace("fn echo<V>(", "fn echo<V: HashKey>(")
        .replace("value.echo(42)", "value.echo([42]); 42");
    let error = KagariEngine::default()
        .compile_to_artifact(
            SourceFile::new("generic-method-bound.kgr", source),
            Default::default(),
            Default::default(),
        )
        .unwrap_err();
    let EmbeddingError::Diagnostics { diagnostics } = error else {
        panic!("expected source diagnostics")
    };
    assert!(
        diagnostics
            .iter()
            .any(|diagnostic| { diagnostic.code == "KG_TYPE_STANDARD_CONSTRAINT_NOT_SATISFIED" }),
        "{diagnostics:?}"
    );
}

#[test]
fn explicit_enum_arguments_execute_with_distinct_concrete_layouts() {
    execute_contextual_source(
        "enum Token<T> { Empty, Data(T) } fn empty<T>() -> Token<T> { Token<T>::Empty } fn main() -> i32 { val a = Token<i32>::Data(7); val b = Token<bool>::Data(true); val e: Token<i32> = empty(); if a == Token<i32>::Data(7) && b == Token<bool>::Data(true) && e == Token<i32>::Empty() { 42 } else { 0 } }",
        42,
    );
}

#[test]
fn recursive_comparable_bounds_execute_for_nominal_and_call_arguments() {
    execute_contextual_source(
        "struct Key<T: Comparable> { val value: i32 } fn consume<T: Comparable>(value: T) {} fn make<T: Comparable>(value: T) -> Key<(T, i32)> { consume((value, 7)); Key { value: 42 } } fn main() -> i32 { make(true).value }",
        42,
    );
}

#[test]
fn irrefutable_match_stops_before_unreachable_generic_arms() {
    for (pattern, result) in [("_", "42"), ("value", "value")] {
        execute_contextual_source(
            &format!(
                "fn grow<T>(x: T) -> i32 {{ grow((x, x)) }} fn main() -> i32 {{ match 42 {{ {pattern} => {result}, _ => grow(1) }} }}"
            ),
            42,
        );
    }
}

#[test]
fn explicit_returns_execute_without_a_synthetic_unit_result() {
    execute_contextual_source(
        r#"
        struct Marker<T> { val value: i32 }
        fn direct() -> Marker<i32> { return Marker { value: 20 }; }
        fn choose(flag: bool) -> i32 {
            if flag { return 22; } else { return 2; };
        }
        fn mixed(flag: bool) -> i32 { if flag { return 0; } else { 9 } }
        fn from_loop() -> i32 { loop { return 0; } }
        fn from_match() -> i32 {
            match true { true => if true { return 0; } else { return 9; }, _ => 9 }
        }
        fn main() -> i32 { return direct().value + choose(true) + mixed(true) + from_loop() + from_match(); false }
        "#,
        42,
    );
}

#[test]
fn returning_initializer_does_not_store_an_unproduced_value() {
    execute_contextual_source(
        "fn main() -> i32 { val unused = if true { return 42; } else { return 7; }; }",
        42,
    );
    execute_contextual_source(
        "fn grow<T>(x: T) { grow((x, x)); } fn take<T>(first: (), second: T) {} fn main() -> i32 { take(if true { return 42; } else { return 7; }, grow(1)); }",
        42,
    );
}

#[test]
fn terminating_helper_operands_preserve_short_circuit_paths() {
    for (condition, expected) in [("false", 9), ("true", 42)] {
        execute_contextual_source(
            &format!(
                "fn main() -> i32 {{ {condition} && (type_of(if true {{ return 42; }} else {{ return 7; }}) == \"\"); 9 }}"
            ),
            expected,
        );
    }
    execute_contextual_source(
        "fn main() -> i32 { while type_of(if true { return 42; } else { return 7; }) == \"\" { } 9 }",
        42,
    );
    execute_contextual_source(
        "fn main() -> i32 { var value = \"\"; value = type_of(if true { return 42; } else { return 7; }); 9 }",
        42,
    );
}

#[test]
fn terminating_assignment_places_stop_before_later_indexes() {
    for target in [
        "grid[index(if true { return 42; } else { return 7; })][grow(1)]",
        "matrix(if true { return 42; } else { return 7; })[grow(1)][0]",
    ] {
        execute_contextual_source(
            &format!(
                "fn grow<T>(x: T) -> i32 {{ grow((x, x)) }} fn index(value: ()) -> i32 {{ 0 }} fn matrix(value: ()) -> [[i32]] {{ [[0]] }} fn main() -> i32 {{ val grid = [[0]]; {target} += grow(2); 9 }}"
            ),
            42,
        );
    }
}

#[test]
fn contextual_phantom_layouts_execute_from_source_and_encoded_artifacts() {
    execute_contextual_source(
        "struct Marker<T> { val value: i32 } struct Outer<T> { val marker: Marker<T> } enum Tag<T> { Empty, Data(Marker<T>) } fn tag() -> Tag<i32> { Tag::Empty } fn build() -> (Outer<i32>, [Outer<bool>]) { (if true { Outer { marker: Marker { value: 20 } } } else { Outer { marker: Marker { value: 0 } } }, [match 1 { 1 => Outer { marker: Marker { value: 22 } }, _ => Outer { marker: Marker { value: 0 } } }]) } fn main() -> i32 { val a: Outer<i32> = Outer { marker: Marker { value: 20 } }; val b: Outer<bool> = Outer { marker: Marker { value: 22 } }; val result = build(); val empty: Tag<i32> = Tag::Empty(); val payload: Tag<bool> = Tag::Data(Marker { value: 5 }); if empty == tag() { result[0].marker.value + result[1][0].marker.value + a.marker.value + b.marker.value } else { 0 } }",
        84,
    );
}

#[test]
fn trait_calls_use_checked_parameter_context() {
    execute_contextual_source(
        r#"
        struct Marker<T> { val value: i32 }
        enum Token<T> { Empty }
        trait Take { fn take(self, marker: Marker<i32>, token: Token<bool>) -> i32; }
        struct Actor { val offset: i32 }
        impl Take for Actor {
            fn take(self, marker: Marker<i32>, token: Token<bool>) -> i32 { self.offset + marker.value }
        }
        fn invoke<T: Take>(value: T) -> i32 { value.take(Marker { value: 40 }, Token::Empty) }
        fn main() -> i32 { invoke(Actor { offset: 2 }) }
    "#,
        42,
    );
}

#[test]
fn generic_calls_propagate_result_and_preceding_argument_context() {
    execute_contextual_source(
        r#"
        struct Marker<T> { val value: i32 }
        enum Token<T> { Empty }
        fn identity<T>(value: T) -> T { value }
        fn empty<T>() -> Token<T> { Token::Empty }
        fn consume<T>(seed: T, marker: Marker<T>) -> i32 { marker.value }
        fn main() -> i32 {
            val first: Marker<i32> = identity(Marker { value: 20 });
            val token: Token<bool> = empty();
            val expected: Token<bool> = Token::Empty;
            if token == expected { first.value + consume(true, Marker { value: 22 }) } else { 0 }
        }
    "#,
        42,
    );
}

#[test]
fn caller_binders_and_trait_self_supply_constructor_context() {
    execute_contextual_source(
        r#"
        struct Marker<T> { val value: i32 }
        trait Read { fn read(self, marker: Marker<Self>) -> i32; }
        struct Actor { val offset: i32 }
        impl Read for Actor {
            fn read(self, marker: Marker<Actor>) -> i32 { self.offset + marker.value }
        }
        fn invoke<T: Read>(value: T) -> i32 { value.read(Marker { value: 20 }) }
        fn consume<T>(seed: T, value: Marker<T>) -> i32 { value.value }
        fn relay<T>(seed: T) -> i32 { consume(seed, Marker { value: 20 }) }
        fn main() -> i32 { invoke(Actor { offset: 2 }) + relay(true) }
    "#,
        42,
    );
}

#[test]
fn assignment_targets_supply_constructor_context() {
    execute_contextual_source(
        r#"
        struct Marker<T> { val value: i32 }
        struct Box { var marker: Marker<i32> }
        enum Token<T> { Empty }
        fn main() -> i32 {
            var local: Marker<i32> = Marker { value: 0 };
            val object = Box { marker: Marker { value: 0 } };
            val array: [Marker<bool>] = [Marker { value: 0 }];
            var token: Token<i32> = Token::Empty;
            local = Marker { value: 10 };
            object.marker = Marker { value: 12 };
            array[0] = Marker { value: 20 };
            token = Token::Empty();
            local.value + object.marker.value + array[0].value
        }
    "#,
        42,
    );
}

#[test]
fn empty_container_context_reaches_returns_fields_and_arguments() {
    execute_contextual_source(
        r#"
        struct Values { val array: [i32], val map: Map<i32, bool>, val set: Set<i32> }
        fn array() -> [i32] { [] }
        fn map() -> Map<i32, bool> { std::map::new() }
        fn set() -> Set<i32> { std::set::new() }
        fn empty(a: [i32], m: Map<i32, bool>, s: Set<i32>) -> bool {
            a.is_empty() && m.is_empty() && s.is_empty()
        }
        fn main() -> i32 {
            val value = Values { array: [], map: std::map::new(), set: std::set::new() };
            var replacement: Map<i32, bool> = map();
            replacement = std::map::new();
            if empty([], std::map::new(), std::set::new())
                && empty(array(), map(), set()) && empty(value.array, value.map, value.set)
                && replacement.is_empty() { 42 } else { 0 }
        }
    "#,
        42,
    );
}

#[test]
fn constructor_members_propagate_context_in_source_order() {
    execute_contextual_source(
        r#"
        struct Marker<T> { val value: i32 }
        struct Bundle<T> { val seed: T, val marker: Marker<T> }
        enum Packet<T> { Data(T, Marker<T>) }
        fn main() -> i32 {
            val bundle = Bundle { seed: 20, marker: Marker { value: 22 } };
            val packet = Packet::Data(true, Marker { value: 7 });
            if packet == packet { bundle.seed + bundle.marker.value } else { 0 }
        }
    "#,
        42,
    );
}

#[test]
fn explicit_struct_arguments_emit_distinct_phantom_layouts() {
    execute_contextual_source(
        r#"
        struct Marker<T> { val value: i32 }
        fn forward<T>() -> Marker<T> { Marker<T> { value: 20 } }
        fn main() -> i32 {
            val first = Marker<i32> { value: 20 };
            val second = Marker<bool> { value: 22 };
            val third: Marker<i32> = forward();
            first.value + second.value + third.value - 20
        }
    "#,
        42,
    );
}

fn execute_contextual_source(source: &str, expected: i32) {
    execute_contextual_source_with_writes(source, expected, false);
}

fn execute_contextual_source_with_writes(source: &str, expected: i32, reflection_write: bool) {
    let engine = KagariEngine::default();
    let mut context = kagari_embed::ExecutionContext::default();
    context.language_profile.allow_jit = true;
    context.language_profile.allow_reflection = true;
    context.language_profile.allow_reflection_write = reflection_write;
    context.capabilities.reflection_write = reflection_write;
    context.capabilities.jit = true;
    let artifact = engine
        .compile_to_artifact(
            SourceFile::new("context.kgr", source),
            kagari_embed::CompileOptions {
                language_profile: context.language_profile,
            },
            Default::default(),
        )
        .unwrap();
    for (encoded, jit) in [(false, false), (true, false), (true, true)] {
        let artifact = if encoded {
            kagari_embed::BytecodeArtifact::from_bytes(&artifact.to_bytes().unwrap()).unwrap()
        } else {
            artifact.clone()
        };
        context.jit_policy = if jit {
            kagari_embed::JitPolicy::Enabled
        } else {
            kagari_embed::JitPolicy::Disabled
        };
        let mut runtime = engine.runtime(context.clone());
        let loaded = runtime.load_program(artifact, Default::default()).unwrap();
        let result = if jit {
            let mut backend = kagari_jit_cranelift::CraneliftBackend::for_host().unwrap();
            runtime.execute_with_backend(&loaded, "main", &[], &context, &mut backend)
        } else {
            runtime.execute(&loaded, "main", &[], &context)
        }
        .unwrap();
        assert_eq!(
            result.return_value,
            kagari_runtime::value::Value::I32(expected)
        );
    }
}

#[test]
fn instance_limits_report_revision_owned_diagnostics_without_poisoning_compilation() {
    let engine = KagariEngine::default();
    let checked = engine
        .compile_source(
            SourceFile::new(
                "instances.kgr",
                "// 泛型😀\r\nfn echo<T>(value: T) -> T { value } fn main() -> i32 { echo(7) }",
            ),
            Default::default(),
        )
        .unwrap();
    let error = engine
        .emit_bytecode(
            &checked,
            ArtifactOptions {
                lowering: kagari_ir::IrLoweringOptions {
                    max_generic_instances: 0,
                    ..Default::default()
                },
                ..Default::default()
            },
        )
        .unwrap_err();
    let EmbeddingError::Diagnostics { diagnostics } = error else {
        panic!("structured diagnostic");
    };
    assert_eq!(diagnostics.len(), 1);
    assert_eq!(diagnostics[0].code, "KG_COMPILE_LIMIT_EXCEEDED");
    assert!(
        engine
            .source_snapshot()
            .contains(diagnostics[0].span.unwrap())
    );
    let artifact = engine.emit_bytecode(&checked, Default::default()).unwrap();
    assert_eq!(
        artifact.program.modules[artifact.program.root.index()]
            .functions
            .len(),
        2
    );
}

#[test]
fn cancelled_instantiation_keeps_the_checked_module_reusable() {
    let engine = KagariEngine::default();
    let checked = engine
        .compile_source(
            SourceFile::new("cancel-instances.kgr", "fn main() -> i32 { 7 }"),
            Default::default(),
        )
        .unwrap();
    let options = ArtifactOptions::default();
    options.lowering.cancel.cancel();
    assert!(matches!(
        engine.emit_bytecode(&checked, options),
        Err(EmbeddingError::Cancelled)
    ));
    engine.emit_bytecode(&checked, Default::default()).unwrap();
}

#[test]
fn unresolved_container_inference_is_a_diagnostic_at_codegen() {
    let engine = KagariEngine::default();
    let checked = engine
        .compile_source(
            SourceFile::new("inference.kgr", "fn main() { std::map::new(); }"),
            Default::default(),
        )
        .unwrap();
    let Err(EmbeddingError::Diagnostics { diagnostics }) =
        engine.emit_bytecode(&checked, Default::default())
    else {
        panic!("unresolved type must not enter IR");
    };
    assert_eq!(diagnostics[0].code, "KG_COMPILE_UNRESOLVED_TYPE");
}

#[test]
fn partially_inferred_parameters_preserve_independent_constructor_context() {
    execute_contextual_source(
        "enum Token<T> { Empty } struct Marker<T> { val value: i32 } fn take<T>(pair: (Token<i32>, T)) -> T { pair[1] } fn read<T>(pair: (Marker<i32>, T)) -> i32 { pair[0].value } fn main() -> i32 { val first = take((Token::Empty, 20)); first + read((Marker { value: 22 }, true)) }",
        42,
    );
}

#[test]
fn partial_constructor_member_context_executes_for_structs_and_enums() {
    execute_contextual_source(
        "enum Token<T> { Empty } struct Pair<T> { val pair: (Token<i32>, T) } enum Payload<T> { Pair((Token<i32>, T)) } fn main() -> i32 { val item = Pair { pair: (Token::Empty, 20) }; val payload = Payload::Pair((Token::Empty, 22)); if payload == Payload<i32>::Pair((Token<i32>::Empty, 22)) { item.pair[1] + 22 } else { 0 } }",
        42,
    );
}

#[test]
fn reflective_write_targets_supply_generic_constructor_context() {
    execute_contextual_source_with_writes(
        "struct Marker<T> { val value: i32 } struct Box { var value: Marker<i32> } fn main() -> i32 { val box = Box { value: Marker { value: 0 } }; val array: [Marker<i32>] = [Marker { value: 0 }]; set_field(box, \"value\", Marker { value: 20 }); set_index(array, 0, Marker { value: 22 }); box.value.value + array[0].value }",
        42,
        true,
    );
}

#[test]
fn standard_container_context_executes_through_methods_and_qualified_calls() {
    execute_contextual_source(
        "struct Marker<T> { val value: i32 } fn main() -> i32 { val values: [Marker<i32>] = []; values.push(Marker { value: 10 }); std::array::push(values, Marker { value: 10 }); val map: Map<i32, Marker<i32>> = std::map::new(); std::map::insert(map, 1, Marker { value: 22 }); values[0].value + values[1].value + map.get(1).unwrap_or(Marker { value: 0 }).value }",
        42,
    );
}

#[test]
fn generic_negation_executes_using_checked_signed_number_bounds() {
    execute_contextual_source(
        "fn negate<T: SignedNumber>(value: T) -> T { -value } fn forward<T>(value: T) -> T where T: SignedNumber { negate(value) } fn main() -> i32 { forward(-20) + negate(-22) }",
        42,
    );
}

#[test]
fn standard_equality_rhs_uses_left_constructor_context() {
    execute_contextual_source(
        "enum Token<T> { Empty } fn main() -> i32 { std::debug::assert_eq(Token<i32>::Empty, Token::Empty, \"inferred rhs\"); std::math::clamp(42, 0, 100) }",
        42,
    );
}

#[test]
fn binary_context_preserves_constructor_inference_and_left_to_right_evaluation() {
    execute_contextual_source(
        "enum Token<T> { Empty } struct Count { var value: i32 } fn left(count: Count) -> Token<i32> { count.value = count.value * 10 + 1; Token::Empty } fn right<T>(count: Count) -> Token<T> { count.value = count.value * 10 + 2; Token::Empty } fn main() -> i32 { val count = Count { value: 0 }; if left(count) == right(count) && Token<i32>::Empty == Token::Empty { count.value + 30 } else { 0 } }",
        42,
    );
}

#[test]
fn array_and_branch_context_execute_with_selected_branch_side_effects() {
    execute_contextual_source(
        "enum Token<T> { Empty } struct Count { var value: i32 } fn tick<T>(count: Count) -> Token<T> { count.value += 1; Token::Empty } fn main() -> i32 { val count = Count { value: 0 }; val values = [Token<i32>::Empty, tick(count)]; val a = if false { Token<i32>::Empty } else { tick(count) }; val b = match false { true => Token<i32>::Empty, false => tick(count) }; if values[1] == a && a == b { count.value + 39 } else { 0 } }",
        42,
    );
}

#[test]
fn terminating_array_members_preserve_prefix_effects_and_skip_suffixes() {
    execute_contextual_source(
        "struct Count { var value: i32 } fn tick(count: Count) -> i32 { count.value += 1; count.value } fn run(count: Count) -> i32 { val items = [tick(count), if true { return 40; } else { return 40; }, true, tick(count)]; 0 } fn main() -> i32 { val count = Count { value: 1 }; run(count) + count.value }",
        42,
    );
}

#[test]
fn returning_if_and_while_conditions_skip_unselected_work() {
    for statement in [
        "if (if true { return 40; } else { return 40; }) { count.value += 100; };",
        "while (if true { return 40; } else { return 40; }) { count.value += 100; }",
    ] {
        execute_contextual_source(
            &format!(
                "struct Count {{ var value: i32 }} fn run(count: Count) -> i32 {{ count.value += 1; {statement} count.value += 1000; 0 }} fn main() -> i32 {{ val count = Count {{ value: 1 }}; run(count) + count.value }}"
            ),
            42,
        );
    }
}

#[test]
fn nested_returns_preserve_the_inner_result_and_single_condition_evaluation() {
    execute_contextual_source(
        "struct Count { var value: i32 } fn tick(count: Count) -> bool { count.value += 1; true } fn run(count: Count) -> i32 { return if tick(count) { return 40; } else { return 0; }; } fn main() -> i32 { val count = Count { value: 1 }; run(count) + count.value }",
        42,
    );
}

#[test]
fn terminating_initializers_and_assignment_values_skip_the_write() {
    for statement in [
        "val value: i32 = if tick(count) { return 40; } else { return 0; };",
        "count.value = if tick(count) { return 40; } else { return 0; };",
        "count.value += if tick(count) { return 40; } else { return 0; };",
    ] {
        execute_contextual_source(
            &format!(
                "struct Count {{ var value: i32 }} fn tick(count: Count) -> bool {{ count.value += 1; true }} fn run(count: Count) -> i32 {{ {statement} count.value += 1000; 0 }} fn main() -> i32 {{ val count = Count {{ value: 1 }}; run(count) + count.value }}"
            ),
            42,
        );
    }
}

#[test]
fn terminating_function_arguments_skip_calls_and_generic_instances() {
    for signature in [
        "fn take(count: Count, value: i32) -> i32 { count.value += 100; value }",
        "fn take<T: SignedNumber>(count: Count, value: T) -> i32 { count.value += 100; 0 }",
    ] {
        execute_contextual_source(
            &format!(
                "struct Count {{ var value: i32 }} fn tick(count: Count) -> bool {{ count.value += 1; true }} {signature} fn run(count: Count) -> i32 {{ take(count, if tick(count) {{ return 40; }} else {{ return 0; }}) }} fn main() -> i32 {{ val count = Count {{ value: 1 }}; run(count) + count.value }}"
            ),
            42,
        );
    }
}

#[test]
fn terminating_trait_and_standard_arguments_preserve_only_operand_effects() {
    for body in [
        "value.take(if tick(count) { return 40; } else { return 0; })",
        r#"std::debug::assert(if tick(count) { return 40; } else { return 0; }, "unreachable"); 0"#,
    ] {
        execute_contextual_source(
            &format!(
                "struct Count {{ var value: i32 }} trait Take {{ fn take(self, input: bool) -> i32; }} impl Take for Count {{ fn take(self, input: bool) -> i32 {{ self.value += 100; 0 }} }} fn tick(count: Count) -> bool {{ count.value += 1; true }} fn run<T: Take>(value: T, count: Count) -> i32 {{ {body} }} fn main() -> i32 {{ val count = Count {{ value: 1 }}; run(count, count) + count.value }}"
            ),
            42,
        );
    }
}

#[test]
fn terminating_math_and_equality_operands_skip_standard_calls() {
    for body in [
        "std::math::min(ARG, 7)",
        "std::math::min(ARG, ARG)",
        "std::math::max(7, ARG)",
        "std::math::clamp(7, ARG, 9)",
        "std::math::abs(ARG)",
        r#"std::debug::assert_eq(ARG, 7, "unreachable"); 0"#,
        r#"std::debug::assert_eq(7, ARG, "unreachable"); 0"#,
    ] {
        let body = body.replace("ARG", "if tick(count) { return 40; } else { return 0; }");
        execute_contextual_source(
            &format!(
                "struct Count {{ var value: i32 }} fn tick(count: Count) -> bool {{ count.value += 1; true }} fn run(count: Count) -> i32 {{ {body} }} fn main() -> i32 {{ val count = Count {{ value: 1 }}; run(count) + count.value }}"
            ),
            42,
        );
    }
}

#[test]
fn terminating_enum_payloads_skip_unresolved_layouts_and_later_effects() {
    for constructor in ["Item::Value", "Item<i32>::Value"] {
        execute_contextual_source(
            &format!(
                "enum Item<T> {{ Value(T, bool) }} struct Count {{ var value: i32 }} fn tick(count: Count) -> bool {{ count.value += 1; true }} fn run(count: Count) -> i32 {{ {constructor}(if tick(count) {{ return 40; }} else {{ return 0; }}, tick(count)); 0 }} fn main() -> i32 {{ val count = Count {{ value: 1 }}; run(count) + count.value }}"
            ),
            42,
        );
    }
}

#[test]
fn terminating_struct_fields_skip_unused_layouts_and_remaining_effects() {
    for constructor in ["Item", "Item<i32>"] {
        execute_contextual_source(
            &format!(
                "struct Item<T> {{ val value: T, val flag: bool }} struct Count {{ var value: i32 }} fn tick(count: Count) -> bool {{ count.value += 1; true }} fn run(count: Count) -> i32 {{ {constructor} {{ value: if tick(count) {{ return 40; }} else {{ return 0; }}, flag: tick(count) }}; 0 }} fn main() -> i32 {{ val count = Count {{ value: 1 }}; run(count) + count.value }}"
            ),
            42,
        );
    }
}

#[test]
fn terminating_unary_operands_require_no_enclosing_result_layouts() {
    for expression in [
        "-(ARG)",
        "!(ARG)",
        "[-(ARG)]",
        "(-(ARG), tick(count))",
        "std::math::abs(-(ARG))",
    ] {
        let expression =
            expression.replace("ARG", "if tick(count) { return 40; } else { return 0; }");
        execute_contextual_source(
            &format!(
                "struct Count {{ var value: i32 }} fn tick(count: Count) -> bool {{ count.value += 1; true }} fn run(count: Count) -> i32 {{ {expression}; 0 }} fn main() -> i32 {{ val count = Count {{ value: 1 }}; run(count) + count.value }}"
            ),
            42,
        );
    }
}

#[test]
fn binary_termination_preserves_evaluation_order_and_short_circuit_paths() {
    for (expression, result) in [
        ("ARG + 7", 42),
        ("7 + ARG", 42),
        ("ARG < 7", 42),
        ("ARG == false", 42),
        ("true && ARG", 42),
        ("false || ARG", 42),
        ("false && ARG", 1),
        ("true || ARG", 1),
        ("ARG || tick(count)", 42),
    ] {
        let expression =
            expression.replace("ARG", "(if tick(count) { return 40; } else { return 0; })");
        execute_contextual_source(
            &format!(
                "struct Count {{ var value: i32 }} fn tick(count: Count) -> bool {{ count.value += 1; true }} fn run(count: Count) -> i32 {{ {expression}; 0 }} fn main() -> i32 {{ val count = Count {{ value: 1 }}; run(count) + count.value }}"
            ),
            result,
        );
    }
}

#[test]
fn terminating_reflection_values_preserve_effects_without_committing_writes() {
    for call in [
        r#"set_field(count, "value", VALUE)"#,
        "set_index(array, 0, VALUE)",
    ] {
        let body = call.replace("VALUE", "if tick(count) { return 30; } else { return 0; }");
        execute_contextual_source_with_writes(
            &format!(
                "struct Count {{ var value: i32 }} fn tick(count: Count) -> bool {{ count.value += 1; true }} fn run(count: Count, array: [i32]) -> i32 {{ {body}; 0 }} fn main() -> i32 {{ val count = Count {{ value: 1 }}; val array = [10]; run(count, array) + count.value + array[0] }}"
            ),
            42,
            true,
        );
    }
}

#[test]
fn terminating_indexes_skip_reads_rhs_effects_and_writes() {
    for statement in [
        "array[INDEX];",
        "tuple[INDEX];",
        "array[INDEX] = later(count);",
        "array[INDEX] += later(count);",
        "tuple[INDEX] = later(count);",
        "set_index(array, INDEX, later(count));",
    ] {
        let statement =
            statement.replace("INDEX", "if tick(count) { return 30; } else { return 0; }");
        execute_contextual_source_with_writes(
            &format!(
                "struct Count {{ var value: i32 }} fn tick(count: Count) -> bool {{ count.value += 1; true }} fn later(count: Count) -> i32 {{ count.value += 100; 99 }} fn run(count: Count, array: [i32]) -> i32 {{ var tuple = (1, true); {statement} 0 }} fn main() -> i32 {{ val count = Count {{ value: 1 }}; val array = [10]; run(count, array) + count.value + array[0] }}"
            ),
            42,
            true,
        );
    }
}

#[test]
fn terminating_match_scrutinees_skip_pattern_dispatch_and_arm_effects() {
    execute_contextual_source(
        "struct Count { var value: i32 } fn tick(count: Count) -> bool { count.value += 1; true } fn run(count: Count) -> i32 { match (if tick(count) { return 40; } else { return 0; }) { 1 => tick(count), _ => 7 }; 0 } fn main() -> i32 { val count = Count { value: 1 }; run(count) + count.value }",
        42,
    );
}

#[test]
fn terminating_if_conditions_produce_no_branch_result_or_effects() {
    for branches in ["{ tick(count) } else { 7 }", "{ tick(count) }"] {
        execute_contextual_source(
            &format!(
                "struct Count {{ var value: i32 }} fn tick(count: Count) -> bool {{ count.value += 1; true }} fn run(count: Count) -> i32 {{ if (if tick(count) {{ return 40; }} else {{ return 0; }}) {branches}; 0 }} fn main() -> i32 {{ val count = Count {{ value: 1 }}; run(count) + count.value }}"
            ),
            42,
        );
    }
}

#[test]
fn terminating_field_receivers_skip_member_layout_resolution() {
    for fields in [".value", ".value.other"] {
        execute_contextual_source(
            &format!(
                "struct Count {{ var value: i32 }} fn tick(count: Count) -> bool {{ count.value += 1; true }} fn run(count: Count) -> i32 {{ (if tick(count) {{ return 40; }} else {{ return 0; }}){fields}; 0 }} fn main() -> i32 {{ val count = Count {{ value: 1 }}; run(count) + count.value }}"
            ),
            42,
        );
    }
}

#[test]
fn terminating_index_receivers_skip_index_effects_and_reads() {
    for indexes in ["[later(count)]", "[later(count)][later(count)]"] {
        execute_contextual_source(
            &format!(
                "struct Count {{ var value: i32 }} fn tick(count: Count) -> bool {{ count.value += 1; true }} fn later(count: Count) -> i32 {{ count.value += 100; 0 }} fn run(count: Count) -> i32 {{ (if tick(count) {{ return 40; }} else {{ return 0; }}){indexes}; 0 }} fn main() -> i32 {{ val count = Count {{ value: 1 }}; run(count) + count.value }}"
            ),
            42,
        );
    }
}

#[test]
fn terminating_standard_receivers_skip_container_and_string_operations() {
    for call in [
        "std::array::len(ARG)",
        "std::map::len(ARG)",
        "std::set::len(ARG)",
        "std::string::len_bytes(ARG)",
        "std::option::is_some(ARG)",
        "std::result::is_ok(ARG)",
        "std::iter::len(ARG)",
        "std::array::push(ARG, tick(count))",
    ] {
        let expression = call.replace("ARG", "if tick(count) { return 40; } else { return 0; }");
        execute_contextual_source(
            &format!(
                "struct Count {{ var value: i32 }} fn tick(count: Count) -> bool {{ count.value += 1; true }} fn run(count: Count) -> i32 {{ {expression}; 0 }} fn main() -> i32 {{ val count = Count {{ value: 1 }}; run(count) + count.value }}"
            ),
            42,
        );
    }
}

#[test]
fn terminating_reflection_receivers_skip_remaining_operands_and_accesses() {
    for call in [
        r#"get_field(BASE, "value")"#,
        r#"set_field(BASE, "value", tick(count))"#,
        "set_index(BASE, tick(count), tick(count))",
    ] {
        let call = call.replace("BASE", "if tick(count) { return 40; } else { return 0; }");
        execute_contextual_source_with_writes(
            &format!(
                "struct Count {{ var value: i32 }} fn tick(count: Count) -> bool {{ count.value += 1; true }} fn run(count: Count) -> i32 {{ {call}; 0 }} fn main() -> i32 {{ val count = Count {{ value: 1 }}; run(count) + count.value }}"
            ),
            42,
            true,
        );
    }
}

#[test]
fn terminating_callees_skip_explicit_argument_effects() {
    for member in ["", ".missing"] {
        execute_contextual_source(
            &format!(
                "struct Count {{ var value: i32 }} fn tick(count: Count) -> bool {{ count.value += 1; true }} fn run(count: Count) -> i32 {{ (if tick(count) {{ return 40; }} else {{ return 0; }}){member}(tick(count)); 0 }} fn main() -> i32 {{ val count = Count {{ value: 1 }}; run(count) + count.value }}"
            ),
            42,
        );
    }
}

#[test]
fn annotated_const_dependencies_execute_independently_of_declaration_order() {
    for declarations in [
        "const ANSWER: i32 = BASE + 2; const BASE: i32 = 40;",
        "const BASE: i32 = 40; const ANSWER: i32 = BASE + 2;",
        "const ANSWER = BASE + 2; const BASE: i32 = 40;",
    ] {
        execute_contextual_source(&format!("{declarations} fn main() -> i32 {{ ANSWER }}"), 42);
    }
}

#[test]
fn concrete_nested_struct_fields_execute_on_all_existing_routes() {
    execute_contextual_source(
        "struct Item { val value: i32 } struct Box<T> { var items: [T] } fn main() -> i32 { val box = Box<Item> { items: [Item { value: 1 }] }; box.items = [Item { value: 42 }]; box.items[0].value }",
        42,
    );
}
