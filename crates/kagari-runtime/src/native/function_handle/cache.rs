use crate::{
    Runtime,
    error::RuntimeError,
    frame::types::arguments::TypeArgument,
    module::Module,
    native::{
        binding::NativeResult,
        conversion::{KagariType, arguments::KagariArguments, context::ConversionContext},
        function_handle::{
            Function, FunctionBinding,
            evidence::{EntryEvidence, EvidenceKey},
        },
        methods::InherentMember,
    },
};
use kagari_common::identity::{DefinitionKind, DefinitionPath};
use kagari_contract::types::PublicItem;
use std::{
    any::TypeId,
    marker::PhantomData,
    sync::{Arc, Weak},
};

#[derive(Debug, Default)]
pub(crate) struct FunctionCache(Vec<CacheEntry>);

#[derive(Debug)]
struct CacheEntry {
    mapping: TypeId,
    evidence: EvidenceKey,
    record: Weak<FunctionBinding>,
}

impl Runtime {
    /// Bind a public function path relative to the supplied live Module.
    ///
    /// For example, `pub use internal::calculate as damage` exposes `damage` in
    /// that namespace without granting navigation to `internal`. Binding consumes
    /// one publication's verified declaration/slot mapping and checks Rust types.
    /// Calls select the current implementation of the originally bound declaration;
    /// later alias retargets affect only fresh bindings. Invalid/private paths and
    /// generic declarations without a closed entry fail before execution.
    pub fn bind_function<A: KagariArguments + 'static, R: KagariType + 'static>(
        &self,
        owner: &Module,
        path: &str,
    ) -> NativeResult<Function<A, R>> {
        self.gc().ensure_no_storage_borrow()?;
        let snapshot = self.capture_module(owner)?;
        let (target, function) = snapshot.function_target(path)?;
        self.cache_function(EntryEvidence::script(self, &target, function)?)
    }

    /// Bind an explicit public declaration with an already installed closed entry.
    /// This declaration capability is independent of relative public-path lookup;
    /// native entries can be bound without adding a source wrapper or export alias.
    pub fn bind_function_declaration<A: KagariArguments + 'static, R: KagariType + 'static>(
        &self,
        owner: &Module,
        declaration: &DefinitionPath,
    ) -> NativeResult<Function<A, R>> {
        self.bind_function_application_declaration(owner, declaration, &[])
    }

    /// Bind an explicit public declaration application using installed evidence.
    /// Missing applications, private declarations and incompatible Rust signatures
    /// fail before execution. This never performs runtime generic specialization.
    pub fn bind_function_application_declaration<
        A: KagariArguments + 'static,
        R: KagariType + 'static,
    >(
        &self,
        owner: &Module,
        declaration: &DefinitionPath,
        arguments: &[TypeArgument],
    ) -> NativeResult<Function<A, R>> {
        self.gc().ensure_no_storage_borrow()?;
        let snapshot = self.capture_module(owner)?;
        let owner = &snapshot.root;
        for argument in arguments {
            argument.validate(self)?;
        }
        let invalid = || {
            RuntimeError::module_validation(
                "public function has no checked closed executable entry",
            )
        };
        let [segment] = declaration.path.as_slice() else {
            return Err(invalid());
        };
        if segment.kind != DefinitionKind::Function || segment.occurrence != 0 {
            return Err(invalid());
        }
        let member = owner
            .members()
            .find(|member| member.bytecode.identity == declaration.module)
            .ok_or_else(invalid)?;
        let exported = member
            .bytecode
            .public_items
            .iter()
            .find_map(|item| match item {
                PublicItem::Function(function) if function.name == segment.name => Some(function),
                _ => None,
            })
            .ok_or_else(invalid)?;
        if exported.generic_params.len() != arguments.len() {
            return Err(invalid());
        }
        let declaration = owner
            .definitions()
            .lookup(declaration)
            .ok_or_else(invalid)?;
        let evidence = EntryEvidence::find(self, owner, declaration, arguments)?;
        self.cache_function(evidence)
    }

    pub(crate) fn bind_member_entry<A: KagariArguments + 'static, R: KagariType + 'static>(
        &self,
        member: &InherentMember,
        arguments: &[TypeArgument],
    ) -> NativeResult<Function<A, R>> {
        self.gc().ensure_no_storage_borrow()?;
        member.applied.validate(self)?;
        let arguments = member.arguments(self, arguments)?;
        let evidence =
            EntryEvidence::find(self, member.applied.owner(), member.declaration, &arguments)?;
        if member.has_receiver
            && !evidence.signature.params.first().is_some_and(|receiver| {
                receiver
                    .view(&evidence.owner)
                    .compatible(member.applied.type_argument().view(member.applied.owner()))
            })
        {
            return Err(RuntimeError::module_validation(
                "method receiver differs from its bound type",
            ));
        }
        self.cache_function(evidence)
    }

    pub(super) fn cache_function<A: KagariArguments + 'static, R: KagariType + 'static>(
        &self,
        evidence: EntryEvidence,
    ) -> NativeResult<Function<A, R>> {
        let member = &evidence.owner;
        let signature = &evidence.signature;
        let cx = ConversionContext::new(self, member)?;
        A::check_types(&cx, &signature.params)?;
        cx.check_type::<R>(&signature.result)?;
        let mut cache = self
            .function_bindings
            .try_borrow_mut()
            .map_err(|_| RuntimeError::module_validation("function binding cache is borrowed"))?;
        cache.0.retain(|entry| entry.record.strong_count() != 0);
        let mapping = TypeId::of::<(A, R)>();
        if let Some(record) = cache
            .0
            .iter()
            .find(|entry| entry.mapping == mapping && entry.evidence == evidence.key)
            .and_then(|entry| entry.record.upgrade())
        {
            return Ok(Function {
                binding: record,
                mapping: PhantomData,
            });
        }
        let record = Arc::new(FunctionBinding::Entry(evidence.bind()?));
        cache
            .0
            .try_reserve(1)
            .map_err(|_| RuntimeError::resource_limit("function binding cache"))?;
        cache.0.push(CacheEntry {
            mapping,
            evidence: evidence.key,
            record: Arc::downgrade(&record),
        });
        Ok(Function {
            binding: record,
            mapping: PhantomData,
        })
    }
}
