//! Bounded opaque admission through source-free verified occurrences and live calls.
use crate::{
    tests::native_boundary::{compile_program, support::round_trip_program},
    vm::Vm,
};
use kagari_bytecode::program::verified::VerifiedBytecodeProgram;
use kagari_runtime::{
    Runtime, RuntimeConfig,
    gc::GcHeapConfig,
    module::{Program, VerifiedProgram},
    native::{
        binding::NativeResult,
        builder::ModuleBuilder,
        declarations::{FunctionDecl, MethodDecl},
        module::NativeModule,
        payload::Opaque,
        typed::NativeContext,
        types::TypeRef,
        value_handle::ScriptValue,
    },
};
use kagari_stdlib::declarations::StandardDeclarations;
use std::slice;

kagari_runtime::native_data! {
    struct BoundedState { value: i32 }
}

fn registration() -> (NativeModule, TypeRef) {
    let mut module = ModuleBuilder::new(
        "example::bounded_native",
        &StandardDeclarations::default().catalog().unwrap(),
    )
    .unwrap();
    let marker = module.define_trait("Tag").unwrap().finish().unwrap();
    let view = module.define_trait("View").unwrap().finish().unwrap();
    let mut declaration = module.define_opaque::<BoundedState>("Holder").unwrap();
    let element = declaration.type_parameter("Element").unwrap();
    declaration
        .bound(element.ty(), marker.apply([]).unwrap().ty())
        .unwrap();
    let holder = declaration.finish().unwrap();
    module
        .implement(holder.clone(), |implementation| {
            let receiver = implementation.receiver();
            let element = implementation.parameter("Element")?.ty();
            implementation.inherent_impl(|methods| {
                for (name, value) in [("new", 11), ("from_value", 17)] {
                    let function = methods
                        .define_method(MethodDecl::static_method(name).returns(receiver.clone()))?;
                    methods.method(&function, |signature| {
                        signature.bound(element.clone(), marker.apply([])?)
                    })?;
                    methods.bind(
                        function,
                        move |cx: &mut NativeContext<'_>,
                              (): ()|
                              -> NativeResult<Opaque<BoundedState>> {
                            let ty = cx.result_native_type()?;
                            cx.collect_garbage()?;
                            ty.create(cx, BoundedState { value }, &[])
                        },
                    )?;
                }
                Ok(())
            })?;
            implementation.trait_impl(view.apply([])?, |_| Ok(()))
        })
        .unwrap();
    let adapted = module
        .define_function(FunctionDecl::new("adapted").returns(view.apply([]).unwrap().ty()))
        .unwrap();
    module
        .function(&adapted, |signature| {
            let element = signature.type_parameter("Element")?;
            signature.bound(element.ty(), marker.apply([])?)?;
            signature.produces(holder.apply([element.ty()])?)
        })
        .unwrap();
    module
        .bind(
            adapted,
            |cx: &mut NativeContext<'_>, (): ()| -> NativeResult<Opaque<BoundedState>> {
                cx.collect_garbage()?;
                cx.result_native_type()?
                    .create(cx, BoundedState { value: 23 }, &[])
            },
        )
        .unwrap();
    (module.finish().unwrap(), holder)
}

fn load(vm: &mut Vm, source: &str, module: &NativeModule) -> Program {
    let metadata = round_trip_program(compile_program(source, Some(module)));

    let bytecode = VerifiedBytecodeProgram::from_metadata(metadata, &Default::default()).unwrap();
    let occurrences = bytecode
        .native_applications()
        .iter()
        .filter(|witness| {
            bytecode
                .definitions()
                .resolve(witness.nominal().declaration)
                .unwrap()
                .last()
                .is_some_and(|segment| segment.name == "Holder")
        })
        .collect::<Vec<_>>();
    assert!(occurrences.iter().any(|witness| witness.is_closed()));
    assert!(occurrences.iter().any(|witness| witness.binder().is_some()));
    // Preserve the seal into a different runtime definition context, exercising
    // VerifiedProgram's identity-preserving witness normalization, not reissuance.

    let verified = VerifiedProgram::from_bytecode(bytecode);
    assert!(!verified.native_applications().is_empty());
    vm.runtime_mut()
        .load_verified_program("bounded", verified)
        .unwrap()
}

fn fixture(source: &str, module: &NativeModule) -> (Vm, Program) {
    let mut runtime = Runtime::new(RuntimeConfig {
        gc: GcHeapConfig {
            collection_threshold: Some(1),
            ..Default::default()
        },
        ..Default::default()
    });
    NativeModule::install_all(&kagari_stdlib::modules().unwrap(), &mut runtime).unwrap();
    module.install(&mut runtime).unwrap();
    let mut vm = Vm::new(runtime);
    let program = load(&mut vm, source, module);
    (vm, program)
}

const SOURCE: &str = r#"
    use example::bounded_native::{Holder, Tag};
    pub struct Key { pub val value: i32 }
    impl Tag for Key {}
    pub struct Other {}
    impl Tag for Other {}
    pub struct OpenKey {}
    impl Tag for OpenKey {}
    pub struct Unseen {}
    impl Tag for Unseen {}
    pub fn direct() -> Holder<Key> { Holder::new::<Key>() }
    pub fn other() -> Holder<Other> { Holder::from_value::<Other>() }
    trait Factory { fn make<T: Tag>(self) -> Holder<T>; }
    impl Factory for i32 {
        fn make<T: Tag>(self) -> Holder<T> { Holder::new::<T>() }
    }
    pub fn indirect() -> Holder<OpenKey> {
        val factory: Factory = 0;
        factory.make::<OpenKey>()
    }
    pub fn signature_only(value: Holder<Unseen>) -> Holder<Unseen> { value }
"#;

#[test]
fn bounded_native_host_construction_requires_closed_type_and_separate_callable_evidence() {
    let (module, holder) = registration();
    let (vm, program) = fixture(SOURCE, &module);
    let runtime = vm.runtime();
    let owner = runtime.select_program(&program).unwrap();
    let key = runtime.bind_type(&program.root(), "Key", &[]).unwrap();
    let native = runtime
        .bind_native_type::<BoundedState>(
            &program.root(),
            &holder,
            slice::from_ref(key.type_argument()),
        )
        .unwrap();
    let constructor = runtime
        .bind_associated_function::<(), Opaque<BoundedState>>(&native, "new")
        .unwrap();
    let mut cx = vm.context(&owner).unwrap();
    let prepared = constructor.prepare_call(runtime).unwrap();
    let object: Opaque<BoundedState> = prepared
        .call_with_options(&mut cx, (), Default::default())
        .unwrap();
    cx.collect_garbage().unwrap();
    assert_eq!(object.read_data(&cx, |state| Ok(state.value)).unwrap(), 11);

    let other = runtime.bind_type(&program.root(), "Other", &[]).unwrap();
    let other_type = runtime
        .bind_native_type::<BoundedState>(
            &program.root(),
            &holder,
            slice::from_ref(other.type_argument()),
        )
        .unwrap();
    assert!(
        runtime
            .bind_associated_function::<(), Opaque<BoundedState>>(&other_type, "new")
            .is_err()
    );
    let from = runtime
        .bind_associated_function::<(), Opaque<BoundedState>>(&other_type, "from_value")
        .unwrap();
    assert_eq!(
        from.call(&mut cx, ())
            .unwrap()
            .read_data(&cx, |state| Ok(state.value))
            .unwrap(),
        17
    );

    for name in ["OpenKey", "Unseen"] {
        let ty = runtime.bind_type(&program.root(), name, &[]).unwrap();
        assert!(
            runtime
                .bind_native_type::<BoundedState>(
                    &program.root(),
                    &holder,
                    slice::from_ref(ty.type_argument())
                )
                .is_err(),
            "{name}"
        );
    }
    // This result comes from an open native occurrence instantiated by the checked
    // generic caller, and its Opaque conversion recovers admission from the object.
    let indirect: Opaque<BoundedState> = vm.execute_typed(&owner, "indirect", ()).unwrap();
    assert_eq!(
        indirect.read_data(&cx, |state| Ok(state.value)).unwrap(),
        11
    );
    assert!(
        runtime
            .bind_associated_function::<(), Opaque<BoundedState>>(
                &indirect.prepare_native_type(&cx).unwrap(),
                "new"
            )
            .is_err()
    );
    assert!(
        runtime
            .bind_native_type::<i64>(
                &program.root(),
                &holder,
                slice::from_ref(key.type_argument())
            )
            .is_err()
    );
}

#[test]
fn bounded_native_witness_cannot_borrow_foreign_runtime_or_argument_layout_scope() {
    let (module, holder) = registration();
    let (mut vm, old) = fixture(SOURCE, &module);
    let (foreign_vm, foreign) = fixture(SOURCE, &module);
    let foreign_key = foreign_vm
        .runtime()
        .bind_type(&foreign.root(), "Key", &[])
        .unwrap();
    assert!(
        vm.runtime()
            .bind_native_type::<BoundedState>(
                &old.root(),
                &holder,
                slice::from_ref(foreign_key.type_argument())
            )
            .is_err()
    );
    let newer = load(
        &mut vm,
        &SOURCE.replace("value: i32", "value: i64"),
        &module,
    );
    let new_key = vm.runtime().bind_type(&newer.root(), "Key", &[]).unwrap();
    assert!(
        vm.runtime()
            .bind_native_type::<BoundedState>(
                &old.root(),
                &holder,
                slice::from_ref(new_key.type_argument())
            )
            .is_err()
    );
    assert!(
        vm.runtime()
            .bind_native_type::<BoundedState>(
                &newer.root(),
                &holder,
                slice::from_ref(new_key.type_argument())
            )
            .is_ok()
    );
}

#[test]
fn bounded_native_result_adapter_consumes_concrete_import_witness() {
    let (module, holder) = registration();
    let (vm, program) = fixture(
        r#"
        use example::bounded_native::{Holder, Tag, View, adapted};
        pub struct Key {}
        impl Tag for Key {}
        pub fn make() -> View { adapted::<Key>() }
        pub struct OpenKey {}
        impl Tag for OpenKey {}
        trait Factory { fn make<T: Tag>(self) -> View; }
        impl Factory for i32 {
            fn make<T: Tag>(self) -> View { adapted::<T>() }
        }
        pub fn indirect() -> View {
            val factory: Factory = 0;
            factory.make::<OpenKey>()
        }
    "#,
        &module,
    );
    let owner = vm.runtime().select_program(&program).unwrap();
    let _: ScriptValue = vm.execute_typed(&owner, "make", ()).unwrap();
    let _: ScriptValue = vm.execute_typed(&owner, "indirect", ()).unwrap();
    let open_key = vm
        .runtime()
        .bind_type(&program.root(), "OpenKey", &[])
        .unwrap();
    assert!(
        vm.runtime()
            .bind_native_type::<BoundedState>(
                &program.root(),
                &holder,
                slice::from_ref(open_key.type_argument()),
            )
            .is_err()
    );
    let key = vm.runtime().bind_type(&program.root(), "Key", &[]).unwrap();
    let native = vm
        .runtime()
        .bind_native_type::<BoundedState>(
            &program.root(),
            &holder,
            slice::from_ref(key.type_argument()),
        )
        .unwrap();
    assert!(
        vm.runtime()
            .bind_associated_function::<(), Opaque<BoundedState>>(&native, "new")
            .is_err()
    );
}
