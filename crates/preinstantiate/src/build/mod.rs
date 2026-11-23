// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Build-time support.
//!
//! This module includes support for configuring build scripts.

mod analysis;
mod errors;
mod imp;
#[cfg(test)]
mod tests;

pub use errors::{
    ConfigError, Error, InvalidShardError, ReadError, TypePathError,
    UnusedSkippedMethod, UnusedSkippedMethodKind,
};
pub use imp::{
    Config, MethodName, ShardIndex, run_build_script, run_build_script_or_exit,
};
