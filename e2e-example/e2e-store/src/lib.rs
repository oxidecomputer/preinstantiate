// SPDX-License-Identifier: 0BSD

mod orders;
mod users;

pub struct DataStore {
    name: String,
}

impl DataStore {
    pub fn new(name: &str) -> Self {
        Self { name: name.to_owned() }
    }

    pub async fn name(&self) -> &str {
        &self.name
    }
}
