//! Actual executable graph edges, resolved only from protected scope inputs.
pub(crate) mod applications;
pub(crate) mod environments;
pub(crate) mod groups;
pub(crate) mod sources;
use crate::{
    error::RuntimeError,
    execution_metadata::{
        MetadataEdge,
        call_contracts::ScopedInterfaceCall,
        groups::{OperationGroupId, group::OperationGroup},
        managed::{
            applications::ApplicationObject, environments::EnvironmentObject,
            groups::OperationGroupObject, sources::MetadataError,
        },
    },
    frame::types::{TypeEnvironment, operations::OperationBindings},
    module::{ExecutableModule, managed::revision::RevisionState},
    native::{application::NativeApplication, future::storage::ColdObject},
    objects::interfaces::description::InterfaceDescription,
    resource::ResourceState,
    value::interfaces::InterfaceObject,
};
use kagari_gc::{
    access::Scope,
    error::GcError,
    object::{AnyObject, Edge, Gc, GcObject, ObjectCharge},
    scan::{EdgeVisitor, ScanBudget, ScanStatus},
};
use std::{
    collections::{HashMap, HashSet},
    ptr,
    sync::Arc,
};

/// Every input already belongs to this heap scope. Facts and semantic IDs choose
/// among these actual targets; they never reopen a global store or promote roots.
#[derive(Clone, Copy)]
pub(crate) enum ProtectedMetadata<'gc> {
    Revision(Gc<'gc, RevisionState>),
    Environment(Gc<'gc, EnvironmentObject>),
    Operations(Gc<'gc, OperationGroupObject>),
    Application(Gc<'gc, ApplicationObject>),
    Interface(Gc<'gc, InterfaceObject>),
    Cold(Gc<'gc, ColdObject>),
}

impl<'gc> ProtectedMetadata<'gc> {
    fn matches(self, cx: &Scope<'gc>, expected: MetadataEdge<'_>) -> bool {
        match (self, expected) {
            (Self::Revision(actual), MetadataEdge::Program(expected)) => cx
                .read(actual, |actual| {
                    actual.code().program_identity() == expected.program_identity()
                }),
            (Self::Environment(actual), MetadataEdge::Environment(expected)) => {
                cx.read(actual, |actual| actual.id() == expected)
            }
            (Self::Operations(actual), MetadataEdge::Group(expected)) => {
                cx.read(actual, |actual| actual.record().id() == expected)
            }
            (Self::Operations(actual), MetadataEdge::Operation(expected)) => {
                cx.read(actual, |actual| actual.record().get(expected).is_some())
            }
            (Self::Interface(actual), MetadataEdge::InterfaceDescription(expected)) => cx
                .read(actual, |actual| {
                    ptr::eq(actual.description().as_ref(), expected)
                }),
            (Self::Application(actual), MetadataEdge::Application(expected)) => {
                cx.read(actual, |actual| actual.id() == expected)
            }
            _ => false,
        }
    }

    pub(crate) fn erase(self) -> Gc<'gc, AnyObject> {
        match self {
            Self::Revision(value) => value.erase(),
            Self::Environment(value) => value.erase(),
            Self::Operations(value) => value.erase(),
            Self::Application(value) => value.erase(),
            Self::Interface(value) => value.erase(),
            Self::Cold(value) => value.erase(),
        }
    }

    fn kind(self) -> DependencyKind {
        match self {
            Self::Revision(_) => DependencyKind::Revision,
            Self::Environment(_) => DependencyKind::Environment,
            Self::Operations(_) => DependencyKind::Operations,
            Self::Application(_) => DependencyKind::Application,
            Self::Interface(_) => DependencyKind::Interface,
            Self::Cold(_) => DependencyKind::Cold,
        }
    }

    fn children(
        self,
        cx: &Scope<'gc>,
        resources: &ResourceState,
    ) -> Result<Vec<Self>, MetadataError> {
        match self {
            Self::Revision(owner) => RevisionState::metadata(cx, owner, resources),
            Self::Cold(owner) => ColdObject::metadata(cx, owner, resources),
            Self::Interface(owner) => InterfaceObject::metadata(cx, owner, resources),
            Self::Environment(owner) => {
                Dependencies::follow(cx, owner, |owner| &owner.dependencies, resources)
            }
            Self::Operations(owner) => {
                Dependencies::follow(cx, owner, |owner| &owner.dependencies, resources)
            }
            Self::Application(owner) => {
                Dependencies::follow(cx, owner, |owner| &owner.dependencies, resources)
            }
        }
    }
}

/// One closed preparation/read borrows this actual graph. The local node list
/// neither owns roots nor survives its heap scope; IDs only select within it.
pub(crate) struct ProtectedGraph<'gc> {
    nodes: Vec<ProtectedMetadata<'gc>>,
    root_count: usize,
}

impl<'gc> ProtectedGraph<'gc> {
    /// Select an immutable description only among actual interface providers.
    /// Sharing facts never creates ownership or admits an arbitrary receiver graph.
    pub(crate) fn interface(
        &self,
        cx: &Scope<'gc>,
        expected: &Arc<InterfaceDescription>,
        resources: &ResourceState,
    ) -> Result<Option<Gc<'gc, InterfaceObject>>, MetadataError> {
        let mut inspected = 0;
        for node in &self.nodes {
            poll(resources, &mut inspected)?;
            if let ProtectedMetadata::Interface(value) = *node
                && cx.read(value, |value| Arc::ptr_eq(value.description(), expected))
            {
                return Ok(Some(value));
            }
        }
        Ok(None)
    }

    /// Select only inside this actual protected graph, preserving the exact
    /// immutable type descriptor. Absence can invalidate an optional cache;
    /// mismatched facts remain an input error rather than a cache miss.
    pub(crate) fn environment(
        &self,
        cx: &Scope<'gc>,
        expected: &TypeEnvironment,
        resources: &ResourceState,
    ) -> Result<Option<Gc<'gc, EnvironmentObject>>, MetadataError> {
        let Some(ProtectedMetadata::Environment(value)) =
            self.find(cx, MetadataEdge::Environment(expected.id), resources)?
        else {
            return Ok(None);
        };
        if !cx.read(value, |value| {
            Arc::ptr_eq(&value.record().types, &expected.types)
        }) {
            return Err(RuntimeError::module_validation("mismatched environment facts").into());
        }
        Ok(Some(value))
    }

    /// Cache facts are reusable only while their exact dictionaries are in the
    /// caller's actual source graph. Missing optional cache backing is a miss;
    /// failed source admission or polling remains an error, never a miss.
    pub(crate) fn operation_dependencies(
        &self,
        cx: &Scope<'gc>,
        operations: &OperationBindings,
        resources: &ResourceState,
    ) -> Result<Option<Vec<ExecutableModule>>, MetadataError> {
        self.dependency_programs(cx, resources, |visit| operations.trace_metadata(visit))
    }

    pub(crate) fn interface_call_dependencies(
        &self,
        cx: &Scope<'gc>,
        call: &ScopedInterfaceCall,
        resources: &ResourceState,
    ) -> Result<Option<Vec<ExecutableModule>>, MetadataError> {
        self.dependency_programs(cx, resources, |visit| call.trace(visit))
    }

    pub(crate) fn native_dependencies(
        &self,
        cx: &Scope<'gc>,
        application: &NativeApplication,
        resources: &ResourceState,
    ) -> Result<Option<Vec<ExecutableModule>>, MetadataError> {
        self.dependency_programs(cx, resources, |visit| application.trace(visit))
    }

    fn dependency_programs<'facts>(
        &self,
        cx: &Scope<'gc>,
        resources: &ResourceState,
        enumerate: impl FnOnce(
            &mut dyn FnMut(MetadataEdge<'facts>) -> Result<(), Option<MetadataError>>,
        ) -> Result<(), Option<MetadataError>>,
    ) -> Result<Option<Vec<ExecutableModule>>, MetadataError> {
        let mut targets = Vec::new();
        let mut inspected = 0usize;
        let found = enumerate(&mut |edge| {
            poll(resources, &mut inspected)
                .map_err(|error| Some(MetadataError::Execution(error)))?;
            let target = match edge {
                MetadataEdge::EnvironmentFacts(facts) => self
                    .environment(cx, facts, resources)
                    .map_err(Some)?
                    .map(ProtectedMetadata::Environment),
                _ => self.find(cx, edge, resources).map_err(Some)?,
            }
            .ok_or(None)?;
            targets
                .try_reserve(1)
                .map_err(|error| Some(MetadataError::from(error)))?;
            targets.push(target);
            Ok(())
        });
        match found {
            Err(None) => Ok(None),
            Err(Some(error)) => Err(error),
            Ok(()) => Ok(Some(
                Self::new(cx, &targets, resources)?.programs(cx, resources)?,
            )),
        }
    }

    pub(crate) fn new(
        cx: &Scope<'gc>,
        roots: &[ProtectedMetadata<'gc>],
        resources: &ResourceState,
    ) -> Result<Self, MetadataError> {
        let mut pending = Vec::new();
        pending.try_reserve_exact(roots.len())?;
        pending.extend_from_slice(roots);
        let mut nodes = Vec::new();
        let mut inspected = 0usize;
        while let Some(target) = pending.pop() {
            poll(resources, &mut inspected)?;
            if contains(&nodes, target, resources, &mut inspected)? {
                continue;
            }
            let children = target.children(cx, resources)?;
            pending.try_reserve(children.len())?;
            pending.extend(children);
            nodes.try_reserve(1)?;
            nodes.push(target);
        }
        Ok(Self {
            nodes,
            root_count: roots.len(),
        })
    }

    pub(crate) fn single_root(&self) -> Option<ProtectedMetadata<'gc>> {
        (self.root_count == 1).then(|| self.nodes[0])
    }

    /// Snapshot dictionaries once for a preparation that visits many members.
    /// The map owns pure immutable facts only; its caller still supplies the
    /// original actual sources when publishing a derived environment/frame.
    pub(crate) fn operation_groups(
        &self,
        cx: &Scope<'gc>,
        resources: &ResourceState,
    ) -> Result<HashMap<OperationGroupId, Arc<OperationGroup>>, MetadataError> {
        let mut groups = HashMap::new();
        let mut inspected = 0usize;
        for node in &self.nodes {
            poll(resources, &mut inspected)?;
            if let ProtectedMetadata::Operations(node) = *node {
                let record = cx.read(node, |node| node.record().clone());
                if let Some(existing) = groups.get(&record.id()) {
                    if !Arc::ptr_eq(existing, &record) {
                        return Err(
                            GcError::Storage("ambiguous protected dictionary identity").into()
                        );
                    }
                } else {
                    groups.try_reserve(1)?;
                    groups.insert(record.id(), record);
                }
            }
        }
        Ok(groups)
    }

    pub(crate) fn find(
        &self,
        cx: &Scope<'gc>,
        expected: MetadataEdge<'_>,
        resources: &ResourceState,
    ) -> Result<Option<ProtectedMetadata<'gc>>, MetadataError> {
        let mut inspected = 0usize;
        for node in &self.nodes {
            poll(resources, &mut inspected)?;
            if node.matches(cx, expected) {
                return Ok(Some(*node));
            }
        }
        Ok(None)
    }

    pub(crate) fn programs(
        &self,
        cx: &Scope<'gc>,
        resources: &ResourceState,
    ) -> Result<Vec<ExecutableModule>, MetadataError> {
        let mut seen = HashSet::new();
        let mut programs = Vec::new();
        let mut inspected = 0usize;
        for node in &self.nodes {
            poll(resources, &mut inspected)?;
            if let ProtectedMetadata::Revision(revision) = *node {
                let code = cx.read(revision, |revision| revision.code().clone());
                seen.try_reserve(1)?;
                if seen.insert(code.program_identity()) {
                    programs.try_reserve(1)?;
                    programs.push(code);
                }
            }
        }
        Ok(programs)
    }
}

#[derive(Debug, Clone, Copy)]
enum DependencyKind {
    Revision,
    Environment,
    Operations,
    Application,
    Interface,
    Cold,
}

#[derive(Debug)]
struct Dependency {
    kind: DependencyKind,
    target: Edge<AnyObject>,
}

/// Immutable node adjacency. Pure descriptors may keep logical keys for lookup;
/// these owned edges independently keep all their supplying managed state alive.
#[derive(Debug)]
pub(crate) struct Dependencies {
    edges: Vec<Dependency>,
}

impl Dependencies {
    pub(crate) fn collect<'facts>(
        resources: &ResourceState,
        enumerate: impl FnOnce(
            &mut dyn FnMut(MetadataEdge<'facts>) -> Result<(), MetadataError>,
        ) -> Result<(), MetadataError>,
    ) -> Result<Vec<MetadataEdge<'facts>>, MetadataError> {
        let mut pending = Vec::new();
        let mut inspected = 0usize;
        enumerate(&mut |edge| {
            poll(resources, &mut inspected)?;
            pending.try_reserve(1)?;
            pending.push(edge);
            Ok(())
        })?;
        Ok(pending)
    }

    /// Resolve direct relationships before allocation. Borrowed descriptor views
    /// expand locally; managed nodes remain single edges rather than flattening
    /// their transitive graphs or introducing permanent Host roots.
    pub(crate) fn resolve<'gc>(
        cx: &Scope<'gc>,
        mut pending: Vec<MetadataEdge<'_>>,
        protected: &[ProtectedMetadata<'gc>],
        resources: &ResourceState,
    ) -> Result<Vec<ProtectedMetadata<'gc>>, MetadataError> {
        // A protected environment/dictionary owns its children. Resolve through
        // that actual graph, never through an unrelated semantic-ID registry.
        let graph = ProtectedGraph::new(cx, protected, resources)?;
        let mut inspected = 0usize;
        let mut resolved = Vec::<ProtectedMetadata<'gc>>::new();
        while let Some(edge) = pending.pop() {
            poll(resources, &mut inspected)?;
            match edge {
                MetadataEdge::Binding(binding) => {
                    pending.try_reserve(2)?;
                    pending.push(MetadataEdge::Program(&binding.owner));
                    pending.extend(
                        binding
                            .environment
                            .iter()
                            .map(MetadataEdge::EnvironmentFacts),
                    );
                    continue;
                }
                MetadataEdge::Program(_)
                | MetadataEdge::Environment(_)
                | MetadataEdge::EnvironmentFacts(_)
                | MetadataEdge::Group(_)
                | MetadataEdge::Operation(_)
                | MetadataEdge::Application(_)
                | MetadataEdge::InterfaceDescription(_) => {}
            }
            let actual = match edge {
                MetadataEdge::EnvironmentFacts(facts) => graph
                    .environment(cx, facts, resources)?
                    .map(ProtectedMetadata::Environment),
                _ => graph.find(cx, edge, resources)?,
            }
            .ok_or(GcError::Representation)?;
            if !contains(&resolved, actual, resources, &mut inspected)? {
                resolved.try_reserve(1)?;
                resolved.push(actual);
            }
        }
        Ok(resolved)
    }

    pub(crate) fn empty(capacity: usize) -> Result<Self, GcError> {
        let mut edges = Vec::new();
        edges.try_reserve_exact(capacity)?;
        Ok(Self { edges })
    }

    /// # Safety
    /// The object is newly allocated and not yet semantically published. Its
    /// scanner covers this container even on cancellation or a partial prefix.
    pub(crate) unsafe fn publish<'gc, T: GcObject>(
        cx: &mut Scope<'gc>,
        owner: Gc<'gc, T>,
        field: fn(&mut T) -> &mut Self,
        protected: &[ProtectedMetadata<'gc>],
        resources: &ResourceState,
    ) -> Result<(), MetadataError> {
        let original = cx.read(owner, GcObject::charge);
        let charge = ObjectCharge {
            bytes: original.bytes,
            units: original
                .units
                .checked_add(protected.len())
                .ok_or(GcError::Limit("metadata dependencies"))?,
        };
        // SAFETY: the caller owns this unpublished object. Every incoming target
        // is scope-protected, capacity is fixed and every push publishes its edge.
        // A failure leaves a scanned prefix and never exposes the object.
        unsafe {
            cx.with_object_mut(owner, |owner, mutation| {
                let dependencies = field(owner);
                if !dependencies.edges.is_empty() || dependencies.edges.capacity() < protected.len()
                {
                    return Err(GcError::Storage("metadata adjacency was already published"));
                }
                mutation.prepare_charge(charge)?;
                let mut inspected = 0usize;
                for target in protected {
                    if let Err(error) = poll(resources, &mut inspected) {
                        return Ok(Err(error));
                    }
                    let stored = mutation.edge_scoped(target.erase())?;
                    dependencies.edges.push(Dependency {
                        kind: target.kind(),
                        target: stored,
                    });
                }
                Ok(Ok(()))
            })?
        }
        .map_err(MetadataError::Execution)
    }

    pub(crate) fn charge(&self) -> ObjectCharge {
        ObjectCharge {
            bytes: self.edges.capacity() * size_of::<Dependency>(),
            units: self.edges.len(),
        }
    }

    pub(crate) fn follow<'gc, T: GcObject>(
        cx: &Scope<'gc>,
        owner: Gc<'gc, T>,
        field: fn(&T) -> &Self,
        resources: &ResourceState,
    ) -> Result<Vec<ProtectedMetadata<'gc>>, MetadataError> {
        let count = cx.read(owner, |owner| field(owner).edges.len());
        let mut targets = Vec::new();
        targets.try_reserve_exact(count)?;
        let mut inspected = 0usize;
        for index in 0..count {
            poll(resources, &mut inspected)?;
            let kind = cx.read(owner, |owner| field(owner).edges[index].kind);
            let target = cx
                .follow(owner, |owner| Some(&field(owner).edges[index].target))
                .expect("initialized metadata dependency");
            targets.push(match kind {
                DependencyKind::Revision => ProtectedMetadata::Revision(cx.downcast(target)?),
                DependencyKind::Environment => ProtectedMetadata::Environment(cx.downcast(target)?),
                DependencyKind::Operations => ProtectedMetadata::Operations(cx.downcast(target)?),
                DependencyKind::Application => ProtectedMetadata::Application(cx.downcast(target)?),
                DependencyKind::Interface => ProtectedMetadata::Interface(cx.downcast(target)?),
                DependencyKind::Cold => ProtectedMetadata::Cold(cx.downcast(target)?),
            });
        }
        Ok(targets)
    }

    /// # Safety
    /// Called only while the admitted provider retains this whole container.
    pub(crate) unsafe fn scan(
        &self,
        next: &mut usize,
        budget: &mut ScanBudget,
        visitor: &mut EdgeVisitor<'_>,
    ) -> Result<ScanStatus, GcError> {
        while let Some(edge) = self.edges.get(*next) {
            if !budget.take(1) {
                return Ok(ScanStatus::Pending);
            }
            // SAFETY: the admitted provider retains this initialized edge.
            if unsafe { visitor.try_visit_stored(&edge.target, budget)? } == ScanStatus::Pending {
                return Ok(ScanStatus::Pending);
            }
            *next += 1;
        }
        Ok(ScanStatus::Complete)
    }
}

fn poll(resources: &ResourceState, inspected: &mut usize) -> Result<(), RuntimeError> {
    if *inspected == 0 {
        resources.poll_closed()?;
    }
    *inspected += 1;
    if *inspected == 1024 {
        *inspected = 0;
    }
    Ok(())
}

fn contains<'gc>(
    targets: &[ProtectedMetadata<'gc>],
    expected: ProtectedMetadata<'gc>,
    resources: &ResourceState,
    inspected: &mut usize,
) -> Result<bool, MetadataError> {
    for target in targets {
        poll(resources, inspected)?;
        if target.erase().ptr_eq(expected.erase()) {
            return Ok(true);
        }
    }
    Ok(false)
}
