// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

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
