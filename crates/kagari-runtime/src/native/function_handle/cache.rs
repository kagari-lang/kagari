use crate::{
    Runtime,
    error::RuntimeError,
    frame::types::arguments::TypeArgument,
    module::{LoadedModule, ModuleEpochRetention},
    native::{
        binding::NativeResult,
        conversion::{KagariType, arguments::KagariArguments, context::ConversionContext},
        function_handle::{
            PinnedFunction, PreparedFunction, Target,
            evidence::{EntryEvidence, EvidenceKey},
        },
        methods::InherentMember,
    },
};
use kagari_common::identity::{DefinitionKind, DefinitionPath, DefinitionPathSegment};
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
    record: Weak<PreparedFunction>,
}

impl Runtime {
    pub fn bind_function<A: KagariArguments + 'static, R: KagariType + 'static>(
        &self,
        owner: &LoadedModule,
        name: &str,
    ) -> NativeResult<PinnedFunction<A, R>> {
        self.bind_function_application(owner, name, &[])
    }

    /// Bind a concrete generic application already evidenced by the installed program.
    pub fn bind_function_application<A: KagariArguments + 'static, R: KagariType + 'static>(
        &self,
        owner: &LoadedModule,
        name: &str,
        arguments: &[TypeArgument],
    ) -> NativeResult<PinnedFunction<A, R>> {
        let declaration = DefinitionPath {
            module: owner.bytecode.identity.clone(),
            path: vec![DefinitionPathSegment {
                kind: DefinitionKind::Function,
                name: name.into(),
                occurrence: 0,
            }],
        };
        self.bind_function_application_declaration(owner, &declaration, arguments)
    }

    pub fn bind_function_declaration<A: KagariArguments + 'static, R: KagariType + 'static>(
        &self,
        owner: &LoadedModule,
        declaration: &DefinitionPath,
    ) -> NativeResult<PinnedFunction<A, R>> {
        self.bind_function_application_declaration(owner, declaration, &[])
    }

    pub fn bind_function_application_declaration<
        A: KagariArguments + 'static,
        R: KagariType + 'static,
    >(
        &self,
        owner: &LoadedModule,
        declaration: &DefinitionPath,
        arguments: &[TypeArgument],
    ) -> NativeResult<PinnedFunction<A, R>> {
        self.gc().ensure_no_native_borrow()?;
        self.validate_loaded_module(owner)?;
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
        let evidence = EntryEvidence::find(self, owner, declaration, arguments)?;
        self.cache_function(evidence)
    }

    pub(crate) fn bind_member_entry<A: KagariArguments + 'static, R: KagariType + 'static>(
        &self,
        member: &InherentMember,
        arguments: &[TypeArgument],
    ) -> NativeResult<PinnedFunction<A, R>> {
        self.gc().ensure_no_native_borrow()?;
        member.applied.validate(self)?;
        let arguments = member.arguments(self, arguments)?;
        let evidence = EntryEvidence::find(
            self,
            member.applied.owner(),
            &member.declaration,
            &arguments,
        )?;
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
    ) -> NativeResult<PinnedFunction<A, R>> {
        let invalid = || RuntimeError::module_validation("callable program retention");
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
            return Ok(PinnedFunction {
                prepared: record,
                mapping: PhantomData,
            });
        }
        let program = self
            .retain_program(member, ModuleEpochRetention::RuntimeValue)
            .ok_or_else(invalid)?;
        let target = evidence.prepare(self)?;
        let record = Arc::new(PreparedFunction {
            owner: member.clone(),
            signature: signature.clone(),
            target: Target::Entry(target),
            _program: program,
        });
        cache
            .0
            .try_reserve(1)
            .map_err(|_| RuntimeError::resource_limit("function binding cache"))?;
        cache.0.push(CacheEntry {
            mapping,
            evidence: evidence.key,
            record: Arc::downgrade(&record),
        });
        Ok(PinnedFunction {
            prepared: record,
            mapping: PhantomData,
        })
    }
}
