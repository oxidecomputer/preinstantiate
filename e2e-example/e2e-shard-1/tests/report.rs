// SPDX-License-Identifier: 0BSD

const REPORT: &str = include_str!(concat!(
    env!("OUT_DIR"),
    "/preinstantiate_async_fns-report.txt"
));

#[test]
fn report() {
    // The test can't observe the opt-level, so it uses debug assertions to tell
    // the two profiles apart. In this workspace, the dev profile has opt-level
    // 0 and debug assertions on, and the release profile has opt-level 3 and
    // debug assertions off.
    if cfg!(debug_assertions) {
        expectorate::assert_contents("tests/output/report.txt", REPORT);
    } else {
        assert_eq!(
            REPORT,
            "analysis skipped: share-generics is off, because opt-level is 3\n",
        );
    }
}

#[test]
fn preinstantiate_async_fns_links() {
    e2e_shard_1::preinstantiate_async_fns();
}
