use kagari_common::SourceFile;
use kagari_embed::{ArtifactOptions, EmbeddingError, KagariEngine};

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
