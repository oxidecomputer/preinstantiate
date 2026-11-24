// SPDX-License-Identifier: 0BSD

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
