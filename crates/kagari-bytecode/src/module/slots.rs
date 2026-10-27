//! Semantic slot type ownership, bounded tree sizes and contextual identity traversal.
use crate::module::BytecodeSemanticSlots;
use kagari_common::{
    cancellation::CancellationToken,
    identity::{
        mapping::{DefinitionMapper, DefinitionMappingError, check_cancel, map_entries},
        scoped::{DefinitionReferences, RemapDefinitions},
        table::DefinitionId,
    },
};
use kagari_types::ty::{
    NominalTy, Ty,
    substitution::{MAX_TYPE_DEPTH, MAX_TYPE_NODES},
};
use std::mem;

/// Bounded size of newly owned slot type trees, not an interning/cache table.
/// `root_depth = 2` preflights children before allocating a tuple/function root.
/// The byte bound includes Arc headers and pessimistic B-tree node slack.
#[derive(Clone, Copy)]
pub(crate) struct TypeShape {
    pub(crate) nodes: usize,
    pub(crate) bytes: usize,
}

impl TypeShape {
    pub(crate) fn measure<'a>(
        roots: impl IntoIterator<Item = &'a Ty>,
        root_depth: usize,
    ) -> Option<Self> {
        let mut pending = roots
            .into_iter()
            .take(MAX_TYPE_NODES + 1)
            .map(|ty| (ty, root_depth))
            .collect::<Vec<_>>();
        let mut nodes = usize::from(root_depth > 1);
        let mut overhead = 2 * mem::size_of::<usize>();
        if root_depth > 1 {
            overhead += pending.len().max(4) * mem::size_of::<Ty>();
        }
        while let Some((ty, depth)) = pending.pop() {
            nodes += 1;
            if nodes + pending.len() > MAX_TYPE_NODES || depth > MAX_TYPE_DEPTH {
                return None;
            }
            let mut nominal = None;
            match ty {
                Ty::Tuple(items) => {
                    overhead += (items.capacity() - items.len()) * mem::size_of::<Ty>();
                    pending.extend(items.iter().map(|ty| (ty, depth + 1)));
                }
                Ty::Function { params, result } => {
                    overhead += (params.capacity() - params.len()) * mem::size_of::<Ty>();
                    pending.extend(params.iter().map(|ty| (ty, depth + 1)));
                    pending.push((result, depth + 1));
                }
                Ty::Iter(item)
                | Ty::Range(item, _)
                | Ty::Array(item)
                | Ty::Reference { target: item, .. } => pending.push((item, depth + 1)),

                Ty::Struct(value)
                | Ty::NativeObject(value)
                | Ty::Enum(value)
                | Ty::Trait(value) => nominal = Some(value),
                Ty::Projection {
                    arguments,
                    receiver,
                    interface,
                    ..
                } => {
                    overhead += (arguments.capacity() - arguments.len()) * mem::size_of::<Ty>();
                    pending.extend(arguments.iter().map(|ty| (ty, depth + 1)));
                    pending.push((receiver, depth + 1));
                    nominal = Some(interface.as_ref());
                    overhead += mem::size_of::<NominalTy>();
                }
                Ty::Host(_) | Ty::SelfType(_) | Ty::Parameter { .. } | Ty::Builtin(_) => {}
            }
            if let Some(nominal) = nominal {
                overhead +=
                    (nominal.arguments.capacity() - nominal.arguments.len()) * mem::size_of::<Ty>();
                pending.extend(nominal.arguments.iter().map(|ty| (ty, depth + 1)));
                pending.extend(nominal.associated_types.values().map(|ty| (ty, depth + 1)));
                // Even one populated entry can own a full eleven-entry B-tree node.
                overhead = overhead.saturating_add(nominal.associated_types.len().saturating_mul(
                    10 * mem::size_of::<Ty>()
                        + 11 * mem::size_of::<DefinitionId>()
                        + 16 * mem::size_of::<usize>(),
                ));
            }
        }
        Some(Self {
            nodes,
            bytes: overhead.saturating_add(nodes.saturating_mul(mem::size_of::<Ty>())),
        })
    }
}

impl RemapDefinitions for BytecodeSemanticSlots {
    fn remap_definitions(
        &self,
        mapper: &mut DefinitionMapper<'_>,
    ) -> Result<Self, DefinitionMappingError> {
        mapper.check()?;
        Ok(BytecodeSemanticSlots {
            generic: self
                .generic
                .as_ref()
                .map(|value| (value).remap_definitions(mapper))
                .transpose()?,
            protocol_adapter: self
                .protocol_adapter
                .as_ref()
                .map(|value| (value).remap_definitions(mapper))
                .transpose()?,
            params: map_entries(
                self.params.len(),
                self.params
                    .iter()
                    .map(|(key, value)| Ok((*(key), (value).remap_definitions(mapper)?))),
            )?,
            result: self
                .result
                .as_ref()
                .map(|value| (value).remap_definitions(mapper))
                .transpose()?,
            slots: map_entries(
                self.slots.len(),
                self.slots
                    .iter()
                    .map(|(key, value)| Ok((*(key), (value).remap_definitions(mapper)?))),
            )?,
        })
    }
}

impl DefinitionReferences for BytecodeSemanticSlots {
    fn visit_definition_ids(
        &self,
        visit: &mut impl FnMut(&DefinitionId) -> Result<(), DefinitionMappingError>,
        cancel: &CancellationToken,
    ) -> Result<(), DefinitionMappingError> {
        check_cancel(cancel)?;
        if let Some(value0) = self.generic.as_ref() {
            (value0).visit_definition_ids(visit, cancel)?;
        }
        if let Some(value0) = self.protocol_adapter.as_ref() {
            (value0).visit_definition_ids(visit, cancel)?;
        }
        for value0 in self.params.values() {
            (value0).visit_definition_ids(visit, cancel)?;
        }
        if let Some(value0) = self.result.as_ref() {
            (value0).visit_definition_ids(visit, cancel)?;
        }
        for value0 in self.slots.values() {
            (value0).visit_definition_ids(visit, cancel)?;
        }
        Ok(())
    }
}
