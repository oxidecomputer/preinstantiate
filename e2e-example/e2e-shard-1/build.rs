// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use preinstantiate::build::{Config, ShardIndex, run_build_script_or_exit};

fn main() {
    let shard = ShardIndex::new(1, 2).expect("shard 1 of 2 is valid");
    let config = Config::new("../e2e-store/src/lib.rs", shard)
        .add_async_inherent_methods("e2e_store::DataStore");
    run_build_script_or_exit(&config);
}
