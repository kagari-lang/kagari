//! Declaration policy sites and module identities, without interpreting tool policy.

use crate::{
    analysis::FileAnalysis,
    hir::{
        attribute::{AttributeFact, AttributeTarget},
        item::Item,
    },
};
use kagari_common::{identity::ModuleIdentity, span::Span};

/// One attribute, its full declaration/member range, and optional child-module scope.
/// The typed target remains on `attribute`; ranges serve lexical containment only.
pub struct ScopedAttribute<'a> {
    pub attribute: &'a AttributeFact,
    pub declaration: Span,
    pub module: Option<ModuleIdentity>,
}

impl FileAnalysis {
    /// Returns this logical module's attributes, excluding inline child contents.
    /// A `mod child;` scope carries the same module identity as `child.kgr` in the
    /// source graph. Consumers must not infer external inheritance from byte spans.
    pub fn scoped_attributes(&self) -> Vec<ScopedAttribute<'_>> {
        let lowered = &self.result().facts().lowered;
        lowered
            .module
            .attributes
            .iter()
            .map(|attribute| {
                let module = if let AttributeTarget::Item(Item::Module(id)) = attribute.target {
                    let child = &lowered.module.modules[id.index()];
                    assert_eq!(child.id, id, "module declaration slot mismatch");
                    let mut identity = self.source().module_identity().clone();
                    identity.path.push(child.name.clone());
                    Some(identity)
                } else {
                    None
                };
                ScopedAttribute {
                    attribute,
                    declaration: lowered.source_map.attribute_target_span(attribute.target),
                    module,
                }
            })
            .collect()
    }
}
