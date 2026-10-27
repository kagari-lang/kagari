//! Allocation-free publication of admitted identities and finite inline fields.
use crate::{
    Runtime,
    error::RuntimeError,
    execution_metadata::MetadataEdge,
    gc::{
        GcHeap,
        graph::objects::{EdgeVisitor, ScanEdge},
    },
    module::ModuleStore,
};

impl Runtime {
    /// Candidate/inline views are consumed synchronously, never retained as jobs.
    /// Callers still perform semantic/availability validation before commit.
    pub(crate) fn publish_metadata_edge(&self, edge: MetadataEdge<'_>) -> Result<(), RuntimeError> {
        if let MetadataEdge::Program(owner) = edge {
            return self.publish_executable_module(owner);
        }
        self.gc.publish_metadata_edge(self.modules(), edge)
    }
}

impl GcHeap {
    pub(crate) fn publish_metadata_edge(
        &self,
        modules: &ModuleStore,
        edge: MetadataEdge<'_>,
    ) -> Result<(), RuntimeError> {
        inline_targets(edge, &mut |edge| match edge {
            ScanEdge::Value(value) => self.publish_value(value),
            ScanEdge::Metadata(MetadataEdge::Program(owner)) => modules.publish_program(owner),
            edge => self.shade_graph_edge(modules, edge),
        })
    }

    pub(crate) fn check_metadata_access(
        &self,
        modules: &ModuleStore,
        edge: MetadataEdge<'_>,
    ) -> Result<(), RuntimeError> {
        inline_targets(edge, &mut |edge| {
            let accessible = match edge {
                ScanEdge::Value(value) => self.validate_value(&value),
                ScanEdge::Metadata(MetadataEdge::Program(owner)) => {
                    modules.contains_module(owner).unwrap_or(false)
                }
                ScanEdge::Metadata(edge) => {
                    self.heap_identity_accessible(self.metadata_scan_identity(edge)?)
                }
                ScanEdge::Identity(id) => self.with_graph_mark(modules, id, |mark| {
                    Ok(self.graph.try_lock()?.accessible(mark))
                })?,
            };
            if accessible {
                Ok(())
            } else {
                Err(RuntimeError::module_validation(
                    "invalid or condemned metadata target",
                ))
            }
        })
    }
}

/// Only finite inline fields recurse here. Admitted owners terminate at an ID;
/// neither access checking nor publication walks the transitive graph.
fn inline_targets(edge: MetadataEdge<'_>, visit: &mut EdgeVisitor<'_>) -> Result<(), RuntimeError> {
    match edge {
        MetadataEdge::EnvironmentView(record) => {
            for index in 0..record.scan_len() {
                if let Some(edge) = record.scan_field(index) {
                    inline_targets(edge, visit)?;
                }
            }
        }
        MetadataEdge::InterfaceView(snapshot) => {
            visit(ScanEdge::Value(snapshot.data))?;
            inline_targets(
                MetadataEdge::InterfaceDescription(&snapshot.description),
                visit,
            )?;
            for parent in &snapshot.parent_views {
                if let Some(id) = parent.get() {
                    inline_targets(MetadataEdge::Interface(*id), visit)?;
                }
            }
        }
        MetadataEdge::Binding(binding) => {
            for field in 0..2 {
                if let Some(edge) = binding.scan_field(field) {
                    inline_targets(edge, visit)?;
                }
            }
        }
        MetadataEdge::Closure(closure) => {
            if closure.call_route().is_none() {
                inline_targets(MetadataEdge::Program(&closure.implementation), visit)?;
            }
            if let Some(environment) = &closure.environment {
                inline_targets(MetadataEdge::Environment(environment.id), visit)?;
            }
            for value in &closure.captures {
                visit(ScanEdge::Value(*value))?;
            }
        }
        MetadataEdge::Selections(selections) => {
            for selection in selections {
                for field in 0..2 {
                    if let Some(edge) = selection.scan_field(field) {
                        inline_targets(edge, visit)?;
                    }
                }
            }
        }
        MetadataEdge::Operations(operations) => {
            for index in 0..operations.scan_len() {
                if let Some(edge) = operations.scan_field(index) {
                    inline_targets(edge, visit)?;
                }
            }
        }
        edge => visit(ScanEdge::Metadata(edge))?,
    }
    Ok(())
}
