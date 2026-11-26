//! Complete native declaration views derived from authoritative registrations.
use crate::{
    analysis::core_source::packaged_modules,
    native::paths::{ImporterPaths, LibraryPaths},
};
use kagari_common::{
    cancellation::CancellationToken,
    identity::{
        DefinitionKind, ModuleIdentity,
        metadata::DefinitionMetadata,
        table::{DefinitionId, DefinitionTable, DefinitionView},
    },
    span::Span,
};
use kagari_source::source::SourceFile;
use kagari_types::{
    callable::CallableImplementation,
    declaration::{
        FnDecl, TypeDefKind,
        module::{DeclarationError, ModuleDecl},
        native::NativeTypeConstructor,
        script::ScriptExpr,
    },
    language::{Protocol, role::LangRole},
    scalar::BuiltinType,
    ty::{GenericBound, GenericParam, NominalTy, Ty},
    visibility::Visibility,
};
use std::{collections::BTreeMap, ops::Deref, sync::Arc};

/// Byte sites recorded while rendering one canonical declaration into generated Kagari text.
#[derive(Debug, Clone)]
pub struct NativeDeclarationSite {
    /// Full declaration byte range in the generated text.
    pub span: Span,
    /// Declaration-name byte range.
    pub name_span: Span,
    /// Generic binder sites in declaration order.
    pub generics: Vec<Span>,
    /// Parameter sites in declaration order.
    pub parameters: Vec<Span>,
    /// Where/bound sites with their target and constraint ranges.
    pub bounds: Vec<NativeBoundSite>,
}

/// Generated source locations for a bound target and its requirements.
#[derive(Debug, Clone)]
pub struct NativeBoundSite {
    /// Bound-subject byte range.
    pub target: Span,
    /// Required trait/type constraint ranges in rendered order.
    pub constraints: Vec<Span>,
}

/// Generated declaration text and identity-to-site metadata from authoritative registration.
///
/// ```text
/// registered ModuleDecl + providers -> declaration_source
///   -> DeclarationSource { uri, text, sites }
///   -> bind text to its logical module for source-only tooling
/// ```
///
/// The generated view is presentation/analysis input, not an independently authored
/// source of native authority. All sites refer to this exact text, in bytes.
#[derive(Debug, Clone)]
pub struct DeclarationSource {
    /// Source URI assigned to the generated module view.
    pub uri: String,
    /// Complete rendered Kagari declarations and documentation.
    pub text: String,
    /// Scoped definition IDs to their generated source locations.
    pub sites: BTreeMap<DefinitionId, NativeDeclarationSite>,
    /// Retained identity prefix for every site in this generated view.
    pub definitions: DefinitionTable,
}

/// Renders a declaration preview using default producer spellings. For
/// standalone source publication use [`DeclarationSnapshot::render_native_document`](crate::analysis::declarations::DeclarationSnapshot::render_native_document),
/// which checks the actual importer and both namespaces.
///
/// # Errors
///
/// Returns a declaration error if registered data cannot be rendered consistently
/// with the supported source declaration model.
pub fn declaration_source(
    module: &DefinitionMetadata<ModuleDecl>,
    providers: &[Arc<DefinitionMetadata<ModuleDecl>>],
) -> Result<DeclarationSource, DeclarationError> {
    declaration_source_with_core(module, providers, packaged_modules())
}

/// Renders a preview with default producer spellings for the specified bundled core owners.
/// Unlike snapshot publication, a preview does not validate the effective import scopes.
/// Returns an error for unrepresentable registered declarations or Script bodies.
pub fn declaration_source_with_core(
    module: &DefinitionMetadata<ModuleDecl>,
    providers: &[Arc<DefinitionMetadata<ModuleDecl>>],
    core: &[Arc<SourceFile>],
) -> Result<DeclarationSource, DeclarationError> {
    let cancel = &CancellationToken::default();
    let paths = LibraryPaths::new(providers, core);
    let paths = paths.importer(&module.records().identity);
    let exports = module
        .records()
        .exports
        .iter()
        .map(|(name, target)| {
            check_cancel(cancel)?;
            let target = module
                .definitions()
                .resolve(*target)
                .map_err(|e| DeclarationError(e.to_string()))?;
            Ok((name.name.clone(), paths.definition_path(target, cancel)?))
        })
        .collect::<Result<Vec<_>, DeclarationError>>()?;
    render_module(module, paths, &exports, cancel)
}

fn render_module(
    module: &DefinitionMetadata<ModuleDecl>,
    paths: ImporterPaths<'_>,
    exports: &[(String, String)],
    cancel: &CancellationToken,
) -> Result<DeclarationSource, DeclarationError> {
    check_cancel(cancel)?;
    module
        .records()
        .validate_script_bodies(module.definitions(), cancel)?;
    DeclarationView::new(module, paths, cancel)?.render(exports)
}

fn check_cancel(cancel: &CancellationToken) -> Result<(), DeclarationError> {
    cancel
        .check()
        .map_err(|_| DeclarationError("native declaration rendering cancelled".into()))
}

/// Borrowed authoritative module plus provider context used to resolve rendered identities.
struct DeclarationView<'a> {
    cancel: &'a CancellationToken,
    module: &'a ModuleDecl,
    paths: ImporterPaths<'a>,
    definitions: &'a DefinitionTable,
    parameter_counts: BTreeMap<DefinitionId, usize>,
}

impl Deref for DeclarationView<'_> {
    type Target = ModuleDecl;
    fn deref(&self) -> &ModuleDecl {
        self.module
    }
}

impl<'a> DeclarationView<'a> {
    fn new(
        metadata: &'a DefinitionMetadata<ModuleDecl>,
        paths: ImporterPaths<'a>,
        cancel: &'a CancellationToken,
    ) -> Result<Self, DeclarationError> {
        let module = metadata.records();
        let mut parameter_counts = BTreeMap::<DefinitionId, usize>::new();
        let parameters = module
            .types
            .iter()
            .flat_map(|item| &item.generic_params)
            .chain(module.traits.iter().flat_map(|item| &item.generic_params))
            .chain(
                module
                    .implementations
                    .iter()
                    .flat_map(|item| &item.generic_params),
            )
            .chain(
                module
                    .functions
                    .iter()
                    .flat_map(|item| &item.generic_params),
            )
            .chain(
                module
                    .traits
                    .iter()
                    .flat_map(|item| &item.methods)
                    .flat_map(|method| &method.generic_params),
            )
            .chain(
                module
                    .implementations
                    .iter()
                    .flat_map(|item| &item.methods)
                    .flat_map(|method| &method.generic_params),
            );
        for parameter in parameters {
            check_cancel(cancel)?;
            let count = parameter_counts.entry(parameter.owner).or_default();
            *count = (*count).max(parameter.position.saturating_add(1));
        }
        Ok(Self {
            cancel,
            module,
            paths,
            definitions: metadata.definitions(),
            parameter_counts,
        })
    }

    fn view(&self, id: DefinitionId) -> Result<DefinitionView<'a>, DeclarationError> {
        check_cancel(self.cancel)?;
        self.definitions
            .resolve(id)
            .map_err(|error| DeclarationError(error.to_string()))
    }

    fn name(&self, id: DefinitionId) -> Result<&'a str, DeclarationError> {
        self.view(id)?
            .last()
            .map(|part| part.name)
            .ok_or_else(|| DeclarationError("missing declaration name".into()))
    }

    fn module_path(&self, identity: &ModuleIdentity) -> Result<String, DeclarationError> {
        self.paths.module_path(identity, self.cancel)
    }

    /// Display names use one-based numbering, omitted for a single parameter.
    /// Method binders use M only when an enclosing generic binder already uses T.
    fn parameter_spelling(
        &self,
        owner: DefinitionId,
        position: usize,
    ) -> Result<String, DeclarationError> {
        let method = self
            .view(owner)?
            .last()
            .is_some_and(|part| part.kind == DefinitionKind::Method);
        let parent = self
            .definitions
            .parent(owner)
            .map_err(|error| DeclarationError(error.to_string()))?;
        let prefix =
            if method && parent.is_some_and(|parent| self.parameter_counts.contains_key(&parent)) {
                "M"
            } else {
                "T"
            };
        Ok(if self.parameter_counts.get(&owner) == Some(&1) {
            prefix.into()
        } else {
            format!("{prefix}{}", position.saturating_add(1))
        })
    }

    /// Sort rendered associated outputs by exact identity, never allocation order.
    fn associated_outputs<'b>(
        &self,
        outputs: &'b BTreeMap<DefinitionId, Ty>,
    ) -> Result<Vec<(DefinitionId, &'b Ty)>, DeclarationError> {
        let mut entries = outputs
            .iter()
            .map(|(id, value)| Ok((*id, value, self.view(*id)?)))
            .collect::<Result<Vec<_>, DeclarationError>>()?;
        entries.sort_by(|(_, _, left), (_, _, right)| {
            left.module()
                .cmp(right.module())
                .then_with(|| left.segments().cmp(right.segments()))
        });
        Ok(entries
            .into_iter()
            .map(|(id, value, _)| (id, value))
            .collect())
    }

    fn render(&self, exports: &[(String, String)]) -> Result<DeclarationSource, DeclarationError> {
        let mut output = Renderer {
            module: self,
            text: "// Generated from native registration definitions. Do not edit.\n\n".into(),
            sites: BTreeMap::new(),
        };
        for line in self.module_documentation.lines() {
            check_cancel(self.cancel)?;
            output.text.push_str("//! ");
            output.text.push_str(line);
            output.text.push('\n');
        }
        output.text.push('\n');

        for (name, path) in exports {
            check_cancel(self.cancel)?;
            output
                .text
                .push_str(&format!("pub use {path} as {name};\n"));
        }
        for ty in &self.types {
            let id = self.definition(ty.kind.definition_kind(), &ty.name, self.definitions)?;
            let is_enum = ty.kind == TypeDefKind::Enum;
            let is_struct = ty.kind == TypeDefKind::Struct;
            output.doc(id)?;
            let start = output.text.len();
            output.text.push_str(match ty.kind {
                TypeDefKind::Enum => "pub enum ",
                TypeDefKind::Struct => "pub struct ",
                TypeDefKind::Host(_) => "pub native host type ",
                TypeDefKind::Native(_) | TypeDefKind::NativeStorage(_) => "pub native type ",
            });
            let name_span = output.name(&ty.name);
            let generics = output.generics(&ty.generic_params)?;
            let bounds = output.bounds(&ty.bounds)?;
            if is_enum {
                output.text.push_str(" {\n");
                for variant in &ty.variants {
                    let owner = ModuleDecl::variant_id(id, &variant.name, self.definitions)?;
                    output.doc(owner)?;
                    let start = output.text.len();
                    output.text.push_str("    ");
                    let name_span = output.name(&variant.name);
                    let mut payload = vec![];
                    if !variant.payload.is_empty() {
                        output.text.push('(');
                        for (index, ty) in variant.payload.iter().enumerate() {
                            if index > 0 {
                                output.text.push_str(", ");
                            }
                            payload.push(output.name(&self.type_spelling(ty)?));
                        }
                        output.text.push(')');
                    }
                    output.text.push_str(",\n");
                    output.site(owner, start, name_span, vec![], payload);
                }
                output.text.push_str("}\n\n");
            } else if is_struct || !ty.fields.is_empty() {
                output.text.push_str(" {\n");
                for field in &ty.fields {
                    let owner = self
                        .definitions
                        .lookup_child(id, DefinitionKind::Field, &field.name, 0)
                        .ok_or_else(|| {
                            DeclarationError("missing registered field identity".into())
                        })?;
                    output.doc(owner)?;
                    let start = output.text.len();
                    output.text.push_str("    ");
                    output.text.push_str(match field.visibility {
                        Visibility::Public => "pub ",
                        Visibility::PublicCrate => "pub(crate) ",
                        Visibility::PublicSuper => "pub(super) ",
                        Visibility::Private => "",
                    });
                    output
                        .text
                        .push_str(if field.mutable { "var " } else { "val " });
                    let name_span = output.name(&field.name);
                    output.text.push_str(": ");
                    let field_type = output.name(&self.type_spelling(&field.ty)?);
                    output.text.push_str(",\n");
                    output.site(owner, start, name_span, vec![], vec![field_type]);
                }
                output.text.push_str("}\n\n");
            } else {
                output.text.push_str(";\n\n");
            }
            output.site(id, start, name_span, generics, vec![]);
            output
                .sites
                .get_mut(&id)
                .expect("type declaration site")
                .bounds = bounds;
            if self.variant_exports.contains(&ty.name) {
                output.text.push_str(&format!(
                    "pub use self::{}::{{{}}};\n\n",
                    ty.name,
                    ty.variants
                        .iter()
                        .map(|variant| variant.name.as_str())
                        .collect::<Vec<_>>()
                        .join(", ")
                ));
            }
        }
        for item in &self.traits {
            let id = self.definition(DefinitionKind::Trait, &item.name, self.definitions)?;
            output.doc(id)?;
            let start = output.text.len();
            if let Some(role) =
                Protocol::from_definition(&id, self.definitions).and_then(LangRole::from_protocol)
            {
                output
                    .text
                    .push_str(&format!("#[lang = \"{}\"]\n", role.name()));
            }
            output.text.push_str("pub trait ");
            let name_span = output.name(&item.name);
            let generics = output.generics(&item.generic_params)?;
            if !item.supertraits.is_empty() {
                output.text.push_str(": ");
                let names = item
                    .supertraits
                    .iter()
                    .map(|ty| self.nominal_spelling(ty))
                    .collect::<Result<Vec<_>, _>>()?;
                output.text.push_str(&names.join(" + "));
            }
            output.text.push_str(" {\n");
            for member in &item.associated_types {
                let name = self.name(member.declaration)?;
                output.doc(member.declaration)?;
                let start = output.text.len();
                output.text.push_str("    type ");
                let name_span = output.name(name);
                let constraints = output.constraints(&member.bounds)?;
                output.text.push_str(";\n");
                output.site(member.declaration, start, name_span, vec![], vec![]);
                output
                    .sites
                    .get_mut(&member.declaration)
                    .expect("associated declaration")
                    .bounds = vec![NativeBoundSite {
                    target: name_span,
                    constraints,
                }];
            }
            for method in &item.methods {
                output.function(
                    ModuleDecl::method_id(id, &method.name, self.definitions)?,
                    method,
                    true,
                )?;
            }
            output.text.push_str("}\n\n");
            output.site(id, start, name_span, generics, vec![]);
        }
        for (index, implementation) in self.implementations.iter().enumerate() {
            check_cancel(self.cancel)?;
            let id = self.implementation_id(index, self.definitions)?;
            let start = output.text.len();
            output.text.push_str("impl");
            let generics = output.generics(&implementation.generic_params)?;
            output.text.push(' ');
            if let Some(trait_type) = &implementation.trait_type {
                let mut header = trait_type.clone();
                header.associated_types.clear();
                output.text.push_str(&self.nominal_spelling(&header)?);
                output.text.push_str(" for ");
            }
            let name_span = output.name(&self.type_spelling(&implementation.for_type)?);
            let bounds = output.bounds(&implementation.bounds)?;
            output.text.push_str(" {\n");
            if let Some(trait_type) = &implementation.trait_type {
                for (member, ty) in self.associated_outputs(&trait_type.associated_types)? {
                    let name = self.name(member)?;
                    let member_id = self
                        .definitions
                        .lookup_child(id, DefinitionKind::AssociatedType, name, 0)
                        .ok_or_else(|| {
                            DeclarationError("missing implementation associated identity".into())
                        })?;
                    let start = output.text.len();
                    output.text.push_str("    type ");
                    let name_span = output.name(name);
                    output.text.push_str(" = ");
                    let value_span = output.name(&self.type_spelling(ty)?);
                    output.text.push_str(";\n");
                    output.site(member_id, start, name_span, vec![], vec![value_span]);
                }
            }
            for method in &implementation.methods {
                output.function(
                    ModuleDecl::method_id(id, &method.name, self.definitions)?,
                    method,
                    true,
                )?;
            }
            output.text.push_str("}\n\n");
            output.site(id, start, name_span, generics, vec![]);
            output
                .sites
                .get_mut(&id)
                .expect("implementation declaration")
                .bounds = bounds;
        }
        for function in &self.functions {
            output.function(
                self.definition(DefinitionKind::Function, &function.name, self.definitions)?,
                function,
                false,
            )?;
        }
        while output.text.ends_with("\n\n") {
            output.text.pop();
        }
        let source = DeclarationSource {
            uri: format!(
                "kagari://native/{}/{}.kgr",
                self.identity.package.0,
                self.identity.path.join("/")
            ),
            text: output.text,
            sites: output.sites,
            definitions: self.definitions.clone(),
        };
        check_cancel(self.cancel)?;
        Ok(source)
    }

    fn type_spelling(&self, ty: &Ty) -> Result<String, DeclarationError> {
        if !ty.within_wire_limits() {
            return Err(DeclarationError("type exceeds native API limits".into()));
        }
        self.spell(ty)
    }

    fn spell(&self, ty: &Ty) -> Result<String, DeclarationError> {
        check_cancel(self.cancel)?;
        let join = |items: &[Ty]| -> Result<String, DeclarationError> {
            Ok(items
                .iter()
                .map(|item| self.spell(item))
                .collect::<Result<Vec<_>, _>>()?
                .join(", "))
        };
        Ok(match ty {
            Ty::Builtin(kind) => match kind {
                BuiltinType::Never => "!",
                BuiltinType::Unit => "()",
                BuiltinType::Bool => "bool",
                BuiltinType::Char => "char",
                BuiltinType::I8 => "i8",
                BuiltinType::I16 => "i16",
                BuiltinType::I32 => "i32",
                BuiltinType::I64 => "i64",
                BuiltinType::U8 => "u8",
                BuiltinType::U16 => "u16",
                BuiltinType::U32 => "u32",
                BuiltinType::U64 => "u64",
                BuiltinType::F32 => "f32",
                BuiltinType::F64 => "f64",
                BuiltinType::String => {
                    let owner = NativeTypeConstructor::String.declaration().module;
                    return Ok(format!("{}::String", self.module_path(&owner)?));
                }
            }
            .into(),
            Ty::Host(declaration) => self.nominal_spelling(&NominalTy {
                declaration: *declaration,
                arguments: vec![],
                associated_types: BTreeMap::new(),
            })?,
            Ty::Parameter { owner, position } => self.parameter_spelling(*owner, *position)?,
            Ty::SelfType(_) => "Self".into(),
            Ty::Array(item) => format!("[{}]", self.spell(item)?),

            Ty::Tuple(items) => format!(
                "({}{})",
                join(items)?,
                if items.len() == 1 { "," } else { "" }
            ),
            Ty::Function { params, result } => {
                format!("fn({}) -> {}", join(params)?, self.spell(result)?)
            }
            Ty::Struct(ty) | Ty::Trait(ty) | Ty::NativeObject(ty) | Ty::Enum(ty) => {
                self.nominal_spelling(ty)?
            }
            Ty::Projection {
                receiver,
                interface,
                member,
                arguments,
            } if arguments.is_empty() => {
                let name = self.name(*member)?;
                format!(
                    "<{} as {}>::{name}",
                    self.spell(receiver)?,
                    self.nominal_spelling(interface)?
                )
            }
            Ty::Range(item, kind) => {
                let owner = NativeTypeConstructor::Range(*kind).declaration().module;
                let name = format!("{}::{}", self.module_path(&owner)?, kind.name());
                if NativeTypeConstructor::Range(*kind).arity() == 0 {
                    name
                } else {
                    format!("{name}<{}>", self.spell(item)?)
                }
            }

            _ => {
                return Err(DeclarationError(
                    "unsupported type in initial native API importer".into(),
                ));
            }
        })
    }

    fn nominal_spelling(&self, ty: &NominalTy) -> Result<String, DeclarationError> {
        let declaration = self.view(ty.declaration)?;
        let name = self.name(ty.declaration)?;
        let mut text = if declaration.module() == &self.identity {
            name.to_owned()
        } else {
            format!("{}::{}", self.module_path(declaration.module())?, name)
        };
        let mut args = ty
            .arguments
            .iter()
            .map(|ty| self.type_spelling(ty))
            .collect::<Result<Vec<_>, _>>()?;
        for (id, value) in self.associated_outputs(&ty.associated_types)? {
            args.push(format!(
                "{} = {}",
                self.name(id)?,
                self.type_spelling(value)?
            ));
        }
        if !args.is_empty() {
            text.push_str(&format!("<{}>", args.join(", ")));
        }
        Ok(text)
    }
}

struct Renderer<'a> {
    module: &'a DeclarationView<'a>,
    text: String,
    sites: BTreeMap<DefinitionId, NativeDeclarationSite>,
}

impl Renderer<'_> {
    fn doc(&mut self, id: DefinitionId) -> Result<(), DeclarationError> {
        let nested = self.module.view(id)?.segments().count() > 1;
        if let Some(documentation) = self.module.documentation.get(&id) {
            for line in documentation.lines() {
                check_cancel(self.module.cancel)?;
                if nested {
                    self.text.push_str("    ");
                }
                self.text.push_str("/// ");
                self.text.push_str(line);
                self.text.push('\n');
            }
        }
        Ok(())
    }

    fn name(&mut self, name: &str) -> Span {
        let start = self.text.len();
        self.text.push_str(name);
        Span::new(start, self.text.len())
    }

    fn generics(&mut self, parameters: &[GenericParam]) -> Result<Vec<Span>, DeclarationError> {
        if parameters.is_empty() {
            return Ok(vec![]);
        }
        self.text.push('<');
        let mut spans = vec![];
        for (index, parameter) in parameters.iter().enumerate() {
            if index != 0 {
                self.text.push_str(", ");
            }
            spans.push(
                self.name(
                    &self
                        .module
                        .parameter_spelling(parameter.owner, parameter.position)?,
                ),
            );
        }
        self.text.push('>');
        Ok(spans)
    }

    fn site(
        &mut self,
        id: DefinitionId,
        start: usize,
        name_span: Span,
        generics: Vec<Span>,
        parameters: Vec<Span>,
    ) {
        self.sites.insert(
            id,
            NativeDeclarationSite {
                span: Span::new(start, self.text.trim_end_matches('\n').len()),
                name_span,
                generics,
                parameters,
                bounds: vec![],
            },
        );
    }

    fn function(
        &mut self,
        id: DefinitionId,
        function: &FnDecl,
        method: bool,
    ) -> Result<(), DeclarationError> {
        self.doc(id)?;
        let start = self.text.len();
        self.text
            .push_str(if self.module.private_functions.contains(&id) {
                ""
            } else if !method {
                "pub "
            } else if self
                .module
                .view(id)?
                .segments()
                .next()
                .is_some_and(|part| part.kind == DefinitionKind::Trait)
            {
                "    "
            } else {
                "    pub "
            });
        if matches!(function.implementation, CallableImplementation::Native(_)) {
            self.text.push_str("native ");
        }
        self.text.push_str("fn ");
        let name_span = self.name(&function.name);
        let own = function
            .generic_params
            .iter()
            .filter(|parameter| !method || parameter.owner == id)
            .cloned()
            .collect::<Vec<_>>();
        let generics = self.generics(&own)?;
        self.text.push('(');
        let mut parameters = vec![];
        for (index, parameter) in function.params.iter().enumerate() {
            if index != 0 {
                self.text.push_str(", ");
            }
            parameters.push(self.name(&parameter.name));
            if parameter.name != "self" {
                self.text.push_str(": ");
                self.text
                    .push_str(&self.module.type_spelling(&parameter.ty)?);
            }
        }
        self.text.push(')');
        if function.return_type != Ty::Builtin(BuiltinType::Unit) {
            self.text.push_str(" -> ");
            self.text
                .push_str(&self.module.type_spelling(&function.return_type)?);
        }
        let bounds = self.bounds(&function.bounds)?;
        if let Some(body) = self.module.script_bodies.get(&id) {
            self.text.push_str(
                " {
        ",
            );
            self.script_expr(body)?;
            self.text.push_str(
                "
    }
",
            );
        } else {
            self.text.push_str(";\n");
        }
        self.site(id, start, name_span, generics, parameters);
        self.sites
            .get_mut(&id)
            .expect("rendered declaration")
            .bounds = bounds;
        Ok(())
    }

    fn script_expr(&mut self, expression: &ScriptExpr) -> Result<(), DeclarationError> {
        check_cancel(self.module.cancel)?;
        match expression {
            ScriptExpr::Name(name) => self.text.push_str(name),
            ScriptExpr::Definition(id) => {
                let target = self.module.view(*id)?;
                let path = target
                    .segments()
                    .map(|part| part.name)
                    .collect::<Vec<_>>()
                    .join("::");
                if target.module() != &self.module.identity {
                    self.text
                        .push_str(&self.module.module_path(target.module())?);
                    self.text.push_str("::");
                } else {
                    self.text.push_str("self::");
                }
                self.text.push_str(&path);
            }
            ScriptExpr::TraitMethod {
                receiver,
                interface,
                member,
            } => {
                self.text.push_str(&format!(
                    "<{} as {}>::{}",
                    self.module.type_spelling(receiver)?,
                    self.module.nominal_spelling(interface)?,
                    self.module.name(*member)?
                ));
            }
            ScriptExpr::Call {
                callee,
                type_arguments,
                arguments,
            } => {
                self.script_expr(callee)?;
                if !type_arguments.is_empty() {
                    self.text.push_str("::<");
                    for (index, ty) in type_arguments.iter().enumerate() {
                        if index != 0 {
                            self.text.push_str(", ");
                        }
                        self.text.push_str(&self.module.type_spelling(ty)?);
                    }
                    self.text.push('>');
                }
                self.text.push('(');
                for (index, argument) in arguments.iter().enumerate() {
                    if index != 0 {
                        self.text.push_str(", ");
                    }
                    self.script_expr(argument)?;
                }
                self.text.push(')');
            }
        }
        Ok(())
    }

    fn bounds(
        &mut self,
        bounds: &[GenericBound],
    ) -> Result<Vec<NativeBoundSite>, DeclarationError> {
        let mut sites = vec![];
        if !bounds.is_empty() {
            self.text.push_str(" where ");
        }
        for (index, bound) in bounds.iter().enumerate() {
            if index != 0 {
                self.text.push_str(", ");
            }
            let target = self.name(&self.module.type_spelling(&bound.ty)?);
            let constraints = self.constraints(&bound.constraints)?;
            sites.push(NativeBoundSite {
                target,
                constraints,
            });
        }
        Ok(sites)
    }

    fn constraints(&mut self, constraints: &[NominalTy]) -> Result<Vec<Span>, DeclarationError> {
        let mut spans = vec![];
        if !constraints.is_empty() {
            self.text.push_str(": ");
        }
        for (index, constraint) in constraints.iter().enumerate() {
            if index != 0 {
                self.text.push_str(" + ");
            }
            let name = self.module.nominal_spelling(constraint)?;
            spans.push(self.name(&name));
        }
        Ok(spans)
    }
}

mod publication;

#[cfg(test)]
mod tests;
