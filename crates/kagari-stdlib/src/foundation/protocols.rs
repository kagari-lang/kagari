//! Canonical standard trait schemas; required methods use ordinary MethodDecl lowering.
mod collections;
use crate::{catalog::documentation, foundation::interface, namespaces};
use kagari_common::identity::{DefinitionKind, table::DefinitionId};

use kagari_runtime::native::{
    assembly::{draft::ModuleDraft, references::DraftReference},
    binding::NativeResult,
    declarations::MethodDecl,
    types::Type,
};
use kagari_types::{
    declaration::FnDecl,
    ty::{NominalTy, Ty},
};

type TraitInventory = (&'static str, usize, (&'static str, &'static str));
type ModuleInventory = (&'static str, &'static str, &'static [TraitInventory]);

pub(super) const INVENTORY: &[ModuleInventory] = &[(
    "std",
    "collections",
    &[
        (
            "List",
            1,
            (
                "A readonly sequence interface. Element types are invariant. A readonly view retains shared identity, so another alias can mutate the underlying sequence.",
                "fn first(values: std::collections::List<i32>) -> Option<i32> { values.get(0) }",
            ),
        ),
        (
            "MutableList",
            1,
            (
                "A mutable sequence interface. Structural edits validate active borrows and invalidate existing cursors. Completed writes and callback effects survive later failures.",
                "fn append(values: std::collections::MutableList<i32>) { values.push(3); }",
            ),
        ),
        (
            "Map",
            2,
            (
                "A readonly key/value interface. Lookup returns `None` for a missing key. Readonly views share storage with other aliases.",
                "fn find(values: std::collections::Map<i32, String>) -> Option<String> { values.get(1) }",
            ),
        ),
        (
            "MutableMap",
            2,
            (
                "A mutable key/value interface. Insert replaces the value of an equal key. Hash and equality callbacks must not structurally mutate a collection under lookup.",
                "fn put(values: std::collections::MutableMap<i32, String>) { values.insert(1, \"one\"); }",
            ),
        ),
        (
            "Set",
            1,
            (
                "A readonly membership interface. Equal values represent the same member. Readonly views share storage with other aliases.",
                "fn has(values: std::collections::Set<i32>) -> bool { values.contains(1) }",
            ),
        ),
        (
            "MutableSet",
            1,
            (
                "A mutable membership interface. Insertion retains one representative of each equal value. Structural edits honor active borrow and lookup guards.",
                "fn add(values: std::collections::MutableSet<i32>) { values.insert(1); }",
            ),
        ),
    ],
)];

struct Contract {
    name: &'static str,
    arity: usize,
    overview: (&'static str, &'static str),
    documents: Vec<(DefinitionId, String)>,
    owner: DraftReference,
    methods: Vec<FnDecl>,
    supertraits: Vec<NominalTy>,
}

impl Contract {
    fn new(
        module: &mut ModuleDraft<'_>,
        name: &'static str,
        arity: usize,
        overview: (&'static str, &'static str),
    ) -> NativeResult<Self> {
        Ok(Self {
            name,
            arity,
            overview,
            documents: vec![],
            owner: interface(module, name, arity)?,
            methods: vec![],
            supertraits: vec![],
        })
    }

    fn method(
        &mut self,
        module: &ModuleDraft<'_>,
        declaration: MethodDecl,
        description: &str,
    ) -> NativeResult<()> {
        let declaration = module.trait_method(self.owner.id(), declaration)?;
        let id = module.member(self.owner.id(), DefinitionKind::Method, &declaration.name)?;
        let text = documentation::example(
            &format!(
                "{description}\n\nSee `{}` for the behavioral contract and examples.",
                self.name
            ),
            self.overview.1,
        );
        self.documents.push((id, text));
        self.methods.push(declaration);
        Ok(())
    }

    fn parent(&mut self, interface: Type) {
        self.supertraits.push(nominal(interface));
    }

    fn finish(self, module: &mut ModuleDraft<'_>) -> NativeResult<()> {
        module.document(self.owner.id(), documentation::markdown(self.overview))?;
        for (id, text) in self.documents {
            module.document(id, text)?;
        }
        module.define_trait(self.name, self.arity, |record, _| {
            record.methods = self.methods;
            record.supertraits = self.supertraits;
            Ok(())
        })?;
        Ok(())
    }
}

fn nominal(interface: Type) -> NominalTy {
    let Ty::Trait(interface) = interface.abi().clone() else {
        unreachable!("trait application")
    };
    interface
}

pub(super) fn declare(module: &mut ModuleDraft<'_>) -> NativeResult<()> {
    for &(package, path, declarations) in INVENTORY {
        if module.identity() != &namespaces::module(package, path) {
            continue;
        }
        for &(name, arity, overview) in declarations {
            let mut contract = Contract::new(module, name, arity, overview)?;
            collections::configure(module, &mut contract)?;
            contract.finish(module)?;
        }
    }
    Ok(())
}
