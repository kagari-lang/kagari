//! Staging, context selection and atomic source-owned capture commits.
mod standalone;

use crate::{
    documents::DocumentInput,
    workspace::{
        CaptureFailure, CaptureIssue, CaptureRequest, ContextCapture, ContextKey, DocumentContext,
        MAX_CONTEXTS, Origin, WorkspaceCapture,
        budget::{InputBudget, InputLease},
        origins::{input_error, issue, native_key},
        overlay::{OverlayInput, ReadCache},
    },
};
use kagari_common::{cancellation::CancellationToken, identity::ModuleIdentity};
use kagari_package::{
    error::PackageError,
    manifest::{MANIFEST_NAME, MAX_MANIFEST_BYTES},
    modules::SourceInput,
    project::{ProjectDiscovery, ProjectOptions},
    tooling::ToolingInputs,
    workspace::MemberSelection,
};
use kagari_source::source_database::SourceDatabase;
use std::{
    cell::RefCell,
    collections::{BTreeMap, BTreeSet},
    path::Path,
    rc::Rc,
};

#[derive(Clone, PartialEq, Eq, PartialOrd, Ord)]
struct ContextIdentity {
    workspace: String,
    root: ModuleIdentity,
}

struct ContextState {
    key: ContextKey,
    database: SourceDatabase,
    lease: InputLease,
}

struct StagedContext {
    workspace: String,
    inputs: ToolingInputs,
    origins: BTreeMap<ModuleIdentity, Origin>,
    errors: Vec<CaptureIssue>,
    semantic_available: bool,
    root_order: usize,
}

impl StagedContext {
    fn identity(&self) -> ContextIdentity {
        ContextIdentity {
            workspace: self.workspace.clone(),
            root: self.inputs.root().clone(),
        }
    }
}

#[derive(Default)]
struct Staging {
    contexts: Vec<StagedContext>,
    identities: BTreeSet<ContextIdentity>,
    unavailable: BTreeSet<String>,
    errors: Vec<CaptureIssue>,
}

pub(super) struct CaptureState {
    normalization_root: String,
    contexts: BTreeMap<ContextIdentity, ContextState>,
    previous_generation: Option<u64>,
    next_epoch: u64,
    budget: InputBudget,
}

impl CaptureState {
    pub fn new(normalization_root: String, budget: InputBudget) -> Self {
        Self {
            normalization_root,
            contexts: BTreeMap::new(),
            previous_generation: None,
            next_epoch: 0,
            budget,
        }
    }

    pub fn capture(
        &mut self,
        request: CaptureRequest,
        cancel: &CancellationToken,
    ) -> Result<WorkspaceCapture, CaptureFailure> {
        if let Some(previous) = self.previous_generation
            && request.generation <= previous
        {
            return Err(CaptureFailure::OutOfOrder {
                previous,
                requested: request.generation,
            });
        }
        self.previous_generation = Some(request.generation);
        check(cancel)?;
        let cache = Rc::new(RefCell::new(ReadCache::new(self.budget.clone())));
        cache
            .borrow_mut()
            .register_documents(&request.documents, cancel)
            .map_err(package_failure)?;
        let mut inputs = Vec::new();
        let mut errors = Vec::new();
        let mut canonical_roots = BTreeSet::new();
        for (order, root) in request.roots.iter().enumerate() {
            match OverlayInput::new(root, &request.documents, cache.clone(), cancel) {
                Ok(input) => {
                    errors.extend(input.errors().iter().map(issue));
                    let key = input.canonical_root().map_err(package_failure)?;
                    // Construct every adapter before reading source: aliases opened
                    // through another configured root must still shadow disk here.
                    if canonical_roots.insert(key) {
                        inputs.push((order, input));
                    }
                }
                Err(PackageError::Cancelled) => return Err(CaptureFailure::Cancelled),
                Err(error) => errors.push(issue(&error)),
            }
        }
        let mut staging = Staging {
            errors,
            ..Staging::default()
        };
        for document in &request.documents {
            if inputs
                .iter()
                .any(|(_, input)| input.relative(&document.source_name).is_some())
                && !inputs
                    .iter()
                    .any(|(_, input)| input.admits_document(&document.source_name))
            {
                staging.unavailable.insert(document.source_name.clone());
            }
        }
        for (order, input) in &inputs {
            stage_projects(input, *order, &request.documents, &mut staging, cancel)?;
        }
        for document in &request.documents {
            check(cancel)?;
            if staging.contexts.iter().any(|context| {
                context
                    .origins
                    .values()
                    .any(|origin| document_matches(document, origin, &cache.borrow()))
            }) {
                continue;
            }
            let selected = inputs
                .iter()
                .find(|(_, input)| input.admits_document(&document.source_name));
            let canonical = cache
                .borrow()
                .document_key(&document.source_name)
                .expect("all admitted document identities are registered");
            let conflicting = cache.borrow().conflicting(&canonical);
            let context = standalone::stage(
                document,
                &canonical,
                conflicting,
                selected.map(|(order, input)| (*order, input)),
                staging.unavailable.contains(&document.source_name),
                cancel,
            )?;
            push_context(&mut staging, context)?;
        }
        check(cancel)?;
        let admitted = staging
            .contexts
            .iter()
            .map(StagedContext::identity)
            .collect::<BTreeSet<_>>();
        self.contexts
            .retain(|identity, _| admitted.contains(identity));
        let mut contexts = Vec::with_capacity(staging.contexts.len());
        let mut orders = BTreeMap::new();
        for candidate in staging.contexts {
            check(cancel)?;
            let identity = candidate.identity();
            if !self.contexts.contains_key(&identity) {
                self.next_epoch = self.next_epoch.checked_add(1).ok_or_else(|| {
                    CaptureFailure::InvalidInput("context epoch exhausted".into())
                })?;
                let key = ContextKey {
                    workspace: identity.workspace.clone(),
                    root: identity.root.clone(),
                    epoch: self.next_epoch,
                };
                let database = SourceDatabase::new(&self.normalization_root)
                    .map_err(CaptureFailure::InvalidInput)?;
                self.contexts.insert(
                    identity.clone(),
                    ContextState {
                        key,
                        database,
                        lease: InputLease::default(),
                    },
                );
            }
            let state = self
                .contexts
                .get_mut(&identity)
                .expect("admitted source context");
            let snapshot = candidate
                .inputs
                .source_snapshot(&mut state.database, cancel)
                .map_err(package_failure)?;
            let lease = self.budget.retain(snapshot.sources.files().cloned())?;
            state.lease = lease.clone();
            let origins = snapshot
                .origins
                .into_iter()
                .map(|(file, origin)| {
                    let presentation = candidate
                        .origins
                        .get(&origin.module)
                        .expect("staged origin matches committed module")
                        .clone();
                    (file, presentation)
                })
                .collect();
            orders.insert(state.key.clone(), candidate.root_order);
            contexts.push(ContextCapture {
                key: state.key.clone(),
                root: candidate.inputs.root().clone(),
                sources: snapshot.sources,
                origins,
                complete: candidate.inputs.complete(),
                discovery_complete: candidate.inputs.discovery_complete(),
                semantic_available: candidate.semantic_available,
                errors: candidate.errors,
                _lease: lease,
            });
        }
        check(cancel)?;
        let mut document_contexts = BTreeMap::new();
        for document in &request.documents {
            let mut candidates = Vec::new();
            for context in &contexts {
                for (file, origin) in &context.origins {
                    if document_matches(document, origin, &cache.borrow()) {
                        let own_package = origin.module.package == context.root.package;
                        candidates.push((!own_package, orders[&context.key], &context.key, *file));
                    }
                }
            }
            candidates.sort();
            if let Some((_, _, context, file)) = candidates.first() {
                document_contexts.insert(
                    document.source_name.clone(),
                    DocumentContext {
                        context: (*context).clone(),
                        file: *file,
                        version: document.version,
                        incarnation: document.incarnation,
                    },
                );
            }
        }
        let lease = self.budget.retain(
            contexts
                .iter()
                .flat_map(|context| context.sources.files().cloned())
                .chain(
                    request
                        .documents
                        .iter()
                        .filter_map(|document| document.source.clone()),
                ),
        )?;
        Ok(WorkspaceCapture {
            generation: request.generation,
            contexts,
            document_contexts,
            document_origins: cache
                .borrow()
                .document_keys()
                .iter()
                .map(|(name, key)| {
                    native_key(Path::new(key))
                        .map(|key| (name.clone(), key))
                        .map_err(CaptureFailure::InvalidInput)
                })
                .collect::<Result<_, _>>()?,
            errors: staging.errors,
            _lease: lease,
        })
    }
}

fn stage_projects(
    input: &OverlayInput,
    order: usize,
    documents: &[DocumentInput],
    staging: &mut Staging,
    cancel: &CancellationToken,
) -> Result<(), CaptureFailure> {
    let mut directories = Vec::new();
    let mut seen = BTreeSet::new();
    for document in documents {
        let Some(relative) = input.canonical_relative(&document.source_name) else {
            continue;
        };
        match nearest_manifest(input, parent(&relative), cancel) {
            Ok(Some(directory)) if seen.insert(directory.clone()) => directories.push(directory),
            Ok(_) => {}
            Err(PackageError::Cancelled) => return Err(CaptureFailure::Cancelled),
            Err(error) => {
                staging.errors.push(issue(&error));
                staging.unavailable.insert(document.source_name.clone());
            }
        }
    }
    match input.read(MANIFEST_NAME, MAX_MANIFEST_BYTES, cancel) {
        Ok(Some(_)) if seen.insert(String::new()) => directories.push(String::new()),
        Ok(_) => {}
        Err(PackageError::Cancelled) => return Err(CaptureFailure::Cancelled),
        Err(error) => staging.errors.push(issue(&error)),
    }
    let mut covered = BTreeSet::new();
    for directory in directories {
        check(cancel)?;
        if covered.contains(&directory) {
            continue;
        }
        let mut options = ProjectOptions {
            selection: MemberSelection::Workspace,
            ..ProjectOptions::default()
        };
        options.graph_limits.workspace.max_manifests = MAX_CONTEXTS;
        match ProjectDiscovery::load(input, &directory, &options, cancel) {
            Ok(project) => {
                covered.insert(project.workspace.directory.clone());
                covered.extend(project.workspace.members.keys().cloned());
                let workspace = native_key(
                    &input
                        .approved(&project.workspace.directory)
                        .map_err(package_failure)?,
                )
                .map_err(CaptureFailure::InvalidInput)?;
                for member in project.roots.keys() {
                    match project.tooling_inputs(member, cancel) {
                        Ok(inputs) => {
                            let errors = inputs.errors().iter().map(issue).collect();
                            let conflict = inputs.errors().iter().any(|error| matches!(error, PackageError::Input { origin, .. } if input.conflicting(origin)));
                            let semantic_available = !conflict
                                && inputs
                                    .sources()
                                    .iter()
                                    .any(|unit| &unit.module == inputs.root());
                            let origins = inputs
                                .sources()
                                .iter()
                                .map(|unit| {
                                    (
                                        unit.module.clone(),
                                        Origin {
                                            uri: if unit.host {
                                                None
                                            } else {
                                                input.uri(&unit.source.key)
                                            },
                                            module: unit.module.clone(),
                                            provider_key: unit.source.key.clone(),
                                            read_only: unit.host,
                                        },
                                    )
                                })
                                .collect();
                            push_context(
                                staging,
                                StagedContext {
                                    workspace: workspace.clone(),
                                    inputs,
                                    origins,
                                    errors,
                                    semantic_available,
                                    root_order: order,
                                },
                            )?;
                        }
                        Err(PackageError::Cancelled) => return Err(CaptureFailure::Cancelled),
                        Err(error) => {
                            staging.errors.push(issue(&error));
                            mark_unavailable(
                                input,
                                &directory,
                                documents,
                                &mut staging.unavailable,
                            );
                        }
                    }
                }
            }
            Err(PackageError::Cancelled) => return Err(CaptureFailure::Cancelled),
            Err(error) => {
                staging.errors.push(issue(&error));
                mark_unavailable(input, &directory, documents, &mut staging.unavailable);
            }
        }
    }
    Ok(())
}

fn push_context(staging: &mut Staging, context: StagedContext) -> Result<(), CaptureFailure> {
    if staging.identities.insert(context.identity()) {
        if staging.contexts.len() == MAX_CONTEXTS {
            return Err(CaptureFailure::InvalidInput(
                "workspace context limit exceeded".into(),
            ));
        }
        staging.contexts.push(context);
    }
    Ok(())
}

fn document_matches(document: &DocumentInput, origin: &Origin, cache: &ReadCache) -> bool {
    cache
        .document_key(&document.source_name)
        .is_some_and(|key| key == origin.provider_key)
}

fn mark_unavailable(
    input: &OverlayInput,
    directory: &str,
    documents: &[DocumentInput],
    unavailable: &mut BTreeSet<String>,
) {
    for document in documents {
        if input
            .canonical_relative(&document.source_name)
            .is_some_and(|relative| {
                directory.is_empty() || relative.starts_with(&format!("{directory}/"))
            })
        {
            unavailable.insert(document.source_name.clone());
        }
    }
}

fn nearest_manifest(
    input: &OverlayInput,
    mut directory: &str,
    cancel: &CancellationToken,
) -> Result<Option<String>, PackageError> {
    for _ in 0..=64 {
        let manifest = if directory.is_empty() {
            MANIFEST_NAME.to_owned()
        } else {
            format!("{directory}/{MANIFEST_NAME}")
        };
        if input.read(&manifest, MAX_MANIFEST_BYTES, cancel)?.is_some() {
            return Ok(Some(directory.to_owned()));
        }
        if directory.is_empty() {
            return Ok(None);
        }
        directory = parent(directory);
    }
    Err(input_error(
        directory,
        "manifest ancestor search exceeds depth limit",
    ))
}

fn parent(path: &str) -> &str {
    path.rsplit_once('/').map_or("", |(parent, _)| parent)
}

fn check(cancel: &CancellationToken) -> Result<(), CaptureFailure> {
    cancel.check().map_err(|_| CaptureFailure::Cancelled)
}

fn package_failure(error: PackageError) -> CaptureFailure {
    match error {
        PackageError::Cancelled => CaptureFailure::Cancelled,
        error => CaptureFailure::InvalidInput(error.to_string()),
    }
}
