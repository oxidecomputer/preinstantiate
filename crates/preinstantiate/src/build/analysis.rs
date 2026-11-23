// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use super::imp::{
    Config, ConfigError, Error, ReadError, ShardIndex, TypePathError,
};
use crate::MAX_ARITY;
use camino::{Utf8Path, Utf8PathBuf};
use std::{collections::BTreeSet, fmt};
use syn::{ext::IdentExt, punctuated::Punctuated, visit::Visit};

#[derive(Debug, Default)]
pub(super) struct Analysis {
    /// Which ones worked.
    pub preinstantiated: Vec<PreinstantiatedMethod>,
    /// Which ones were skipped for things that shouldn't be warned about.
    pub skipped: Vec<Skipped>,
    /// Which ones had problems that should be reported.
    pub problems: Vec<SourceProblem>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct PreinstantiatedMethod {
    pub file: Utf8PathBuf,
    pub ty_path: String,
    pub name: MethodName,
    pub arity: usize,
    pub line: usize,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Skipped {
    pub file: Utf8PathBuf,
    pub item: SkippedItem,
}

impl fmt::Display for Skipped {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.file, self.item)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum SkippedItem {
    CfgFile,
    Module { path: String, reason: ModuleSkipReason },
    Method { ty_path: String, name: MethodName, reason: MethodSkipReason },
}

impl fmt::Display for SkippedItem {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SkippedItem::CfgFile => f.write_str(
                "file has an inner #![cfg], or a #![cfg_attr] that may \
                     expand to one",
            ),
            SkippedItem::Module { path, reason } => {
                write!(f, "mod {path}: {reason}")
            }
            SkippedItem::Method { ty_path, name, reason } => {
                write!(f, "{ty_path}::{name}: {reason}")
            }
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum ModuleSkipReason {
    Cfg,
    PathAttribute,
}

impl fmt::Display for ModuleSkipReason {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ModuleSkipReason::Cfg => f.write_str("has a cfg or cfg_attr"),
            ModuleSkipReason::PathAttribute => f.write_str("has #[path]"),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum MethodSkipReason {
    CfgOnImpl,
    CfgOnMethod,
    CfgOnParameter,
    TargetFeature,
    ExternAbi,
    ImplHasGenerics,
    SelfTypeHasGenericArguments,
    TypeOrConstGenerics,
    ImplTraitArgument,
    Unsafe,
    TooManyArguments(usize),
}

impl fmt::Display for MethodSkipReason {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            MethodSkipReason::CfgOnImpl => {
                f.write_str("impl block has a cfg or cfg_attr")
            }
            MethodSkipReason::CfgOnMethod => {
                f.write_str("has a cfg or cfg_attr")
            }
            MethodSkipReason::CfgOnParameter => {
                f.write_str("a parameter has a cfg or cfg_attr")
            }
            MethodSkipReason::TargetFeature => {
                f.write_str("has #[target_feature]")
            }
            MethodSkipReason::ExternAbi => f.write_str("has a non-Rust ABI"),
            MethodSkipReason::ImplHasGenerics => {
                f.write_str("impl block has generics")
            }
            MethodSkipReason::SelfTypeHasGenericArguments => {
                f.write_str("impl block's self type has generic arguments")
            }
            MethodSkipReason::TypeOrConstGenerics => {
                f.write_str("has type or const generics")
            }
            MethodSkipReason::ImplTraitArgument => {
                f.write_str("has an impl Trait argument")
            }
            MethodSkipReason::Unsafe => f.write_str("is unsafe"),
            MethodSkipReason::TooManyArguments(n) => {
                write!(
                    f,
                    "has {n} arguments including self, more than the \
                     maximum of {MAX_ARITY}"
                )
            }
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct SourceProblem {
    pub file: Utf8PathBuf,
    pub kind: SourceProblemKind,
}

impl fmt::Display for SourceProblem {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.file, self.kind)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum SourceProblemKind {
    ParseError { line: usize, column: usize, message: String },
    ModuleFileNotFound { module: String },
}

impl fmt::Display for SourceProblemKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SourceProblemKind::ParseError { line, column, message } => write!(
                f,
                "failed to parse at line {line}, column {column}: {message}"
            ),
            SourceProblemKind::ModuleFileNotFound { module } => {
                write!(f, "mod {module}: module file not found")
            }
        }
    }
}

#[derive(Debug)]
pub(super) struct ValidatedConfig<'a> {
    crate_root: &'a Utf8Path,
    source_dir: &'a Utf8Path,
    root_file: &'a str,
    shard: ShardIndex,
    types: Vec<TargetType>,
}

impl ValidatedConfig<'_> {
    /// Returns the directory the crate root is in, relative to the build
    /// script's manifest directory.
    ///
    /// For example, with a crate root of `../nexus-db-queries/src/lib.rs`, this
    /// is `../nexus-db-queries/src`, and a module file like `db/silo.rs` is
    /// read from `../nexus-db-queries/src/db/silo.rs`.
    pub(super) fn source_dir(&self) -> &Utf8Path {
        self.source_dir
    }

    pub(super) fn shard(&self) -> ShardIndex {
        self.shard
    }
}

/// Validates the config.
///
/// Validation doesn't depend on share-generics so that initial problems are
/// reported on all builds, not just those with share-generics enabled.
pub(super) fn validate(config: &Config) -> Result<ValidatedConfig<'_>, Error> {
    let mut errors = Vec::new();

    let crate_root = config.crate_root.as_path();
    let root_file = crate_root.file_name();
    if root_file.is_none() {
        errors.push(ConfigError::InvalidCrateRoot {
            path: crate_root.to_owned(),
        });
    }

    let mut types: Vec<TargetType> = Vec::new();
    if config.async_inherent_method_types.is_empty() {
        errors.push(ConfigError::NoTypes);
    }
    for ty_path in &config.async_inherent_method_types {
        let ty = match TargetType::parse(ty_path) {
            Ok(ty) => ty,
            Err(error) => {
                errors.push(ConfigError::InvalidTypePath {
                    path: ty_path.clone(),
                    error,
                });
                continue;
            }
        };
        // The find() is quadratic time in the limit, but we don't expect too
        // many types — maybe a handful.
        match types.iter().find(|t| t.name == ty.name) {
            Some(existing) if existing.same_path(&ty) => {
                errors.push(ConfigError::DuplicateType { path: ty.path });
            }
            Some(existing) => errors.push(ConfigError::DuplicateTypeName {
                name: ty.name.into_string(),
                first: existing.path.clone(),
                second: ty.path,
            }),
            None => types.push(ty),
        }
    }

    match (root_file, errors.is_empty()) {
        (Some(root_file), true) => Ok(ValidatedConfig {
            crate_root,
            source_dir: crate_root.parent().unwrap_or(Utf8Path::new("")),
            root_file,
            shard: config.shard,
            types,
        }),
        (Some(_), false) | (None, _) => Err(Error::InvalidConfig(errors)),
    }
}

/// Walks the module tree from the crate root and returns the list of methods to
/// be pre-instantiated.
///
/// `read` is a function that reads a file by path, relative to the crate root
/// and returns its contents as a string.
pub(super) fn analyze<R>(
    config: &ValidatedConfig<'_>,
    read: R,
) -> Result<Analysis, Error>
where
    R: FnMut(&Utf8Path) -> std::io::Result<String>,
{
    let mut reader = ModuleReader {
        source_dir: config.source_dir,
        read,
        errors: Vec::new(),
    };
    let mut analysis = Analysis::default();
    let mut matched_types = BTreeSet::new();
    let mut unanalyzed_modules: Vec<Skipped> = Vec::new();
    let mut stack =
        Vec::from([PendingModule::Root(Utf8PathBuf::from(config.root_file))]);

    while let Some(pending) = stack.pop() {
        let Some(source) = pending.resolve(&mut reader, &mut analysis) else {
            continue;
        };
        let ownership = if config.shard.contains(&source.path) {
            FileOwnership::ThisShard
        } else {
            FileOwnership::OtherShard
        };
        let file = match syn::parse_file(&source.contents) {
            Ok(file) => file,
            Err(error) => {
                analysis.problems.push(SourceProblem {
                    file: source.path,
                    // proc-macro2's columns start at 0, but editors and rustc
                    // count columns from 1.
                    kind: SourceProblemKind::ParseError {
                        line: error.span().start().line,
                        column: error.span().start().column + 1,
                        message: error.to_string(),
                    },
                });
                continue;
            }
        };
        let mut walker = ItemWalker {
            types: &config.types,
            file_path: &source.path,
            ownership,
            analysis: &mut analysis,
            matched_types: &mut matched_types,
            unanalyzed_modules: &mut unanalyzed_modules,
            child_modules: Vec::new(),
        };
        walker.walk_file(&file, &source.child_dir);
        stack.extend(walker.child_modules);
    }

    if !reader.errors.is_empty() {
        let mut errors = reader.errors;
        errors.sort_by(|a, b| a.path.cmp(&b.path));
        return Err(Error::ReadSources(errors));
    }

    let unmatched: Vec<String> = config
        .types
        .iter()
        .enumerate()
        .filter(|(index, _)| !matched_types.contains(index))
        .map(|(_, ty)| ty.path.clone())
        .collect();
    let unanalyzed = || {
        let source_dir = config.source_dir;
        let mut unanalyzed: Vec<String> = unanalyzed_modules
            .iter()
            .map(|skipped| {
                format!("{}: {}", source_dir.join(&skipped.file), skipped.item)
            })
            .chain(analysis.problems.iter().map(|problem| {
                format!("{}: {}", source_dir.join(&problem.file), problem.kind)
            }))
            .collect();
        unanalyzed.sort();
        unanalyzed
    };
    if !unmatched.is_empty() {
        return Err(Error::TypesNotFound {
            crate_root: config.crate_root.to_owned(),
            types: unmatched,
            unanalyzed: unanalyzed(),
        });
    }

    // Sorting by file and then by line ensures that the order of generated
    // calls matches the source.
    analysis
        .preinstantiated
        .sort_by(|a, b| (&a.file, a.line).cmp(&(&b.file, b.line)));
    // Sort skips and problems by file to give the report a predictable order.
    analysis.skipped.sort_by(|a, b| a.file.cmp(&b.file));
    analysis.problems.sort_by(|a, b| a.file.cmp(&b.file));
    Ok(analysis)
}

#[derive(Debug)]
struct TargetType {
    name: TypeName,
    path: String,
}

impl TargetType {
    fn parse(ty_path: &str) -> Result<Self, TypePathError> {
        let parsed = parse_type_path(ty_path)?;
        let last = parsed
            .segments
            .last()
            .expect("a parsed path has at least one segment");
        let leading_colon =
            if parsed.leading_colon.is_some() { "::" } else { "" };
        let segments: Vec<String> = parsed
            .segments
            .iter()
            .map(|segment| segment.ident.to_string())
            .collect();
        let path = format!("{leading_colon}{}", segments.join("::"));
        Ok(Self { name: TypeName::from_ident(&last.ident), path })
    }

    /// Returns true if `self` and `other` are lexically the same path.
    ///
    /// In particular, handles `a::B` vs `::a::B`.
    fn same_path(&self, other: &TargetType) -> bool {
        self.path.trim_start_matches("::")
            == other.path.trim_start_matches("::")
    }
}

fn parse_type_path(ty_path: &str) -> Result<syn::Path, TypePathError> {
    let parsed =
        syn::parse_str::<syn::Path>(ty_path).map_err(TypePathError::Parse)?;
    if parsed.segments.iter().any(|segment| !segment.arguments.is_none()) {
        return Err(TypePathError::GenericArguments);
    }
    // Since we only do a source-level analysis, we depend on the type name
    // (like DataStore) being unique within the crate. So accept any valid path
    // that ends in the type name.
    let mut keyword_prefix_allowed = parsed.leading_colon.is_none();
    for (index, segment) in parsed.segments.iter().enumerate() {
        let ident = segment.ident.to_string();
        let allowed = match ident.as_str() {
            "crate" | "self" => keyword_prefix_allowed && index == 0,
            "super" => keyword_prefix_allowed,
            "Self" => false,
            _ => true,
        };
        if !allowed {
            return Err(TypePathError::KeywordSegment { segment: ident });
        }
        keyword_prefix_allowed =
            keyword_prefix_allowed && (ident == "self" || ident == "super");
    }
    Ok(parsed)
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct TypeName(String);

impl TypeName {
    fn from_ident(ident: &syn::Ident) -> Self {
        Self(ident.unraw().to_string())
    }

    fn into_string(self) -> String {
        self.0
    }
}

/// A method name without a potential leading `r#`.
///
/// The generated code contains `r#{name}` for every method.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct MethodName(String);

impl MethodName {
    fn from_ident(ident: &syn::Ident) -> Self {
        Self(ident.unraw().to_string())
    }
}

impl fmt::Display for MethodName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

struct ModuleReader<'a, R> {
    source_dir: &'a Utf8Path,
    read: R,
    errors: Vec<ReadError>,
}

impl<R> ModuleReader<'_, R>
where
    R: FnMut(&Utf8Path) -> std::io::Result<String>,
{
    fn read_required(&mut self, rel_path: &Utf8Path) -> Option<String> {
        match (self.read)(rel_path) {
            Ok(contents) => Some(contents),
            Err(error) => {
                self.record(rel_path, error);
                None
            }
        }
    }

    fn read_candidate(&mut self, rel_path: &Utf8Path) -> CandidateRead {
        match (self.read)(rel_path) {
            Ok(contents) => CandidateRead::Found(contents),
            // NotADirectory comes up when `foo` is a file and `foo/mod.rs` is
            // tried, so treat it as similar to NotFound.
            Err(error) if is_missing_candidate(error.kind()) => {
                CandidateRead::Missing
            }
            Err(error) => {
                self.record(rel_path, error);
                CandidateRead::Failed
            }
        }
    }

    fn record(&mut self, rel_path: &Utf8Path, error: std::io::Error) {
        self.errors
            .push(ReadError { path: self.source_dir.join(rel_path), error });
    }
}

fn is_missing_candidate(kind: std::io::ErrorKind) -> bool {
    kind == std::io::ErrorKind::NotFound
        || kind == std::io::ErrorKind::NotADirectory
}

enum CandidateRead {
    Found(String),
    Missing,
    Failed,
}

enum PendingModule {
    Root(Utf8PathBuf),
    Child { parent: Utf8PathBuf, dir: Utf8PathBuf, name: String },
}

impl PendingModule {
    fn resolve<R>(
        self,
        reader: &mut ModuleReader<'_, R>,
        analysis: &mut Analysis,
    ) -> Option<ModuleSource>
    where
        R: FnMut(&Utf8Path) -> std::io::Result<String>,
    {
        match self {
            PendingModule::Root(path) => {
                let contents = reader.read_required(&path)?;
                Some(ModuleSource {
                    path,
                    contents,
                    child_dir: Utf8PathBuf::new(),
                })
            }
            PendingModule::Child { parent, dir, name } => {
                let child_dir = dir.join(&name);
                for path in
                    [dir.join(format!("{name}.rs")), child_dir.join("mod.rs")]
                {
                    match reader.read_candidate(&path) {
                        CandidateRead::Found(contents) => {
                            return Some(ModuleSource {
                                path,
                                contents,
                                child_dir,
                            });
                        }
                        CandidateRead::Missing => {}
                        CandidateRead::Failed => return None,
                    }
                }
                analysis.problems.push(SourceProblem {
                    file: parent,
                    kind: SourceProblemKind::ModuleFileNotFound {
                        module: module_path(&dir, &name),
                    },
                });
                None
            }
        }
    }
}

/// Returns the module path.
///
/// The module's directory relative to the crate root mirrors its module path
/// (currently, we don't support `#[path]` directives), so joining the
/// directory's components allows us to provide the correct name for the
/// module..
fn module_path(dir: &Utf8Path, name: &str) -> String {
    dir.join(name).iter().collect::<Vec<_>>().join("::")
}

struct ModuleSource {
    path: Utf8PathBuf,
    contents: String,
    child_dir: Utf8PathBuf,
}

/// Walks a Rust file's syntax tree.
struct ItemWalker<'a> {
    types: &'a [TargetType],
    file_path: &'a Utf8Path,
    ownership: FileOwnership,
    analysis: &'a mut Analysis,
    matched_types: &'a mut BTreeSet<usize>,
    unanalyzed_modules: &'a mut Vec<Skipped>,
    child_modules: Vec<PendingModule>,
}

#[derive(Clone, Copy, Debug)]
enum FileOwnership {
    ThisShard,
    OtherShard,
}

impl<'a> ItemWalker<'a> {
    fn walk_file(&mut self, file: &syn::File, module_dir: &Utf8Path) {
        if has_cfg(&file.attrs) {
            self.record_skip(SkippedItem::CfgFile);
            return;
        }
        self.walk_items(&file.items, module_dir);
    }

    fn walk_items(&mut self, items: &[syn::Item], module_dir: &Utf8Path) {
        for item in items {
            match item {
                syn::Item::Mod(item_mod) => self.walk_mod(item_mod, module_dir),
                syn::Item::Impl(item_impl) => self.walk_impl(item_impl),
                _ => {}
            }
        }
    }

    fn walk_mod(&mut self, item_mod: &syn::ItemMod, module_dir: &Utf8Path) {
        // If rustc encounters `mod r#foo;`, it looks for foo.rs, so unraw is
        // appropriate.
        let name = item_mod.ident.unraw().to_string();
        let reason = if has_cfg(&item_mod.attrs) {
            Some(ModuleSkipReason::Cfg)
        } else if item_mod.attrs.iter().any(|a| a.path().is_ident("path")) {
            Some(ModuleSkipReason::PathAttribute)
        } else {
            None
        };
        if let Some(reason) = reason {
            self.record_skip(SkippedItem::Module {
                path: module_path(module_dir, &name),
                reason,
            });
            return;
        }
        match &item_mod.content {
            Some((_, items)) => self.walk_items(items, &module_dir.join(&name)),
            None => self.child_modules.push(PendingModule::Child {
                parent: self.file_path.to_owned(),
                dir: module_dir.to_owned(),
                name,
            }),
        }
    }

    fn walk_impl(&mut self, item_impl: &syn::ItemImpl) {
        if item_impl.trait_.is_some() {
            return;
        }
        let Some(matched) = self.matching_type(&item_impl.self_ty) else {
            return;
        };
        // Record whether this is a match before checking whether our shard is
        // responsible for the file. Otherwise a shard that owned none of a
        // type's impl blocks would wrongly report the type as not found.
        self.matched_types.insert(matched.index);

        let ty = &self.types[matched.index];
        let impl_skip = if has_cfg(&item_impl.attrs) {
            Some(MethodSkipReason::CfgOnImpl)
        } else if !item_impl.generics.params.is_empty() {
            Some(MethodSkipReason::ImplHasGenerics)
        } else {
            match matched.arguments {
                SelfTypeArguments::None => None,
                SelfTypeArguments::Present => {
                    Some(MethodSkipReason::SelfTypeHasGenericArguments)
                }
            }
        };
        for impl_item in &item_impl.items {
            let syn::ImplItem::Fn(method) = impl_item else {
                continue;
            };
            match method.vis {
                syn::Visibility::Public(_) => {}
                syn::Visibility::Restricted(_) | syn::Visibility::Inherited => {
                    continue;
                }
            }
            let sig = &method.sig;
            if sig.asyncness.is_none() {
                continue;
            }
            let name = MethodName::from_ident(&sig.ident);
            let reason = impl_skip.or_else(|| method_skip(method));
            match reason {
                Some(reason) => self.record_skip(SkippedItem::Method {
                    ty_path: ty.path.clone(),
                    name,
                    reason,
                }),
                None => self.record_preinstantiated(PreinstantiatedMethod {
                    file: self.file_path.to_owned(),
                    ty_path: ty.path.clone(),
                    name,
                    arity: sig.inputs.len(),
                    line: sig.ident.span().start().line,
                }),
            }
        }
    }

    fn matching_type(&self, ty: &syn::Type) -> Option<MatchedType> {
        let syn::Type::Path(type_path) = ty else {
            return None;
        };
        if type_path.qself.is_some() {
            return None;
        }
        let last = type_path.path.segments.last()?;
        let name = TypeName::from_ident(&last.ident);
        let index = self.types.iter().position(|t| t.name == name)?;
        let arguments = if last.arguments.is_none() {
            SelfTypeArguments::None
        } else {
            SelfTypeArguments::Present
        };
        Some(MatchedType { index, arguments })
    }

    fn record_preinstantiated(&mut self, method: PreinstantiatedMethod) {
        match self.ownership {
            FileOwnership::ThisShard => {
                self.analysis.preinstantiated.push(method)
            }
            FileOwnership::OtherShard => {}
        }
    }

    fn record_skip(&mut self, item: SkippedItem) {
        let skipped = Skipped { file: self.file_path.to_owned(), item };

        // Record skipped modules, including ones in files that another shard
        // is responsible for.
        //
        // For example, suppose that `lib.rs` has:
        //
        //     #[cfg(feature = "db")]
        //     mod db;
        //
        // and all of `DataStore`'s impl blocks are under `db/`. Because we
        // don't go into `cfg()` modules, the walk never reads `db/`, so it
        // doesn't find any `impl DataStore` blocks, and the analysis fails with
        // `TypesNotFound`. The error lists `src/lib.rs: mod db: has a cfg or
        // cfg_attr`, so that the user can see where the impl blocks went.
        //
        // Now suppose there are two shards, and shard 0 is responsible for
        // `lib.rs`:
        //
        // * Both shards still read `lib.rs`, both skip `mod db`, and both fail
        //   with `TypesNotFound`, because neither one ever reads `db/`.
        // * If only shard 0 recorded the skipped item, shard 1's error would
        //   only say that `DataStore` wasn't found, and suggest checking the
        //   type path and the crate root. Both of those are already correct,
        //   so the error would be misleading.
        // * The user may well see only shard 1's error. The shards' build
        //   scripts run in parallel, and Cargo stops queueing up new build jobs
        //   after the first one fails, so shard 0's error might never show up.
        //
        // So every shard records the skipped item, whichever shard is
        // responsible for the file.
        match &skipped.item {
            SkippedItem::CfgFile | SkippedItem::Module { .. } => {
                self.unanalyzed_modules.push(skipped.clone());
            }
            SkippedItem::Method { .. } => {}
        }

        // But the report only lists skipped items from this shard's own files.
        match self.ownership {
            FileOwnership::ThisShard => self.analysis.skipped.push(skipped),
            FileOwnership::OtherShard => {}
        }
    }
}

struct MatchedType {
    index: usize,
    arguments: SelfTypeArguments,
}

enum SelfTypeArguments {
    None,
    Present,
}

fn method_skip(method: &syn::ImplItemFn) -> Option<MethodSkipReason> {
    let sig = &method.sig;
    if has_cfg(&method.attrs) {
        return Some(MethodSkipReason::CfgOnMethod);
    }
    // A cfg'd parameter causes the method's arity to be different depending on
    // whether it's enabled. That's far too annoying to deal with, so skip over
    // methods with cfg on parameters.
    if sig.inputs.iter().any(|input| match input {
        syn::FnArg::Receiver(receiver) => has_cfg(&receiver.attrs),
        syn::FnArg::Typed(pat_type) => has_cfg(&pat_type.attrs),
    }) {
        return Some(MethodSkipReason::CfgOnParameter);
    }
    // A #[target_feature] function doesn't implement the Fn traits, so the
    // generated call wouldn't compile.
    if method.attrs.iter().any(|a| a.path().is_ident("target_feature")) {
        return Some(MethodSkipReason::TargetFeature);
    }
    // A function with a non-Rust ABI doesn't implement the Fn traits either.
    // (An `extern` with no ABI name means "C".)
    if let Some(abi) = &sig.abi {
        let is_rust_abi =
            abi.name.as_ref().is_some_and(|name| name.value() == "Rust");
        if !is_rust_abi {
            return Some(MethodSkipReason::ExternAbi);
        }
    }
    match sig.safety {
        syn::Safety::Unsafe(_) => return Some(MethodSkipReason::Unsafe),
        syn::Safety::Safe(_) | syn::Safety::Default => {}
    }
    if sig.generics.params.iter().any(|p| match p {
        syn::GenericParam::Lifetime(_) => false,
        syn::GenericParam::Type(_) | syn::GenericParam::Const(_) => true,
    }) {
        return Some(MethodSkipReason::TypeOrConstGenerics);
    }

    // Can't instantiate methods with impl Trait parameters.
    if has_impl_trait_argument(sig) {
        return Some(MethodSkipReason::ImplTraitArgument);
    }

    // Can't instantiate methods with too many arguments.
    if sig.inputs.len() > MAX_ARITY {
        return Some(MethodSkipReason::TooManyArguments(sig.inputs.len()));
    }
    None
}

fn has_cfg(attrs: &[syn::Attribute]) -> bool {
    attrs.iter().any(attr_may_be_cfg)
}

// If a `cfg_attr` could expand to one of the following attributes, the item
// it's on is skipped rather than pre-instantiated. A `cfg_attr` only applies
// under some configurations, and we can't tell from the source which ones will
// be active. So we skip the item to be safe: missing a method only costs build
// time, but a wrong entry breaks the shard's build.
//
// * If it expands to `cfg`, the item might not exist. For example,
//   `#[cfg_attr(unix, cfg(test))]`.
// * If it expands to `path`, the module might come from a different file. For
//   example, `#[cfg_attr(windows, path = "imp_windows.rs")]`.
// * If it expands to `target_feature`, the method doesn't implement the `Fn`
//   traits, so the generated call wouldn't compile. For example,
//   `#[cfg_attr(feature = "simd", target_feature(enable = "avx2"))]`.
// * If it expands to another `cfg_attr`, that one might expand to any of the
//   above.
//
// Any other attribute is fine either way. For example, with `#[cfg_attr(test,
// allow(dead_code))]` or `#[cfg_attr(feature = "tracing",
// tracing::instrument)]` on a method, the generated call compiles whether or
// not the attribute applies. (This is all admittedly quite heuristic!)
const CFG_ATTR_EXPANSIONS_THAT_COUNT: [&str; 4] =
    ["cfg", "cfg_attr", "path", "target_feature"];

fn attr_may_be_cfg(attr: &syn::Attribute) -> bool {
    if attr.path().is_ident("cfg") {
        return true;
    }
    if !attr.path().is_ident("cfg_attr") {
        return false;
    }
    // The predicate is parsed on its own because it can be a bare `true` or
    // `false`, which isn't a Meta.
    let attrs = attr.parse_args_with(|input: syn::parse::ParseStream<'_>| {
        if input.peek(syn::LitBool) {
            input.parse::<syn::LitBool>()?;
        } else {
            input.parse::<syn::Meta>()?;
        }
        input.parse::<syn::Token![,]>()?;
        Punctuated::<syn::Meta, syn::Token![,]>::parse_terminated(input)
    });
    match attrs {
        Ok(attrs) => attrs.iter().any(|meta| {
            CFG_ATTR_EXPANSIONS_THAT_COUNT
                .iter()
                .any(|name| meta.path().is_ident(name))
        }),
        // A cfg_attr that doesn't parse counts as a cfg, to be safe.
        Err(_) => true,
    }
}

fn has_impl_trait_argument(sig: &syn::Signature) -> bool {
    struct ImplTraitFinder {
        found: bool,
    }

    impl<'ast> Visit<'ast> for ImplTraitFinder {
        fn visit_type_impl_trait(&mut self, _: &'ast syn::TypeImplTrait) {
            self.found = true;
        }
    }

    let mut finder = ImplTraitFinder { found: false };
    for input in &sig.inputs {
        finder.visit_fn_arg(input);
    }
    finder.found
}
