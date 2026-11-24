// SPDX-License-Identifier: 0BSD

use preinstantiate::build::{Config, ShardIndex, run_build_script_or_exit};

include!("../preinstantiate_config.rs");

fn main() {
    let shard = ShardIndex::new(0, 2).expect("shard 0 of 2 is valid");
    run_build_script_or_exit(&config(shard));
}
