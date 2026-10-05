use super::fixture;
use crate::compile_program;
use kagari_runtime::{
    native::{
        binding::NativeResult,
        builder::ModuleBuilder,
        declarations::{CallableRequirement, FunctionDecl, MethodDecl},
        objects::Object,
        registration::FunctionSpec,
        selected_method::SelectedMethod,
        typed::NativeContext,
        types::Type,
    },
    session::ExecutionPhase,
};
use kagari_stdlib::declarations::StandardDeclarations;
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicUsize, Ordering},
};

type Read = SelectedMethod<(Object, i32), i32>;
type Twice = SelectedMethod<(Object,), i32>;
type Base = SelectedMethod<(), i32>;

#[test]
fn selected_handles_retain_generic_default_and_static_calls_across_gc_and_reload() {
    let mut module = ModuleBuilder::new(
        "example::selected",
        &StandardDeclarations::default().catalog().unwrap(),
    );
    let effects = Arc::new(AtomicUsize::new(0));
    let observed = effects.clone();
    module
        .add_function(
            FunctionSpec::new("tick"),
            move |cx: &mut NativeContext<'_>, (): ()| -> NativeResult<()> {
                observed.fetch_add(1, Ordering::Relaxed);
                cx.collect_garbage()?;
                Ok(())
            },
        )
        .unwrap();
    let mut contract = module.define_trait("Read");
    let read = contract
        .define_method(
            MethodDecl::instance("read")
                .parameter("divisor", Type::i32())
                .returns(Type::i32()),
        )
        .unwrap();
    contract
        .define_method(MethodDecl::static_method("base").returns(Type::i32()))
        .unwrap();
    let twice = contract
        .define_method(MethodDecl::instance("twice").returns(Type::i32()))
        .unwrap();
    let operation = contract.operation(&read).unwrap();
    let required = contract
        .method(&twice, |method| Ok(method.requires(operation)))
        .unwrap();
    contract
        .bind_default_method(
            twice,
            move |cx: &mut NativeContext<'_>, receiver: Object, (): ()| -> NativeResult<i32> {
                let read = cx.selected_method::<(Object, i32), i32>(&required)?;
                Ok(read.call(cx, (receiver, 1))? * 2)
            },
        )
        .unwrap();
    let contract = contract.finish().unwrap();
    let wrong = module
        .define_function(FunctionDecl::new("wrong").returns(Type::i32()))
        .unwrap();
    let wrong_key = module
        .function(&wrong, |function| {
            let item = function.type_parameter("T")?.ty();
            function.parameter("item", item.clone());
            function.bound(item.clone(), contract.apply([]));
            Ok(function.requires(CallableRequirement::method(item, contract.method("read")?)))
        })
        .unwrap();
    module
        .bind_typed(
            wrong,
            |_: &mut NativeContext<'_>, (_item,): (Object,)| -> NativeResult<i32> { Ok(0) },
        )
        .unwrap();
    let capture = module
        .define_function(FunctionDecl::new("capture").returns(Type::i32()))
        .unwrap();
    let (read_key, twice_key, base_key) = module
        .function(&capture, |function| {
            let item = function.type_parameter("T")?.ty();
            function.parameter("item", item.clone());
            function.bound(item.clone(), contract.apply([]));
            Ok((
                function.requires(CallableRequirement::method(
                    item.clone(),
                    contract.method("read")?,
                )),
                function.requires(CallableRequirement::method(
                    item.clone(),
                    contract.method("twice")?,
                )),
                function.requires(CallableRequirement::method(item, contract.method("base")?)),
            ))
        })
        .unwrap();
    let retained: Arc<Mutex<Option<(Read, Twice, Base)>>> = Arc::default();
    let saved = retained.clone();
    module
        .bind_typed(
            capture,
            move |cx: &mut NativeContext<'_>, (receiver,): (Object,)| -> NativeResult<i32> {
                assert!(
                    cx.selected_method::<(Object, i32), i32>(&wrong_key)
                        .is_err()
                );
                assert!(cx.selected_method::<(Object,), i32>(&read_key).is_err());
                assert!(
                    cx.selected_method::<(Object, i32), bool>(&read_key)
                        .is_err()
                );
                let read = cx.selected_method::<(Object, i32), i32>(&read_key)?;
                let twice = cx.selected_method::<(Object,), i32>(&twice_key)?;
                let base = cx.selected_method::<(), i32>(&base_key)?;
                cx.collect_garbage()?;
                assert_eq!(base.call(cx, ())?, 20);
                let result = twice.call(cx, (receiver,))?;
                *saved.lock().unwrap() = Some((read, twice, base));
                Ok(result)
            },
        )
        .unwrap();
    let module = module.finish().unwrap();
    let source = r#"
        use example::selected::{Read, capture, tick};
        pub struct Counter<T> { pub val amount: i32, pub val marker: T }
        impl<T> Read for Counter<T> {
            fn read(self, divisor: i32) -> i32 { tick(); self.amount / divisor }
            fn base() -> i32 { 20 }
        }
        trait Relay {
            fn relay<T: Read>(self, item: T) -> i32 { capture(item) }
        }
        impl Relay for i32 {}
        pub fn make() -> Counter<i32> { Counter { amount: 21, marker: 7 } }
        pub fn wrong_receiver() -> Counter<bool> { Counter { amount: 21, marker: true } }
        pub fn run() -> i32 { val relay: Relay = 0; relay.relay(make()) }
    "#;
    let (vm, old) = fixture(source, Some(&module));
    let run = vm.runtime().bind_function::<(), i32>(&old, "run").unwrap();
    let make = vm
        .runtime()
        .bind_function::<(), Object>(&old, "make")
        .unwrap();
    assert_eq!(vm.call(&run, ()).unwrap(), 42);
    let (read, twice, base) = retained.lock().unwrap().take().unwrap();
    let receiver = vm.call(&make, ()).unwrap();
    vm.runtime().collect_garbage().unwrap();
    let mut cx = vm.context(&old).unwrap();
    assert_eq!(read.call(&mut cx, (receiver.clone(), 1)).unwrap(), 21);
    let before = effects.load(Ordering::Relaxed);
    assert!(read.call(&mut cx, (receiver.clone(), 0)).is_err());
    assert_eq!(effects.load(Ordering::Relaxed), before + 1);
    assert_eq!(twice.call(&mut cx, (receiver.clone(),)).unwrap(), 42);
    assert_eq!(base.call(&mut cx, ()).unwrap(), 20);
    drop(cx);
    let updated = source.replace("self.amount / divisor", "(self.amount + 1) / divisor");
    let staged = vm
        .runtime()
        .stage_reload_program(&old, "functions", compile_program(&updated, Some(&module)))
        .unwrap();
    {
        let mut cx = vm.context(staged.module()).unwrap();
        let mut options = vm.runtime().execution_options();
        options.phase = ExecutionPhase::CandidateInitialization;
        let _session = vm
            .runtime()
            .begin_candidate_initialization(&staged)
            .unwrap();
        assert!(base.call_with_options(&mut cx, (), options).is_err());
    }
    drop(staged);
    let published = vm
        .reload_program(&old, "functions", compile_program(&updated, Some(&module)))
        .unwrap();
    let new_run = vm
        .runtime()
        .bind_function::<(), i32>(&published, "run")
        .unwrap();
    assert_eq!(vm.call(&new_run, ()).unwrap(), 44);
    let (_, new_twice, _) = retained.lock().unwrap().take().unwrap();
    let mut cx = vm.context(&published).unwrap();
    assert_eq!(twice.call(&mut cx, (receiver.clone(),)).unwrap(), 42);
    // Arguments follow the existing layout-compatibility rules across reload;
    // the operation still executes the exact version selected at preparation.
    assert_eq!(new_twice.call(&mut cx, (receiver.clone(),)).unwrap(), 44);
    let wrong_receiver: Object = vm.execute_typed(&published, "wrong_receiver", ()).unwrap();
    let before = effects.load(Ordering::Relaxed);
    assert!(new_twice.call(&mut cx, (wrong_receiver,)).is_err());
    assert_eq!(effects.load(Ordering::Relaxed), before);
    let (foreign, foreign_owner) = fixture("pub fn main() {}", None);
    assert!(
        base.call(&mut foreign.context(&foreign_owner).unwrap(), ())
            .is_err()
    );
    drop(cx);
    drop((run, make, receiver, read, twice));
    assert!(
        !vm.runtime()
            .collect_garbage()
            .unwrap()
            .reclaimed_modules
            .contains(&old.key())
    );
    drop(base);
    assert!(
        vm.runtime()
            .collect_garbage()
            .unwrap()
            .reclaimed_modules
            .contains(&old.key())
    );
}

#[test]
fn typed_generic_native_selected_member_keeps_applied_object_results_and_rejects_other_shapes() {
    let mut module = ModuleBuilder::new(
        "example::selected_generic",
        &StandardDeclarations::default().catalog().unwrap(),
    );
    let mut contract = module.define_trait("Identity");
    let method = contract
        .define_method(MethodDecl::instance("identity"))
        .unwrap();
    contract
        .method(&method, |method| {
            let item = method.type_parameter("T")?.ty();
            method.parameter("item", item.clone());
            method.returns(item);
            Ok(())
        })
        .unwrap();
    let contract = contract.finish().unwrap();
    let invoke = module.define_function(FunctionDecl::new("invoke")).unwrap();
    let key = module
        .function(&invoke, |function| {
            let source = function.type_parameter("S")?.ty();
            let item = function.type_parameter("T")?.ty();
            function.parameter("source", source.clone());
            function.parameter("item", item.clone());
            function.returns(item.clone());
            function.bound(source.clone(), contract.apply([]));
            Ok(function.requires(
                CallableRequirement::method(source, contract.method("identity")?).arguments([item]),
            ))
        })
        .unwrap();
    let effects = Arc::new(AtomicUsize::new(0));
    let observed = effects.clone();
    module
        .bind_typed(
            invoke,
            move |cx: &mut NativeContext<'_>,
                  (source, item): (Object, Object)|
                  -> NativeResult<Object> {
                observed.fetch_add(1, Ordering::Relaxed);
                let operation = cx.selected_method::<(Object, Object), Object>(&key)?;
                cx.collect_garbage()?;
                let result = operation.call(cx, (source, item))?;
                cx.collect_garbage()?;
                Ok(result)
            },
        )
        .unwrap();
    let module = module.finish().unwrap();
    let (vm, owner) = fixture(
        r#"
        use example::selected_generic::{Identity, invoke};
        struct Source { var calls: i32 }
        impl Identity for Source {
            fn identity<T>(self, item: T) -> T { self.calls += 1; item }
        }
        trait Relay {
            fn relay<S: Identity, T>(self, source: S, item: T) -> T { invoke(source, item) }
        }
        impl Relay for i32 {}
        pub struct Item { pub var value: i32 }
        pub fn run() -> Item {
            val relay: Relay = 0;
            val item = Item { value: 42 };
            val result = relay.relay(Source { calls: 0 }, item);
            result.value = 43;
            item
        }
        pub fn wrong() -> String {
            val relay: Relay = 0;
            relay.relay(Source { calls: 0 }, "wrong")
        }
    "#,
        Some(&module),
    );
    let run = vm
        .runtime()
        .bind_function::<(), Object>(&owner, "run")
        .unwrap();
    let wrong = vm
        .runtime()
        .bind_function::<(), String>(&owner, "wrong")
        .unwrap();
    let item = vm.call(&run, ()).unwrap();
    let ty = vm.runtime().bind_type(&owner, "Item", &[]).unwrap();
    let value = vm.runtime().bind_field::<i32>(&ty, "value").unwrap();
    vm.runtime().collect_garbage().unwrap();
    assert_eq!(
        item.get(&mut vm.context(&owner).unwrap(), &value).unwrap(),
        43
    );
    assert!(vm.call(&wrong, ()).is_err());
    assert_eq!(effects.load(Ordering::Relaxed), 1);
    assert_eq!(vm.runtime().resources().counters().current_call_depth, 0);
    drop(item);
    assert_eq!(vm.runtime().collect_garbage().unwrap().live_objects, 0);
}

#[test]
fn selected_primitive_operations_use_checked_typed_arguments() {
    let declarations = StandardDeclarations::default();
    let mut module = ModuleBuilder::new("example::selected_hash", &declarations.catalog().unwrap());
    let hash = declarations.hash();
    let function = module
        .define_function(FunctionDecl::new("repeat").returns(Type::i64()))
        .unwrap();
    let key = module
        .function(&function, |function| {
            let item = function.type_parameter("T")?.ty();
            function.parameter("item", item.clone());
            function.bound(item.clone(), hash.apply([]));
            Ok(function.requires(CallableRequirement::method(item, hash.method("hash")?)))
        })
        .unwrap();
    module
        .bind_typed(
            function,
            move |cx: &mut NativeContext<'_>, (item,): (i32,)| -> NativeResult<i64> {
                let operation = cx.selected_method::<(i32,), i64>(&key)?;
                let result = operation.call(cx, (item,))?;
                cx.collect_garbage()?;
                assert_eq!(operation.call(cx, (item,))?, result);
                Ok(result)
            },
        )
        .unwrap();
    let module = module.finish().unwrap();
    let (vm, owner) = fixture(
        r#"
        use example::selected_hash::repeat;
        pub fn run() -> bool { repeat(42) == 42.hash() }
    "#,
        Some(&module),
    );
    assert!(vm.execute_typed::<_, bool>(&owner, "run", ()).unwrap());
    assert_eq!(vm.runtime().gc().active_roots(), 0);
}
