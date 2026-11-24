// SPDX-License-Identifier: 0BSD

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
