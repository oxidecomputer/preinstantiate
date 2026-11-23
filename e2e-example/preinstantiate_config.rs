// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

fn config(shard: ShardIndex) -> Config {
    Config::new("../e2e-store/src/lib.rs", shard)
        .add_async_inherent_methods("e2e_store::DataStore")
        .skip_method("e2e_store::DataStore", "record_audit")
}
