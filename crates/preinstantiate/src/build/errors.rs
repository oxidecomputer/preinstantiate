// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use super::imp::MethodName;
use camino::Utf8PathBuf;
use std::fmt;

/// An error that occurred while pre-instantiating methods.
///
/// Returned by [`run_build_script`](super::run_build_script).
#[derive(Debug)]
#[non_exhaustive]
pub enum Error {
    /// An error occurred while writing the output file.
    ///
    /// For example, the build script couldn't write
    /// `preinstantiate_async_fns.rs` to `OUT_DIR` because the disk is full.
    WriteOutput {
        /// The path to the output file.
        path: Utf8PathBuf,
        /// The error that occurred.
        error: std::io::Error,
    },
    /// An environment variable is missing.
    ///
    /// Cargo is expected to set this variable for build scripts.
    ///
    /// For example, `OUT_DIR` isn't set because the build script was run
    /// outside of Cargo.
    MissingEnv {
        /// The name of the missing environment variable.
        name: &'static str,
    },
    /// An environment variable is invalid UTF-8.
    ///
    /// For example, `CARGO_MANIFEST_DIR` isn't valid UTF-8 because the crate is
    /// in a directory whose name isn't valid UTF-8.
    NonUtf8Env {
        /// The name of the environment variable.
        name: &'static str,
    },
    /// The configuration is invalid.
    ///
    /// For example, a type path passed to `add_async_inherent_methods` doesn't
    /// parse, or `skip_method` is called twice for the same method. Every
    /// problem with the configuration is reported at once.
    InvalidConfig(Vec<ConfigError>),
    /// The sources could not be read.
    ///
    /// For example, the crate root passed to `Config::new` doesn't exist, or a
    /// module file exists but can't be read because of its permissions. (A
    /// `mod foo;` whose file doesn't exist is reported as a warning instead.)
    ReadSources(Vec<ReadError>),
    /// The types could not be found.
    ///
    /// For example, `add_async_inherent_methods("db::DataStore")` was called,
    /// but the crate has no `impl DataStore` block. That can happen if the
    /// crate root points at the wrong crate, or if every `impl DataStore` block
    /// is in a module behind `#[cfg(...)]`.
    TypesNotFound {
        /// The root of the crate.
        crate_root: Utf8PathBuf,
        /// The types that were not found.
        types: Vec<String>,
        /// The types that were not analyzed.
        unanalyzed: Vec<String>,
    },
    /// The unused skipped methods could not be found.
    ///
    /// For example, `silo_delete` was passed to `skip_method` but has since
    /// been renamed, or it's unsafe and so is left out anyway.
    UnusedSkippedMethods {
        /// The root of the crate.
        crate_root: Utf8PathBuf,
        /// The methods that were not found.
        methods: Vec<UnusedSkippedMethod>,
        /// The methods that were not analyzed.
        unanalyzed: Vec<String>,
    },
    /// The `OPT_LEVEL` environment variable is not recognized.
    ///
    /// For example, `OPT_LEVEL` is set to `4`. Cargo only sets it to `0`, `1`,
    /// `2`, `3`, `s` or `z`.
    UnknownOptLevel {
        /// The value of the `OPT_LEVEL` environment variable.
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
                write_unanalyzed(
                    f,
                    unanalyzed,
                    "impl blocks",
                    UnanalyzedLeadIn::SameLine,
                )
            }
            Error::UnusedSkippedMethods { crate_root, methods, unanalyzed } => {
                match methods.len() {
                    1 => write!(
                        f,
                        "1 skip_method call has no effect in the crate at \
                         {crate_root}, so fix or remove it:"
                    )?,
                    count => write!(
                        f,
                        "{count} skip_method calls have no effect in the \
                         crate at {crate_root}, so fix or remove each one:"
                    )?,
                }
                for method in methods {
                    write!(f, "\n  - {method}")?;
                }
                write_unanalyzed(
                    f,
                    unanalyzed,
                    "methods",
                    UnanalyzedLeadIn::NewLine,
                )
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
            | Error::UnusedSkippedMethods { .. }
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

#[derive(Clone, Copy, Debug)]
enum UnanalyzedLeadIn {
    SameLine,
    NewLine,
}

fn write_unanalyzed(
    f: &mut fmt::Formatter<'_>,
    unanalyzed: &[String],
    what: &str,
    lead_in: UnanalyzedLeadIn,
) -> fmt::Result {
    if !unanalyzed.is_empty() {
        match lead_in {
            UnanalyzedLeadIn::SameLine => f.write_str(". ")?,
            UnanalyzedLeadIn::NewLine => f.write_str("\n")?,
        }
        write!(f, "These weren't analyzed, so {what} in them aren't seen:")?;
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

pub(super) fn causes<'a>(
    error: &'a (dyn std::error::Error + 'static),
) -> impl Iterator<Item = &'a (dyn std::error::Error + 'static)> {
    std::iter::successors(error.source(), |cause| cause.source())
}

/// A skipped method that isn't used.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UnusedSkippedMethod {
    /// The type path as passed to `add_async_inherent_methods`.
    pub ty_path: String,
    /// The method name without `r#`.
    pub name: MethodName,
    /// The reason the skip method was unused.
    pub kind: UnusedSkippedMethodKind,
}

/// The reason a `skip_method` call had no effect.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum UnusedSkippedMethodKind {
    /// No `pub async fn` with this name was found in an inherent impl block of
    /// the type.
    ///
    /// For example, `skip_method("db::DataStore", "silo_delete")`, where
    /// `DataStore` has no `pub async fn silo_delete`, or where it's only in a
    /// module behind `#[cfg(test)]`.
    NotFound,
    /// Every such method was found, but is left out for a different reason,
    /// like being unsafe.
    ///
    /// For example, `skip_method("db::DataStore", "u")`, where `u` is a
    /// `pub async unsafe fn`. Unsafe methods are never pre-instantiated, so the
    /// `skip_method` call has no effect.
    AlreadyLeftOut {
        /// The reason the method was left out.
        reason: String,
    },
}

impl fmt::Display for UnusedSkippedMethod {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let method = format!("{}::{}", self.ty_path, self.name);
        match &self.kind {
            UnusedSkippedMethodKind::NotFound => write!(
                f,
                "`{method}`: no `pub async fn` with this name was found in \
                 an inherent impl block of the type"
            ),
            UnusedSkippedMethodKind::AlreadyLeftOut { reason } => {
                write!(f, "`{method}`: already left out ({reason})")
            }
        }
    }
}

/// A configuration error.
#[derive(Debug)]
#[non_exhaustive]
pub enum ConfigError {
    /// The crate root path is invalid.
    ///
    /// For example, `Config::new("", shard)`. The crate root has to name a
    /// file, like `../my-crate/src/lib.rs`.
    InvalidCrateRoot {
        /// The path that is invalid.
        path: Utf8PathBuf,
    },
    /// No types were configured for pre-instantiation.
    ///
    /// For example, a `Config` that `add_async_inherent_methods` was never
    /// called on.
    NoTypes,
    /// The type path is invalid.
    ///
    /// For example, `add_async_inherent_methods("a::Wrapper<u8>")`, which has
    /// generic arguments.
    InvalidTypePath {
        /// The path that is invalid.
        path: String,
        /// The error that occurred while parsing the type path.
        error: TypePathError,
    },
    /// A type path is duplicated.
    ///
    /// For example, `add_async_inherent_methods("up::S")` followed by
    /// `add_async_inherent_methods("::up::S")`. A leading `::` doesn't make the
    /// two paths different.
    DuplicateType {
        /// The path that is duplicated.
        path: String,
    },
    /// A type name is duplicated.
    ///
    /// For example, `add_async_inherent_methods("a::DataStore")` and
    /// `add_async_inherent_methods("b::DataStore")`. Impl blocks are matched by
    /// the last segment alone, so the two types can't be told apart.
    DuplicateTypeName {
        /// The name that is duplicated.
        name: String,
        /// The first occurrence of the name.
        first: String,
        /// The second occurrence of the name.
        second: String,
    },
    /// For a skipped method, the type path is invalid.
    ///
    /// For example, `skip_method("a::Wrapper<u8>", "f")`, whose type path has
    /// generic arguments.
    InvalidSkippedMethodTypePath {
        /// The type path that is invalid.
        ty_path: String,
        /// The name of the method.
        name: String,
        /// The error that occurred while parsing the type path.
        error: TypePathError,
    },
    /// For a skipped method, the type path does not match the configured path.
    ///
    /// For example, `skip_method("DataStore", "f")` after
    /// `add_async_inherent_methods("a::DataStore")`.
    SkippedMethodTypePathMismatch {
        /// The type path that is invalid.
        ty_path: String,
        /// The name of the method.
        name: String,
        /// The configured name of the type.
        configured: String,
    },
    /// For a skipped method, the type path was not configured.
    ///
    /// For example, `skip_method("unconfigured::Type", "f")`, where
    /// `unconfigured::Type` was never passed to `add_async_inherent_methods`.
    SkippedMethodForUnconfiguredType {
        /// The type path that wasn't configured.
        ty_path: String,
        /// The name of the skipped method.
        name: String,
    },
    /// For a skipped method, the name is invalid.
    ///
    /// For example, `skip_method("up::S", "self")`, or
    /// `skip_method("up::S", " f ")` with spaces around the name.
    InvalidSkippedMethodName {
        /// The type path of the skipped method.
        ty_path: String,
        /// The name of the skipped method.
        name: String,
    },
    /// For a skipped method, the type path is duplicated.
    ///
    /// For example, `skip_method("up::S", "g")` followed by
    /// `skip_method("::up::S", "g")`, or `skip_method("up::S", "type")`
    /// followed by `skip_method("up::S", "r#type")`.
    DuplicateSkippedMethod {
        /// The type path of the skipped method.
        ty_path: String,
        /// The name of the skipped method.
        name: String,
    },
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
            ConfigError::SkippedMethodForUnconfiguredType { ty_path, name } => {
                write!(
                    f,
                    "`skip_method({ty_path:?}, {name:?})`: `{ty_path}` wasn't \
                     passed to add_async_inherent_methods, so pass it there \
                     too, or fix the type path"
                )
            }
            ConfigError::InvalidSkippedMethodTypePath {
                ty_path, name, ..
            } => write!(
                f,
                "`skip_method({ty_path:?}, {name:?})` has an invalid type path"
            ),
            ConfigError::SkippedMethodTypePathMismatch {
                ty_path,
                name,
                configured,
            } => write!(
                f,
                "`skip_method({ty_path:?}, {name:?})`: `{ty_path}` wasn't \
                 passed to add_async_inherent_methods, but `{configured}` \
                 was, so pass `{configured}` here instead"
            ),
            ConfigError::InvalidSkippedMethodName { ty_path, name } => write!(
                f,
                "`skip_method({ty_path:?}, {name:?})` has an invalid method \
                 name: pass the name alone, like `silo_delete_internal` or \
                 `r#type`"
            ),
            ConfigError::DuplicateSkippedMethod { ty_path, name } => write!(
                f,
                "`skip_method({ty_path:?}, {name:?})` is a duplicate of an \
                 earlier skip_method call for the same method, so remove one \
                 of them"
            ),
        }
    }
}

impl std::error::Error for ConfigError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            ConfigError::InvalidTypePath { error, .. }
            | ConfigError::InvalidSkippedMethodTypePath { error, .. } => {
                Some(error)
            }
            ConfigError::InvalidCrateRoot { .. }
            | ConfigError::NoTypes
            | ConfigError::DuplicateType { .. }
            | ConfigError::DuplicateTypeName { .. }
            | ConfigError::SkippedMethodTypePathMismatch { .. }
            | ConfigError::SkippedMethodForUnconfiguredType { .. }
            | ConfigError::InvalidSkippedMethodName { .. }
            | ConfigError::DuplicateSkippedMethod { .. } => None,
        }
    }
}

/// An error that occurred while reading a file.
#[derive(Debug)]
pub struct ReadError {
    /// The path of the file that could not be read.
    pub path: Utf8PathBuf,
    /// The error that occurred while reading the file.
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

/// An error that occurred while parsing a type path.
#[derive(Debug)]
#[non_exhaustive]
pub enum TypePathError {
    /// The type path could not be parsed.
    Parse(syn::Error),
    /// The type path contains generic arguments, which are not supported.
    GenericArguments,
    /// The type path contains a segment that's a keyword, such as `a::super::`.
    KeywordSegment {
        /// The segment that's a keyword.
        segment: String,
    },
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

/// A shard index is out of range.
#[derive(Debug)]
pub struct InvalidShardError {
    /// The index of the shard that is out of range.
    ///
    /// A valid `index` must be less than `count`.
    pub index: u32,
    /// The number of shards.
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
