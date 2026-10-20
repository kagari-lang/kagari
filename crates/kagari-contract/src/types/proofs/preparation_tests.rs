//! Work accounting and deterministic interruption of structural preparation.
use super::*;
use kagari_common::identity::map::DefinitionContext;

thread_local! {
    static WORK: Cell<(usize, Option<usize>)> = const { Cell::new((0, None)) };
}

pub(super) fn record(cancel: &CancellationToken) {
    WORK.with(|work| {
        let (count, stop) = work.get();
        work.set((count + 1, stop));
        if stop == Some(count + 1) {
            cancel.cancel();
        }
    });
}

fn empty_module<'a>() -> ProofModule<'a> {
    ProofModule {
        implementations: vec![],
        enumerations: vec![],
        contracts: vec![],
        native_declarations: vec![],
    }
}

#[test]
fn shared_graph_resources_prepare_once_and_poll_during_admission() {
    let context = DefinitionContext::new().unwrap();
    let root = context
        .intern_root(&ModuleIdentity::single_file("shared.kgr"))
        .unwrap();
    let id = context
        .intern_child(root, DefinitionKind::Trait, "Shared", 0)
        .unwrap();
    let contract = TraitDef {
        name: "Shared".into(),
        associated_consts: vec![],
        supertraits: vec![],
        generic_params: vec![],
        bounds: vec![],
        methods: vec![],
        associated_types: vec![],
        conversion_adapter: None,
    };
    for count in [1, 8, 32] {
        WORK.set((0, None));
        let mut modules = (0..count).map(|_| empty_module()).collect::<Vec<_>>();
        for module in &mut modules {
            module.contracts.push((id, &contract));
        }
        let cancel = CancellationToken::default();
        let graph = resources::ProgramProofs::new(modules, &context.snapshot(), &cancel).unwrap();
        for index in 0..count {
            let view = graph.view([0, index], &cancel).unwrap();
            assert_eq!(view.trait_contract(&id), Some(&contract));
        }
        assert_eq!(WORK.get().0, 1, "{count} dependent views");
    }
    // Interrupt after structural work has started, not just at admission entry.
    // Distinct valid declaration IDs reach multiple structural records.
    WORK.set((0, Some(2)));
    let cancel = CancellationToken::default();
    let mut module = empty_module();
    module.contracts = (0..32)
        .map(|index| {
            let id = context
                .intern_child(root, DefinitionKind::Trait, &format!("Trait{index}"), 0)
                .unwrap();
            (id, &contract)
        })
        .collect();
    let result = resources::ProgramProofs::new(vec![module], &context.snapshot(), &cancel);
    assert!(matches!(result, Err(TypeTransformError::Cancelled)));
    assert_eq!(WORK.get().0, 2);
    WORK.set((0, None));
}
