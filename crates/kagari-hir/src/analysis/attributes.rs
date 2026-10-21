//! Declaration policy sites and module identities, without interpreting tool policy.

use crate::{analysis::FileAnalysis, lower::AttributeFact};
use kagari_common::{identity::ModuleIdentity, span::Span};

/// A declaration site in this file or the identity of a declared child module.
#[derive(Debug, Clone)]
pub enum AttributeTarget {
    Declaration(Span),
    Module {
        identity: ModuleIdentity,
        declaration: Span,
    },
}

/// One attribute and its HIR-owned declaration/module scope.
pub struct ScopedAttribute<'a> {
    pub attribute: &'a AttributeFact,
    pub target: AttributeTarget,
}

impl FileAnalysis {
    /// Returns this logical module's attributes, excluding inline child contents.
    /// A `mod child;` scope carries the same module identity as `child.kgr` in the
    /// source graph. Consumers must not infer external inheritance from byte spans.
    pub fn scoped_attributes(&self) -> Vec<ScopedAttribute<'_>> {
        let lowered = &self.result().facts().lowered;
        lowered
            .attributes
            .iter()
            .map(|attribute| {
                let child = lowered.module.modules.iter().find(|module| {
                    lowered.source_map.module_span(module.id) == attribute.target_span
                });
                let target = if let Some(child) = child {
                    let mut identity = self.source().module_identity().clone();
                    identity.path.push(child.name.clone());
                    AttributeTarget::Module {
                        identity,
                        declaration: attribute.target_span,
                    }
                } else {
                    AttributeTarget::Declaration(attribute.target_span)
                };
                ScopedAttribute { attribute, target }
            })
            .collect()
    }
}
