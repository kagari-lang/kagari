//! Execute the selected native operation carried by an implementation-local fact.
use super::{owner, path};
use crate::{
    Runtime,
    frame::driver::ReturnAction,
    module::LoadedModule,
    native::{
        assembly::{
            ModuleDefinition, ModuleSetBuilder,
            authoring::PendingBinding,
            conflict,
            draft::{TraitImplementation, traits::TraitOverride},
        },
        binding::{Codec, NativeBinding, NativeResult},
        catalog::DeclarationCatalog,
        context::ScriptCall,
        declarations::{MethodDecl, SelectedCall},
        module::NativeModule,
        typed::NativeContext,
        types::{Type, same_definition},
    },
    value::Value,
};
use kagari_bytecode::{instruction::NativeImportId, module::CallableTarget};
use kagari_common::identity::DefinitionKind;
use kagari_compiler::{bytecode::lower_program_to_bytecode, source::program::lower_program_to_mir};
use kagari_hir::analysis::AnalysisDatabase;
use kagari_source::source_database::{SourceDatabase, SourceLayer};
use std::{
    cell::Cell,
    rc::Rc,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
};

fn invoke_selected(
    runtime: &Runtime,
    _: &LoadedModule,
    target: ScriptCall<'_>,
    arguments: &[Value],
) -> NativeResult<Value> {
    let ScriptCall::Pinned(function) = target else {
        panic!("pinned native operation")
    };
    let stack = runtime.enter_pinned_execution_stack(function)?;
    stack.push_pinned_call(runtime, function, arguments)?;
    stack.start_native_entry(runtime, invoke_selected)?;
    let ReturnAction::Complete(result) = stack.finish_native_return(runtime)? else {
        panic!("selected return")
    };
    Ok(result)
}

#[test]
fn same_batch_override_executes_its_compiler_selected_requirement_in_both_orders() {
    for reverse in [false, true] {
        let prepares = Rc::new(Cell::new(0));
        let factory_count = prepares.clone();
        let calls = Arc::new(AtomicUsize::new(0));
        let callback_count = calls.clone();
        let contracts = ModuleDefinition::new(owner("calls"), |module| {
            for (name, method) in [("Base", "base"), ("Answer", "answer")] {
                let id = module.definition(DefinitionKind::Trait, name)?;
                let method =
                    module.trait_method(id, MethodDecl::instance(method).returns(Type::i32()))?;
                module.define_trait(name, 0, |record, _| {
                    record.methods.push(method);
                    Ok(())
                })?;
            }
            Ok(())
        });
        let implementations = ModuleDefinition::new(owner("call_impls"), move |module| {
            let base = module.reference_trait(path("calls", DefinitionKind::Trait, "Base"), 0)?;
            let base_method = module.reference_method(&base, "base", 0)?;
            let answer =
                module.reference_trait(path("calls", DefinitionKind::Trait, "Answer"), 0)?;
            let answer_method = module.reference_method(&answer, "answer", 0)?;
            module.trait_implementation(
                Type::i32(),
                base.apply([])?,
                vec![(
                    "base".into(),
                    PendingBinding::implementation(NativeBinding::new(
                        vec![Codec::Exact(Type::i32())],
                        Codec::Exact(Type::i32()),
                        move |cx| {
                            callback_count.fetch_add(1, Ordering::SeqCst);
                            let Value::I32(value) = cx.argument(0)? else {
                                return Err(conflict("i32 receiver"));
                            };
                            Ok(Value::I32(value + 2))
                        },
                    )),
                )],
            )?;
            let requirement = base_method.requirement(answer.receiver()?, base.apply([])?, [])?;
            let entry = TraitOverride::new(
                answer_method,
                PendingBinding::implementation_with(move |preparation| {
                    factory_count.set(factory_count.get() + 1);
                    let declaration = preparation.declaration();
                    let token = SelectedCall::from_declaration(
                        declaration,
                        &declaration.records().callable_requirements[0],
                    )?;
                    Ok(NativeBinding::declared(
                        declaration,
                        move |cx: &mut NativeContext<'_>, (value,): (i32,)| {
                            cx.selected_method::<(i32,), i32>(&token)?
                                .call(cx, (value,))
                        },
                    ))
                }),
            )
            .requires(requirement);
            module.trait_implementation_with(0, |_| {
                Ok(TraitImplementation {
                    receiver: Type::i32(),
                    interface: answer.apply([])?,
                    bounds: vec![],
                    bindings: vec![],
                }
                .with_overrides([entry]))
            })
        });
        let mut set = ModuleSetBuilder::new(&DeclarationCatalog::default()).unwrap();
        if reverse {
            set.add(implementations).unwrap();
            set.add(contracts).unwrap();
        } else {
            set.add(contracts).unwrap();
            set.add(implementations).unwrap();
        }
        let assembly = set.finish().unwrap();
        let snapshot = assembly.declarations();
        assert_eq!(prepares.get(), 0);
        assert_eq!(calls.load(Ordering::SeqCst), 0);
        let declaration = snapshot
            .catalog()
            .declarations
            .values()
            .find(|d| d.function.name == "answer")
            .unwrap();
        let answer = declaration.declaration;
        let definitions = snapshot.catalog().definitions();
        let modules = assembly.into_modules().unwrap();
        assert_eq!(prepares.get(), 1);
        let mut analysis = AnalysisDatabase::default();
        analysis.set_native_modules(snapshot.modules().iter().cloned().map(Arc::new).collect());
        let mut sources = SourceDatabase::default();
        let root = sources
            .set(
                "forward-call.kgr",
                "use forward::calls::Answer; fn main() -> i32 { 40.answer() }".into(),
                SourceLayer::Base,
            )
            .unwrap();
        let checked = analysis
            .snapshot(sources.snapshot(), &Default::default())
            .unwrap()
            .check_program(root, &Default::default())
            .unwrap();
        let mir = lower_program_to_mir(&checked, &Default::default()).unwrap();
        let bytecode = lower_program_to_bytecode(&mir).unwrap();
        let mut runtime = Runtime::default();
        NativeModule::install_all(&modules, &mut runtime).unwrap();
        let loaded = runtime.load_program("forward-call", bytecode).unwrap();
        let (owner, slot) = loaded
            .members()
            .find_map(|owner| {
                owner
                    .bytecode
                    .native_imports
                    .iter()
                    .position(|import| {
                        same_definition(
                            import.instance.declaration,
                            owner.definitions(),
                            answer,
                            &definitions,
                        )
                    })
                    .map(|slot| (owner, slot))
            })
            .expect("compiler selected override import");
        let stack = runtime.enter_execution_stack(&owner).unwrap();
        stack
            .push_callable(
                &runtime,
                owner.clone(),
                CallableTarget::Native(NativeImportId::new(slot)),
                &[Value::I32(40)],
                None,
            )
            .unwrap();
        stack.start_native_entry(&runtime, invoke_selected).unwrap();
        let ReturnAction::Complete(result) = stack.finish_native_return(&runtime).unwrap() else {
            panic!("override return")
        };
        assert_eq!(result, Value::I32(42));
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        assert_eq!(runtime.gc().active_roots(), 0);
    }
}
