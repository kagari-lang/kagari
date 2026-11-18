//! Attribution of HIR nodes without pretending registration has authored byte ranges.
use crate::{
    hir::{
        ids::{FieldId, GenericParamId, ParamId, TraitMethodId, VariantId},
        item::Item,
        origin::NodeOrigin,
    },
    source_map::SourceMap,
};
use kagari_common::{identity::table::DefinitionId, span::Span};

/// A node in the matching lowering. Plain declaration IDs require the matching map.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum OriginTarget {
    Item(Item),
    Field(FieldId),
    Variant(VariantId),
    GenericParam(GenericParamId),
    TraitMethod(TraitMethodId),
    Param(ParamId),
}

impl SourceMap {
    /// Whether this map describes registration records rather than an authored file.
    pub fn is_structured(&self) -> bool {
        self.structured_input
    }

    pub(crate) fn mark_structured_input(&mut self) {
        self.structured_input = true;
    }

    pub(crate) fn structured_definitions(&self) -> impl Iterator<Item = &DefinitionId> {
        self.structured_origins.values()
    }

    /// Returns real source attribution or the structured record responsible for a node.
    pub fn origin(&self, target: OriginTarget) -> NodeOrigin {
        if let Some(declaration) = self.structured_origins.get(&target) {
            return NodeOrigin::Structured {
                declaration: *declaration,
            };
        }
        let span = match target {
            OriginTarget::Item(id) => self.item_span(id),
            OriginTarget::Field(id) => self.field_span(id),
            OriginTarget::Variant(id) => self.variant_span(id),
            OriginTarget::GenericParam(id) => self.generic_param_span(id),
            OriginTarget::TraitMethod(id) => self.trait_method_span(id),
            OriginTarget::Param(id) => self.param_span(id),
        };
        NodeOrigin::Source(span)
    }

    /// Optional authored byte surface. Legacy span accessors retain empty diagnostic
    /// anchors for structured nodes; those anchors must not be used for navigation.
    pub fn source_span(&self, target: OriginTarget) -> Option<Span> {
        match self.origin(target) {
            NodeOrigin::Source(span) => Some(span),
            NodeOrigin::Structured { .. } => None,
        }
    }

    pub(crate) fn node_origin(&self, span: Span) -> NodeOrigin {
        match self.structured_owner {
            Some(declaration) => NodeOrigin::Structured { declaration },
            None => NodeOrigin::Source(span),
        }
    }

    pub(crate) fn set_structured_origin(&mut self, declaration: DefinitionId) {
        self.structured_owner = Some(declaration);
    }

    pub(crate) fn record_origin(&mut self, target: OriginTarget) {
        if let Some(declaration) = self.structured_owner {
            assert!(
                self.structured_origins
                    .insert(target, declaration)
                    .is_none()
            );
        }
    }
}
