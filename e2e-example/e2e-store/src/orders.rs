// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use crate::DataStore;

// `AuditToken` is private to this crate, so rustc rejects any use of
// `record_audit` from another crate, including a shard's generated call to it.
// preinstantiate_config.rs handles this by passing in `record_audit` to
// `skip_method`.
pub(crate) struct AuditToken;

impl DataStore {
    pub async fn connect(name: &str) -> DataStore {
        DataStore::new(name)
    }

    pub async fn order_total(&self, quantity: u64, unit_price: u64) -> u64 {
        quantity * unit_price
    }

    #[allow(private_interfaces)]
    pub async fn record_audit(&self, _token: AuditToken) {}
}
