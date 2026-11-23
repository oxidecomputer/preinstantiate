// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

include!(concat!(env!("OUT_DIR"), "/preinstantiate_async_fns.rs"));

// In a release build, share-generics is off, so the generated function is empty
// and doesn't name either crate. Without these lines, Cargo's
// `cargo::unused_dependencies` lint would cause "unused dependency `e2e-store`"
// and "unused dependency `preinstantiate`" warnings to occur.
use e2e_store as _;
use preinstantiate as _;
