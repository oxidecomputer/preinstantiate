// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use super::analysis::{
    Analysis, SourceProblemKind, ValidatedConfig, analyze, validate,
};
use atomicwrites::{AtomicFile, OverwriteBehavior};
use camino::{Utf8Path, Utf8PathBuf};
use std::{fmt, fmt::Write as _, io::Write as _};

/// Configuration for a build script.
#[derive(Clone, Debug)]
pub struct Config {
    pub(super) crate_root: Utf8PathBuf,
    pub(super) shard: ShardIndex,
    pub(super) async_inherent_method_types: Vec<String>,
}

impl Config {
    /// Creates a new configuration.
    ///
    /// `crate_root` is the crate's `lib.rs` relative to the build script's
    /// manifest directory, and `shard` is the portion of the pre-instantiate
    /// this build script corresponds to.
    pub fn new<P>(crate_root: P, shard: ShardIndex) -> Self
    where
        P: Into<Utf8PathBuf>,
    {
        Self {
            crate_root: crate_root.into(),
            shard,
            async_inherent_method_types: Vec::new(),
        }
    }

    /// Pre-instantiates `pub async` functions of the type at the specified
    /// path.
    pub fn add_async_inherent_methods<S>(mut self, ty_path: S) -> Self
    where
        S: Into<String>,
    {
        self.async_inherent_method_types.push(ty_path.into());
        self
    }
}

/// The current and total shard indexes.
///
/// This determines which portion of a crate this build script is
/// pre-instantiating.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ShardIndex {
    index: u32,
    count: u32,
}

impl ShardIndex {
    /// Creates a new [`ShardIndex`] with the given index and total count.
    ///
    /// `index` is 0-based.
    ///
    /// Returns an error if the index is greater than or equal to the count.
    pub fn new(index: u32, count: u32) -> Result<Self, InvalidShardError> {
        if index < count {
            Ok(Self { index, count })
        } else {
            Err(InvalidShardError { index, count })
        }
    }

    /// Returns the index of this shard.
    pub fn index(&self) -> u32 {
        self.index
    }

    /// Returns the total number of shards.
    pub fn count(&self) -> u32 {
        self.count
    }

    /// Determines if this shard contains the given relative path.
    pub fn contains(&self, rel_path: &Utf8Path) -> bool {
        // This is FNV-1a. It's super easy so we just inline it here rather than
        // import a crate for it.
        fn fnv1a_step(hash: u64, byte: u8) -> u64 {
            (hash ^ u64::from(byte)).wrapping_mul(0x0000_0100_0000_01b3)
        }

        let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
        for (i, component) in rel_path.iter().enumerate() {
            if i > 0 {
                hash = fnv1a_step(hash, b'/');
            }
            for byte in component.bytes() {
                hash = fnv1a_step(hash, byte);
            }
        }
        hash % u64::from(self.count) == u64::from(self.index)
    }
}

/// Calls [`run_build_script`], exiting the program if an error occurred.
///
/// This is what build scripts generally want.
pub fn run_build_script_or_exit(config: &Config) {
    if let Err(error) = run_build_script(config) {
        eprintln!("error: preinstantiate: {error}");
        for cause in causes(&error) {
            eprintln!("  caused by: {cause}");
        }
        std::process::exit(1);
    }
}

/// The entry point for a build script.
///
/// This writes out the following files into `OUT_DIR`:
///
/// - `preinstantiate_async_fns.rs` (to `include!` from the crate)
/// - `preinstantiate_async_fns-report.txt` for a human-friendly report.
pub fn run_build_script(config: &Config) -> Result<(), Error> {
    let env = BuildEnv {
        out_dir: env_path("OUT_DIR")?,
        manifest_dir: env_path("CARGO_MANIFEST_DIR")?,
        // We only use OPT_LEVEL to determine whether share-generics is on or
        // off, not `CARGO_ENCODED_RUSTFLAGS` with `-Zshare-generics`. (It's
        // unlikely the latter is used within Omicron.)
        opt_level: OptLevel::parse(&env_string("OPT_LEVEL")?)?,
    };
    run_with_env(config, &env)
}

#[derive(Debug)]
pub(super) struct BuildEnv {
    pub(super) out_dir: Utf8PathBuf,
    pub(super) manifest_dir: Utf8PathBuf,
    pub(super) opt_level: OptLevel,
}

pub(super) fn run_with_env(
    config: &Config,
    env: &BuildEnv,
) -> Result<(), Error> {
    let config = validate(config)?;
    let source_dir = env.manifest_dir.join(config.source_dir());

    let (analysis, report) = match env.opt_level.share_generics() {
        ShareGenerics::On => {
            println!("cargo::rerun-if-changed={source_dir}");
            let analysis = analyze(&config, |rel_path| {
                std::fs::read_to_string(source_dir.join(rel_path))
            })?;
            for warning in problem_warnings(&config, &analysis) {
                println!("cargo::warning={warning}");
            }
            let report = render_report(&analysis);
            (analysis, report)
        }
        ShareGenerics::Off => {
            let report = format!(
                "analysis skipped: share-generics is off, because \
                 opt-level is {}\n",
                env.opt_level.as_str()
            );
            (Analysis::default(), report)
        }
    };

    write_if_changed(
        &env.out_dir.join("preinstantiate_async_fns.rs"),
        &render(&config, &analysis),
    )?;
    write_if_changed(
        &env.out_dir.join("preinstantiate_async_fns-report.txt"),
        &report,
    )?;
    Ok(())
}

fn env_path(name: &'static str) -> Result<Utf8PathBuf, Error> {
    env_string(name).map(Utf8PathBuf::from)
}

fn env_string(name: &'static str) -> Result<String, Error> {
    match std::env::var(name) {
        Ok(value) => Ok(value),
        Err(std::env::VarError::NotPresent) => Err(Error::MissingEnv { name }),
        Err(std::env::VarError::NotUnicode(_)) => {
            Err(Error::NonUtf8Env { name })
        }
    }
}

pub(super) fn write_if_changed(
    path: &Utf8Path,
    contents: &str,
) -> Result<(), Error> {
    if let Ok(existing) = std::fs::read_to_string(path)
        && existing == contents
    {
        return Ok(());
    }

    AtomicFile::new(path, OverwriteBehavior::AllowOverwrite)
        .write(|file| file.write_all(contents.as_bytes()))
        .map_err(|error| Error::WriteOutput {
            path: path.to_owned(),
            error: error.into(),
        })
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum ShareGenerics {
    On,
    Off,
}

/// The opt-levels supported by rustc.
///
/// share-generics is on by default for every opt-level except 2 and 3 (see
/// `share_generics` and `OptLevel` in rustc_session's config.rs).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum OptLevel {
    O0,
    O1,
    O2,
    O3,
    Size,
    SizeMin,
}

impl OptLevel {
    fn parse(value: &str) -> Result<Self, Error> {
        match value {
            "0" => Ok(OptLevel::O0),
            "1" => Ok(OptLevel::O1),
            "2" => Ok(OptLevel::O2),
            "3" => Ok(OptLevel::O3),
            "s" => Ok(OptLevel::Size),
            "z" => Ok(OptLevel::SizeMin),
            _ => Err(Error::UnknownOptLevel { value: value.to_owned() }),
        }
    }

    fn as_str(self) -> &'static str {
        match self {
            OptLevel::O0 => "0",
            OptLevel::O1 => "1",
            OptLevel::O2 => "2",
            OptLevel::O3 => "3",
            OptLevel::Size => "s",
            OptLevel::SizeMin => "z",
        }
    }

    fn share_generics(self) -> ShareGenerics {
        match self {
            OptLevel::O0
            | OptLevel::O1
            | OptLevel::Size
            | OptLevel::SizeMin => ShareGenerics::On,
            OptLevel::O2 | OptLevel::O3 => ShareGenerics::Off,
        }
    }
}

/// Renders the `preinstantiate_async_fns` function for this shard. For example:
///
/// ```rust,ignore
/// #[doc(hidden)]
/// #[inline(never)]
/// #[allow(deprecated)]
/// pub fn preinstantiate_async_fns() {
///     // ../nexus-db-queries/src/db/datastore/silo.rs:123
///     ::preinstantiate::preinstantiate_async_fn_2(
///         <nexus_db_queries::db::DataStore>::r#silo_delete,
///     );
/// }
/// ```
///
/// When share-generics is off, `analysis` is empty, so the function exists but
/// is empty.
pub(super) fn render(
    config: &ValidatedConfig<'_>,
    analysis: &Analysis,
) -> String {
    let shard = config.shard();
    let mut out = format!(
        "// @generated by preinstantiate for shard {} of {}.\n",
        shard.index, shard.count
    );
    out.push_str("#[doc(hidden)]\n");
    out.push_str("#[inline(never)]\n");
    // If a method like `DataStore::old_query` is `#[deprecated]`, naming it
    // below would make rustc warn about this generated file. The method is
    // still worth pre-instantiating, so the warning is silenced.
    out.push_str("#[allow(deprecated)]\n");
    out.push_str("pub fn preinstantiate_async_fns() {\n");
    for method in &analysis.preinstantiated {
        // rustc usually reports errors against the generated file, not against
        // the method's own fils. So a comment like `//
        // ../nexus-db-queries/src/db/datastore/silo.rs:123` above each call can
        // be helpful at identifying the source.
        writeln!(
            out,
            "    // {}:{}",
            config.source_dir().join(&method.file),
            method.line
        )
        .expect("writing to a String succeeds");
        // * Use an `r#` prefix to avoid keyword conflicts in new editions.
        // * Use angle brackets to apply a type's default parameters: for
        //   `struct G<T = ()>` with an `impl G`, `<G>::f` works, while a
        //   bare `G::f` is ambiguous.
        writeln!(
            out,
            "    ::preinstantiate::preinstantiate_async_fn_{}(<{}>::r#{});",
            method.arity, method.ty_path, method.name
        )
        .expect("writing to a String succeeds");
    }
    out.push_str("}\n");
    out
}

/// Returns the `cargo::warning` lines for this shard's source problems.
pub(super) fn problem_warnings(
    config: &ValidatedConfig<'_>,
    analysis: &Analysis,
) -> Vec<String> {
    analysis
        .problems
        .iter()
        .filter(|problem| match problem.kind {
            // For parse errors, only display warnings for the owning shard.
            SourceProblemKind::ParseError { .. } => {
                config.shard().contains(&problem.file)
            }
            // Don't show warnings for missing module files since rustc would do
            // this anyway on the upstream crate.
            SourceProblemKind::ModuleFileNotFound { .. } => false,
        })
        .map(|problem| {
            format!(
                "preinstantiate: {}: {}, so methods in it and its submodules \
                 aren't pre-instantiated",
                config.source_dir().join(&problem.file),
                problem.kind,
            )
            // `cargo::warning` lines are displayed up to the end of the line,
            // so replace any newlines in the message with spaces. (syn doesn't
            // normally generate multi-line messages so this is more of a
            // precaution.)
            .replace('\n', " ")
        })
        .collect()
}

pub(super) fn render_report(analysis: &Analysis) -> String {
    let mut out = String::new();
    for problem in &analysis.problems {
        writeln!(out, "problem {problem}")
            .expect("writing to a String succeeds");
    }
    for method in &analysis.preinstantiated {
        writeln!(
            out,
            "preinstantiated {}: {}::{}",
            method.file, method.ty_path, method.name
        )
        .expect("writing to a String succeeds");
    }
    for skipped in &analysis.skipped {
        writeln!(out, "skipped {skipped}")
            .expect("writing to a String succeeds");
    }
    out
}

#[derive(Debug)]
pub enum Error {
    WriteOutput {
        path: Utf8PathBuf,
        error: std::io::Error,
    },
    MissingEnv {
        name: &'static str,
    },
    NonUtf8Env {
        name: &'static str,
    },
    InvalidConfig(Vec<ConfigError>),
    ReadSources(Vec<ReadError>),
    TypesNotFound {
        crate_root: Utf8PathBuf,
        types: Vec<String>,
        unanalyzed: Vec<String>,
    },
    UnknownOptLevel {
        value: String,
    },
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::WriteOutput { path, .. } => {
                write!(f, "failed to write generated file {path}")
            }
            Error::MissingEnv { name } => write!(
                f,
                "environment variable {name} is not set: run_build_script \
                 must be called from a Cargo build script"
            ),
            Error::NonUtf8Env { name } => {
                write!(f, "environment variable {name} is not valid UTF-8")
            }
            Error::InvalidConfig(errors) => {
                let noun =
                    if errors.len() == 1 { "problem" } else { "problems" };
                write!(f, "invalid configuration ({} {noun}):", errors.len())?;
                write_error_list(f, errors)
            }
            Error::ReadSources(errors) => {
                let noun = if errors.len() == 1 { "file" } else { "files" };
                write!(f, "failed to read {} source {noun}:", errors.len())?;
                write_error_list(f, errors)
            }
            Error::TypesNotFound { crate_root, types, unanalyzed } => {
                write!(
                    f,
                    "no inherent impl blocks found in the crate at \
                     {crate_root} for "
                )?;
                write_backticked_list(f, types)?;
                f.write_str(
                    ": check the type paths and the crate root (impl blocks \
                     are matched by the type's own name, not by a name it's \
                     re-exported as)",
                )?;
                write_unanalyzed(f, unanalyzed, "impl blocks")
            }
            Error::UnknownOptLevel { value } => write!(
                f,
                "unrecognized value `{value}` for the OPT_LEVEL environment \
                 variable: expected 0, 1, 2, 3, s, or z"
            ),
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Error::WriteOutput { error, .. } => Some(error),
            // source() can only return a single cause so we rely on the Display
            // impl instead.
            Error::InvalidConfig(_) | Error::ReadSources(_) => None,
            Error::MissingEnv { .. }
            | Error::NonUtf8Env { .. }
            | Error::TypesNotFound { .. }
            | Error::UnknownOptLevel { .. } => None,
        }
    }
}

fn write_backticked_list(
    f: &mut fmt::Formatter<'_>,
    items: &[String],
) -> fmt::Result {
    for (index, item) in items.iter().enumerate() {
        if index > 0 {
            f.write_str(", ")?;
        }
        write!(f, "`{item}`")?;
    }
    Ok(())
}

fn write_unanalyzed(
    f: &mut fmt::Formatter<'_>,
    unanalyzed: &[String],
    what: &str,
) -> fmt::Result {
    if !unanalyzed.is_empty() {
        write!(f, ". These weren't analyzed, so {what} in them aren't seen:")?;
        for item in unanalyzed {
            write!(f, "\n  - {item}")?;
        }
    }
    Ok(())
}

fn write_error_list<E>(f: &mut fmt::Formatter<'_>, errors: &[E]) -> fmt::Result
where
    E: std::error::Error + 'static,
{
    for error in errors {
        write!(f, "\n  - {error}")?;
        for cause in causes(error) {
            write!(f, ": {cause}")?;
        }
    }
    Ok(())
}

fn causes<'a>(
    error: &'a (dyn std::error::Error + 'static),
) -> impl Iterator<Item = &'a (dyn std::error::Error + 'static)> {
    std::iter::successors(error.source(), |cause| cause.source())
}

#[derive(Debug)]
pub enum ConfigError {
    InvalidCrateRoot { path: Utf8PathBuf },
    NoTypes,
    InvalidTypePath { path: String, error: TypePathError },
    DuplicateType { path: String },
    DuplicateTypeName { name: String, first: String, second: String },
}

impl fmt::Display for ConfigError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ConfigError::InvalidCrateRoot { path } => write!(
                f,
                "crate root `{path}` doesn't name a file: expected a path \
                 like `../my-crate/src/lib.rs`"
            ),
            ConfigError::NoTypes => f.write_str(
                "no types to pre-instantiate: call \
                 add_async_inherent_methods on the Config",
            ),
            ConfigError::InvalidTypePath { path, .. } => {
                write!(f, "invalid type path `{path}`")
            }
            ConfigError::DuplicateType { path } => {
                write!(f, "`{path}` is configured more than once")
            }
            ConfigError::DuplicateTypeName { name, first, second } => write!(
                f,
                "`{first}` and `{second}` both end in `{name}`, and impl \
                 blocks are matched by that name alone, so they can't be \
                 told apart"
            ),
        }
    }
}

impl std::error::Error for ConfigError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            ConfigError::InvalidTypePath { error, .. } => Some(error),
            ConfigError::InvalidCrateRoot { .. }
            | ConfigError::NoTypes
            | ConfigError::DuplicateType { .. }
            | ConfigError::DuplicateTypeName { .. } => None,
        }
    }
}

#[derive(Debug)]
pub struct ReadError {
    pub path: Utf8PathBuf,
    pub error: std::io::Error,
}

impl fmt::Display for ReadError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "failed to read {}", self.path)
    }
}

impl std::error::Error for ReadError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(&self.error)
    }
}

#[derive(Debug)]
pub enum TypePathError {
    Parse(syn::Error),
    GenericArguments,
    KeywordSegment { segment: String },
}

impl fmt::Display for TypePathError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            TypePathError::Parse(_) => f.write_str("failed to parse as a path"),
            TypePathError::GenericArguments => {
                f.write_str("generic arguments aren't supported")
            }
            TypePathError::KeywordSegment { segment } => write!(
                f,
                "has `{segment}` where it can't name the type: `crate`, \
                 `self` and `super` can only start a path, `super` can also \
                 follow `self` or `super`, none of them can follow a leading \
                 `::`, and `Self` doesn't exist in the generated code"
            ),
        }
    }
}

impl std::error::Error for TypePathError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            TypePathError::Parse(error) => Some(error),
            TypePathError::GenericArguments
            | TypePathError::KeywordSegment { .. } => None,
        }
    }
}

#[derive(Debug)]
pub struct InvalidShardError {
    pub index: u32,
    pub count: u32,
}

impl fmt::Display for InvalidShardError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let noun = if self.count == 1 { "shard" } else { "shards" };
        write!(
            f,
            "shard index {} is out of range: there are {} {noun}",
            self.index, self.count
        )
    }
}

impl std::error::Error for InvalidShardError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn opt_level_decides_share_generics() {
        let cases = [
            ("0", OptLevel::O0, ShareGenerics::On),
            ("1", OptLevel::O1, ShareGenerics::On),
            ("2", OptLevel::O2, ShareGenerics::Off),
            ("3", OptLevel::O3, ShareGenerics::Off),
            ("s", OptLevel::Size, ShareGenerics::On),
            ("z", OptLevel::SizeMin, ShareGenerics::On),
        ];
        for (value, expected_level, expected_share_generics) in cases {
            let level = OptLevel::parse(value).expect("opt-level is valid");
            assert_eq!(level, expected_level, "opt-level {value}");
            assert_eq!(level.as_str(), value, "opt-level {value}");
            assert_eq!(
                level.share_generics(),
                expected_share_generics,
                "opt-level {value}"
            );
        }
    }

    #[test]
    fn unknown_opt_level_is_an_error() {
        for value in ["", "4", "fast", " 0"] {
            match OptLevel::parse(value) {
                Err(Error::UnknownOptLevel { value: got }) => {
                    assert_eq!(got, value)
                }
                other => panic!(
                    "expected UnknownOptLevel for {value:?}, got {other:?}"
                ),
            }
        }
    }
}
