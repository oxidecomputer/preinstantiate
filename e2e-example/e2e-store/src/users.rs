// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use crate::DataStore;

impl DataStore {
    pub async fn user_name(&self, id: u32) -> String {
        format!("user-{id}")
    }

    pub async fn r#type(&self) -> &'static str {
        "users"
    }

    pub async fn user_with_extra<T: Send>(&self, id: u32, _extra: T) -> u32 {
        id
    }
}
