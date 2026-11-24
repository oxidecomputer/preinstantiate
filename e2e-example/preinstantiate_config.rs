// SPDX-License-Identifier: 0BSD

fn config(shard: ShardIndex) -> Config {
    Config::new("../e2e-store/src/lib.rs", shard)
        .add_async_inherent_methods("e2e_store::DataStore")
        .skip_method("e2e_store::DataStore", "record_audit")
}
