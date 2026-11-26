//! Complete native declaration views derived from authoritative registrations.
use self::publication::ReferencePaths;
use crate::{analysis::core_source::packaged_modules, native::paths::LibraryPaths};
use kagari_common::{
    cancellation::CancellationToken,
    identity::{
        DefinitionKind, DefinitionPath,
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
        names::NameNamespace,
        native::NativeTypeConstructor,
        script::ScriptExpr,
    },
    language::{Protocol, role::LangRole},
    scalar::BuiltinType,
    surface::builtin_type,
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
    render_module(module, ReferencePaths::Preview(paths), &exports, cancel)
}

fn render_module(
    module: &DefinitionMetadata<ModuleDecl>,
    paths: ReferencePaths<'_>,
    exports: &[(String, String)],
    cancel: &CancellationToken,
) -> Result<DeclarationSource, DeclarationError> {
    check_cancel(cancel)?;
    module
        .records()
        .validate_script_bodies(module.definitions(), cancel)?;
    DeclarationView {
        module: module.records(),
        definitions: module.definitions(),
        paths,
        cancel,
    }
    .render(exports)
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
    paths: ReferencePaths<'a>,
    definitions: &'a DefinitionTable,
}

impl Deref for DeclarationView<'_> {
    type Target = ModuleDecl;
    fn deref(&self) -> &ModuleDecl {
        self.module
    }
}

impl<'a> DeclarationView<'a> {
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

    fn scope<'b>(&'b self, enclosing: &'b [GenericParam], own: &'b [GenericParam]) -> Spelling<'b> {
        Spelling {
            module: self,
            enclosing,
            own,
        }
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
            let scope = self.scope(&[], &ty.generic_params);
            let generics = output.generics(id, scope)?;
            let bounds = output.bounds(scope, &ty.bounds)?;
            if is_enum {
                output.text.push_str(" {\n");
                for variant in &ty.variants {
                    let owner = ModuleDecl::variant_id(id, &variant.name, self.definitions)?;
                    output.doc(owner)?;
                    let start = output.text.len();
                    output.text.push_str("    ");
                    let name_span = output.name(&variant.name);
                    let mut payload = vec![];
                    if !variant.shape.valid_for(variant.payload.len()) {
                        return Err(DeclarationError(format!(
                            "invalid field labels for {}::{}",
                            ty.name, variant.name
                        )));
                    }
                    let labels = variant.shape.labels();
                    if labels.is_some() || !variant.payload.is_empty() {
                        output
                            .text
                            .push_str(if labels.is_some() { " { " } else { "(" });
                        for (index, ty) in variant.payload.iter().enumerate() {
                            if index > 0 {
                                output.text.push_str(", ");
                            }
                            if let Some(labels) = labels {
                                output.text.push_str(&labels[index]);
                                output.text.push_str(": ");
                            }
                            payload.push(output.name(&scope.type_spelling(ty)?));
                        }
                        output
                            .text
                            .push_str(if labels.is_some() { " }" } else { ")" });
                    }
                    output.text.push_str(",\n");
                    output.site(owner, start, name_span, vec![], payload, vec![]);
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
                    let field_type = output.name(&scope.type_spelling(&field.ty)?);
                    output.text.push_str(",\n");
                    output.site(owner, start, name_span, vec![], vec![field_type], vec![]);
                }
                output.text.push_str("}\n\n");
            } else {
                output.text.push_str(";\n\n");
            }
            output.site(id, start, name_span, generics, vec![], bounds);
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
            let scope = self.scope(&[], &item.generic_params);
            let generics = output.generics(id, scope)?;
            if !item.supertraits.is_empty() {
                output.text.push_str(": ");
                let names = item
                    .supertraits
                    .iter()
                    .map(|ty| scope.nominal_spelling(ty))
                    .collect::<Result<Vec<_>, _>>()?;
                output.text.push_str(&names.join(" + "));
            }
            let bounds = output.bounds(scope, &item.bounds)?;
            output.text.push_str(" {\n");
            for member in &item.associated_types {
                let name = self.name(member.declaration)?;
                output.doc(member.declaration)?;
                let start = output.text.len();
                output.text.push_str("    type ");
                let name_span = output.name(name);
                let member_scope = self.scope(&item.generic_params, &member.generic_params);
                let generics = output.generics(member.declaration, member_scope)?;
                let constraints = output.constraints(member_scope, &member.bounds)?;
                let mut bounds = vec![NativeBoundSite {
                    target: name_span,
                    constraints,
                }];
                bounds.extend(output.bounds(member_scope, &member.parameter_bounds)?);
                output.text.push_str(";\n");
                output.site(
                    member.declaration,
                    start,
                    name_span,
                    generics,
                    vec![],
                    bounds,
                );
            }
            for method in &item.methods {
                output.function(
                    ModuleDecl::method_id(id, &method.name, self.definitions)?,
                    method,
                    Some(&item.generic_params),
                )?;
            }
            output.text.push_str("}\n\n");
            output.site(id, start, name_span, generics, vec![], bounds);
        }
        for (index, implementation) in self.implementations.iter().enumerate() {
            check_cancel(self.cancel)?;
            let id = self.implementation_id(index, self.definitions)?;
            let start = output.text.len();
            output.text.push_str("impl");
            let scope = self.scope(&[], &implementation.generic_params);
            let generics = output.generics(id, scope)?;
            output.text.push(' ');
            if let Some(trait_type) = &implementation.trait_type {
                let mut header = trait_type.clone();
                header.associated_types.clear();
                output.text.push_str(&scope.nominal_spelling(&header)?);
                output.text.push_str(" for ");
            }
            let name_span = output.name(&scope.type_spelling(&implementation.for_type)?);
            let bounds = output.bounds(scope, &implementation.bounds)?;
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
                    let value_span = output.name(&scope.type_spelling(ty)?);
                    output.text.push_str(";\n");
                    output.site(
                        member_id,
                        start,
                        name_span,
                        vec![],
                        vec![value_span],
                        vec![],
                    );
                }
            }
            for method in &implementation.methods {
                output.function(
                    ModuleDecl::method_id(id, &method.name, self.definitions)?,
                    method,
                    Some(&implementation.generic_params),
                )?;
            }
            output.text.push_str("}\n\n");
            output.site(id, start, name_span, generics, vec![], bounds);
        }
        for function in &self.functions {
            output.function(
                self.definition(DefinitionKind::Function, &function.name, self.definitions)?,
                function,
                None,
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
}

/// Generic names visible at this exact use site, including the parent of a method.
#[derive(Clone, Copy)]
struct Spelling<'a> {
    module: &'a DeclarationView<'a>,
    enclosing: &'a [GenericParam],
    own: &'a [GenericParam],
}

impl Spelling<'_> {
    fn parameter_name(
        &self,
        owner: DefinitionId,
        position: usize,
    ) -> Result<String, DeclarationError> {
        let nested = !self.enclosing.is_empty();
        for (binders, prefix) in [
            (self.own, if nested { "M" } else { "T" }),
            (self.enclosing, "T"),
        ] {
            if binders
                .get(position)
                .is_some_and(|p| p.owner == owner && p.position == position)
            {
                return Ok(if binders.len() == 1 {
                    prefix.into()
                } else {
                    format!("{prefix}{}", position + 1)
                });
            }
        }
        Err(DeclarationError(
            "type parameter is outside the rendered binder scope".into(),
        ))
    }

    fn reference(
        &self,
        target: &DefinitionPath,
        namespace: NameNamespace,
    ) -> Result<String, DeclarationError> {
        self.module.paths.spelling(
            &self.module.identity,
            target,
            namespace,
            |name| {
                if builtin_type(name).is_some_and(|kind| kind != BuiltinType::String) {
                    return Ok(false);
                }
                for binder in self.enclosing.iter().chain(self.own) {
                    if self.parameter_name(binder.owner, binder.position)? == name {
                        return Ok(false);
                    }
                }
                Ok(true)
            },
            self.module.cancel,
        )
    }

    fn type_spelling(&self, ty: &Ty) -> Result<String, DeclarationError> {
        if !ty.within_wire_limits() {
            return Err(DeclarationError("type exceeds native API limits".into()));
        }
        self.spell(ty)
    }

    fn spell(&self, ty: &Ty) -> Result<String, DeclarationError> {
        check_cancel(self.module.cancel)?;
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
                    return self.reference(
                        &NativeTypeConstructor::String.declaration(),
                        NameNamespace::Type,
                    );
                }
            }
            .into(),
            Ty::Host(declaration) => self.nominal_spelling(&NominalTy {
                declaration: *declaration,
                arguments: vec![],
                associated_types: BTreeMap::new(),
            })?,
            Ty::Parameter { owner, position } => self.parameter_name(*owner, *position)?,
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
                let name = self.module.name(*member)?;
                format!(
                    "<{} as {}>::{name}",
                    self.spell(receiver)?,
                    self.nominal_spelling(interface)?
                )
            }
            Ty::Range(item, kind) => {
                let constructor = NativeTypeConstructor::Range(*kind);
                let name = self.reference(&constructor.declaration(), NameNamespace::Type)?;
                if constructor.arity() == 0 {
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
        let declaration = self.module.view(ty.declaration)?;
        let mut text = self.reference(&declaration.to_path(), NameNamespace::Type)?;
        let mut args = ty
            .arguments
            .iter()
            .map(|ty| self.type_spelling(ty))
            .collect::<Result<Vec<_>, _>>()?;
        for (id, value) in self.module.associated_outputs(&ty.associated_types)? {
            args.push(format!(
                "{} = {}",
                self.module.name(id)?,
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

    fn generics(
        &mut self,
        owner: DefinitionId,
        scope: Spelling<'_>,
    ) -> Result<Vec<Span>, DeclarationError> {
        if scope.own.is_empty() {
            return Ok(vec![]);
        }
        self.text.push('<');
        let mut spans = vec![];
        for (index, parameter) in scope.own.iter().enumerate() {
            check_cancel(self.module.cancel)?;
            if parameter.owner != owner || parameter.position != index {
                return Err(DeclarationError(
                    "invalid generic binder in rendered declaration".into(),
                ));
            }
            if index != 0 {
                self.text.push_str(", ");
            }
            spans.push(self.name(&scope.parameter_name(owner, index)?));
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
        bounds: Vec<NativeBoundSite>,
    ) {
        self.sites.insert(
            id,
            NativeDeclarationSite {
                span: Span::new(start, self.text.trim_end_matches('\n').len()),
                name_span,
                generics,
                parameters,
                bounds,
            },
        );
    }

    fn function(
        &mut self,
        id: DefinitionId,
        function: &FnDecl,
        enclosing: Option<&[GenericParam]>,
    ) -> Result<(), DeclarationError> {
        self.doc(id)?;
        let start = self.text.len();
        self.text
            .push_str(if self.module.private_functions.contains(&id) {
                ""
            } else if enclosing.is_none() {
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
            .filter(|parameter| enclosing.is_none() || parameter.owner == id)
            .cloned()
            .collect::<Vec<_>>();
        let scope = self.module.scope(enclosing.unwrap_or_default(), &own);
        let generics = self.generics(id, scope)?;
        self.text.push('(');
        let mut parameters = vec![];
        for (index, parameter) in function.params.iter().enumerate() {
            if index != 0 {
                self.text.push_str(", ");
            }
            parameters.push(self.name(&parameter.name));
            if parameter.name != "self" {
                self.text.push_str(": ");
                self.text.push_str(&scope.type_spelling(&parameter.ty)?);
            }
        }
        self.text.push(')');
        if function.return_type != Ty::Builtin(BuiltinType::Unit) {
            self.text.push_str(" -> ");
            self.text
                .push_str(&scope.type_spelling(&function.return_type)?);
        }
        let bounds = self.bounds(scope, &function.bounds)?;
        if let Some(body) = self.module.script_bodies.get(&id) {
            self.text.push_str(
                " {
        ",
            );
            self.script_expr(scope, body)?;
            self.text.push_str(
                "
    }
",
            );
        } else {
            self.text.push_str(";\n");
        }
        self.site(id, start, name_span, generics, parameters, bounds);
        Ok(())
    }

    fn script_expr(
        &mut self,
        scope: Spelling<'_>,
        expression: &ScriptExpr,
    ) -> Result<(), DeclarationError> {
        check_cancel(self.module.cancel)?;
        match expression {
            ScriptExpr::Name(name) => self.text.push_str(name),
            ScriptExpr::Definition(id) => {
                let target = self.module.view(*id)?;
                self.text
                    .push_str(&scope.reference(&target.to_path(), NameNamespace::Value)?);
            }
            ScriptExpr::TraitMethod {
                receiver,
                interface,
                member,
            } => {
                self.text.push_str(&format!(
                    "<{} as {}>::{}",
                    scope.type_spelling(receiver)?,
                    scope.nominal_spelling(interface)?,
                    self.module.name(*member)?
                ));
            }
            ScriptExpr::Call {
                callee,
                type_arguments,
                arguments,
            } => {
                self.script_expr(scope, callee)?;
                if !type_arguments.is_empty() {
                    self.text.push_str("::<");
                    for (index, ty) in type_arguments.iter().enumerate() {
                        if index != 0 {
                            self.text.push_str(", ");
                        }
                        self.text.push_str(&scope.type_spelling(ty)?);
                    }
                    self.text.push('>');
                }
                self.text.push('(');
                for (index, argument) in arguments.iter().enumerate() {
                    if index != 0 {
                        self.text.push_str(", ");
                    }
                    self.script_expr(scope, argument)?;
                }
                self.text.push(')');
            }
        }
        Ok(())
    }

    fn bounds(
        &mut self,
        scope: Spelling<'_>,
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
            let target = self.name(&scope.type_spelling(&bound.ty)?);
            let constraints = self.constraints(scope, &bound.constraints)?;
            sites.push(NativeBoundSite {
                target,
                constraints,
            });
        }
        Ok(sites)
    }

    fn constraints(
        &mut self,
        scope: Spelling<'_>,
        constraints: &[NominalTy],
    ) -> Result<Vec<Span>, DeclarationError> {
        let mut spans = vec![];
        if !constraints.is_empty() {
            self.text.push_str(": ");
        }
        for (index, constraint) in constraints.iter().enumerate() {
            if index != 0 {
                self.text.push_str(" + ");
            }
            let name = scope.nominal_spelling(constraint)?;
            spans.push(self.name(&name));
        }
        Ok(spans)
    }
}

mod publication;

#[cfg(test)]
mod tests;
