// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// The first two uses silence Cargo's `unused-dependencies` lint.
use e2e_shard_0 as _;
use e2e_shard_1 as _;
use e2e_store::DataStore;

pub async fn order_summary(
    store: &DataStore,
    user_id: u32,
    quantity: u64,
) -> String {
    let name = store.name().await;
    let user = store.user_name(user_id).await;
    let total = store.order_total(quantity, 25).await;
    format!("{name}: {user} owes {total}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        future::Future,
        pin::pin,
        task::{Context, Poll, Waker},
    };

    fn poll_once<F: Future>(future: F) -> F::Output {
        let mut future = pin!(future);
        // The underlying futures don't register with the waker, so a noop waker
        // is sufficient.
        let mut cx = Context::from_waker(Waker::noop());
        match future.as_mut().poll(&mut cx) {
            Poll::Ready(output) => output,
            Poll::Pending => panic!("future completed on the first poll"),
        }
    }

    #[test]
    fn order_summary_runs() {
        let store = poll_once(DataStore::connect("shop"));
        assert_eq!(
            poll_once(order_summary(&store, 7, 4)),
            "shop: user-7 owes 100",
        );
    }
}
