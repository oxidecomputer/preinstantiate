// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use super::{analysis::*, imp::*};
use camino::{Utf8Path, Utf8PathBuf};
use camino_tempfile::Utf8TempDir;
use std::{
    collections::BTreeMap,
    time::{Duration, SystemTime},
};

fn test_config(shard: ShardIndex) -> Config {
    Config::new("src/lib.rs", shard).add_async_inherent_methods("dq::DataStore")
}

fn one_shard() -> ShardIndex {
    ShardIndex::new(0, 1).expect("valid shard")
}

fn analyze_fn<R>(config: &Config, read: R) -> Result<Analysis, Error>
where
    R: FnMut(&Utf8Path) -> std::io::Result<String>,
{
    analyze(&validate(config)?, read)
}

fn analyze_with<P, C>(
    config: &Config,
    sources: &[(P, C)],
) -> Result<Analysis, Error>
where
    P: AsRef<str>,
    C: AsRef<str>,
{
    let files: BTreeMap<Utf8PathBuf, String> = sources
        .iter()
        .map(|(path, contents)| {
            (Utf8PathBuf::from(path.as_ref()), contents.as_ref().to_owned())
        })
        .collect();
    analyze_fn(config, |path| {
        files
            .get(path)
            .cloned()
            .ok_or_else(|| std::io::ErrorKind::NotFound.into())
    })
}

fn analyze_sources<P, C>(shard: ShardIndex, sources: &[(P, C)]) -> Analysis
where
    P: AsRef<str>,
    C: AsRef<str>,
{
    analyze_with(&test_config(shard), sources).expect("analysis succeeded")
}

/// Returns a textual representation of the report and generated code ready to
/// be snapshotted.
fn report_and_generated(config: &Config, analysis: &Analysis) -> String {
    let validated = validate(config).expect("config is valid");
    format!(
        "# report\n{}\n# generated\n{}",
        render_report(analysis),
        render(&validated, analysis)
    )
}

fn two_shards() -> [ShardIndex; 2] {
    [0, 1].map(|index| ShardIndex::new(index, 2).expect("valid shard"))
}

/// Finds a module name whose file is owned by the given shard owns.
///
/// For example, `module_owned_by(shard_1, "broken")` tries `broken_0.rs`,
/// `broken_1.rs` and so on, and returns the first name whose file hashes to
/// shard 1, say `broken_3`. That lets a test put something in a file it knows
/// shard 1 owns, without hardcoding hash values.
fn module_owned_by(shard: ShardIndex, prefix: &str) -> String {
    (0..)
        .map(|i| format!("{prefix}_{i}"))
        .find(|name| shard.contains(Utf8Path::new(&format!("{name}.rs"))))
        .expect("some file name hashes to each shard")
}

#[test]
fn method_filtering() {
    let analysis = analyze_sources(
        one_shard(),
        &[(
            "lib.rs",
            r#"
            impl DataStore {
                pub async fn plain(&self) {}
                pub async fn no_receiver() {}
                pub async fn with_args(&self, a: u32, b: &str) {}
                pub async fn with_lifetime<'a>(&'a self, x: &'a str) {}
                pub async fn at_max(
                    &self, a: u8, b: u8, c: u8, d: u8, e: u8, f: u8, g: u8,
                    h: u8, i: u8, j: u8, k: u8,
                ) {}
                #[deprecated]
                pub async fn deprecated(&self) {}
                pub async fn type_generic<T>(&self, t: T) {}
                pub async fn const_generic<const N: usize>(&self) {}
                pub async fn impl_trait(&self, x: impl Into<String>) {}
                pub async fn nested_impl_trait(&self, x: Vec<impl Sized>) {}
                pub async unsafe fn unsafe_fn(&self) {}
                #[cfg(test)]
                pub async fn cfg_fn(&self) {}
                #[cfg_attr(test, allow(unused))]
                pub async fn cfg_attr_fn(&self) {}
                #[cfg_attr(feature = "tracing", tracing::instrument)]
                pub async fn cfg_attr_instrument(&self) {}
                #[cfg_attr(test, cfg(test))]
                pub async fn cfg_attr_cfg(&self) {}
                #[cfg_attr(feature = "simd", target_feature(enable = "avx2"))]
                pub async fn cfg_attr_target_feature(&self) {}
                pub async fn cfg_param(&self, #[cfg(test)] a: u8) {}
                pub async fn cfg_receiver(#[cfg(test)] &self) {}
                #[target_feature(enable = "avx2")]
                pub async fn target_feature(&self) {}
                pub async fn returns_impl(&self) -> impl Sized {}
                pub async extern "C" fn extern_c(&self) {}
                pub async extern "Rust" fn extern_rust(&self) {}
                pub async extern "system" fn extern_system(&self) {}
                pub async extern fn extern_bare(&self) {}
                pub async fn too_many(
                    &self, a: u8, b: u8, c: u8, d: u8, e: u8, f: u8, g: u8,
                    h: u8, i: u8, j: u8, k: u8, l: u8,
                ) {}
                pub fn not_async(&self) {}
                pub(crate) async fn crate_visible(&self) {}
                async fn private(&self) {}
            }
            impl super::DataStore {
                pub async fn via_path(&self) {}
            }
            #[cfg(test)]
            impl DataStore {
                pub async fn in_cfg_impl(&self) {}
            }
            impl DataStore {
                #![cfg(test)]
                pub async fn in_inner_cfg_impl(&self) {}
            }
            impl<'a> DataStore {
                pub async fn in_generic_impl(&self) {}
            }
            impl DataStore<u8> {
                pub async fn in_args_impl(&self) {}
            }
            impl OtherType {
                pub async fn other_type(&self) {}
            }
            "#,
        )],
    );

    expectorate::assert_contents(
        "tests/output/method_filtering.txt",
        &report_and_generated(&test_config(one_shard()), &analysis),
    );
}

#[test]
fn module_tree() {
    let analysis = analyze_sources(
        one_shard(),
        &[
            (
                "lib.rs",
                r#"
                //! Crate docs.
                #![allow(unused)]
                #![cfg_attr(test, allow(unused))]
                mod a;
                mod b;
                #[cfg(test)]
                mod test_only;
                #[cfg(feature = "testing")]
                mod inline_cfg {
                    impl DataStore { pub async fn in_inline_cfg(&self) {} }
                }
                mod inline {
                    mod nested;
                    mod missing;
                    impl DataStore { pub async fn in_inline(&self) {} }
                }
                mod inner_cfg {
                    #![cfg(test)]
                    impl DataStore { pub async fn in_inner_cfg(&self) {} }
                }
                mod inner_allow {
                    //! Module docs.
                    #![cfg_attr(test, allow(unused))]
                    impl DataStore { pub async fn in_inner_allow(&self) {} }
                }
                #[path = "elsewhere.rs"]
                mod moved;
                mod cfg_file;
                mod cfg_attr_file;
                mod cfg_attr_later_file;
                mod cfg_attr_nested_file;
                mod cfg_attr_unparsed_file;
                mod cfg_attr_true_file;
                #[cfg_attr(windows, path = "imp_windows.rs")]
                mod cfg_attr_path;
                #[cfg_attr(test, allow(unused))]
                mod cfg_attr_allow;
                "#,
            ),
            (
                "a.rs",
                r#"
                mod child;
                mod inner {
                    mod x;
                    #[cfg(test)]
                    mod tests {}
                }
                impl DataStore { pub async fn in_a(&self) {} }
                "#,
            ),
            (
                "a/child.rs",
                "impl DataStore { pub async fn in_a_child(&self) {} }",
            ),
            (
                "a/inner/x.rs",
                "impl DataStore { pub async fn in_a_inner_x(&self) {} }",
            ),
            ("b/mod.rs", "mod c;"),
            ("b/c.rs", "impl DataStore { pub async fn in_b_c(&self) {} }"),
            (
                "inline/nested.rs",
                "impl DataStore { pub async fn in_inline_nested(&self) {} }",
            ),
            (
                "test_only.rs",
                "impl DataStore { pub async fn in_test_only(&self) {} }",
            ),
            (
                "elsewhere.rs",
                "impl DataStore { pub async fn in_elsewhere(&self) {} }",
            ),
            (
                "cfg_file.rs",
                "#![cfg(test)] mod below; \
                 impl DataStore { pub async fn in_cfg_file(&self) {} }",
            ),
            (
                "cfg_file/below.rs",
                "impl DataStore { pub async fn below_cfg_file(&self) {} }",
            ),
            (
                "cfg_attr_file.rs",
                "#![cfg_attr(unix, cfg(test))] \
                 impl DataStore { pub async fn in_cfg_attr_file(&self) {} }",
            ),
            (
                "cfg_attr_later_file.rs",
                "#![cfg_attr(test, allow(unused), cfg(test))] \
                 impl DataStore { pub async fn in_cfg_attr_later(&self) {} }",
            ),
            (
                "cfg_attr_nested_file.rs",
                "#![cfg_attr(unix, cfg_attr(test, cfg(test)))] \
                 impl DataStore { pub async fn in_cfg_attr_nested(&self) {} }",
            ),
            (
                "cfg_attr_unparsed_file.rs",
                "#![cfg_attr(test)] \
                 impl DataStore { \
                 pub async fn in_cfg_attr_unparsed(&self) {} }",
            ),
            (
                "cfg_attr_allow.rs",
                "impl DataStore { pub async fn in_cfg_attr_allow(&self) {} }",
            ),
            (
                "cfg_attr_true_file.rs",
                "#![cfg_attr(true, allow(unused))] \
                 impl DataStore { pub async fn in_cfg_attr_true(&self) {} }",
            ),
        ],
    );

    expectorate::assert_contents(
        "tests/output/module_tree.txt",
        &report_and_generated(&test_config(one_shard()), &analysis),
    );
}

#[test]
fn parse_error_is_a_problem() {
    let analysis = analyze_sources(
        one_shard(),
        &[
            ("lib.rs", "mod broken; mod fine;"),
            (
                "broken.rs",
                concat!(
                    "impl DataStore {\n",
                    "    pub async fn ok(&self) {}\n",
                    "    pub async fn broken(&self) -> {}\n",
                    "}\n",
                ),
            ),
            ("fine.rs", "impl DataStore { pub async fn ok(&self) {} }"),
        ],
    );
    // In the snapshot, line 3 of broken.rs is
    // `    pub async fn broken(&self) -> {}`, and column 35 is the `{` where a
    // return type should be. The walk carries on past broken.rs, so fine.rs's
    // `ok` is still collected.
    expectorate::assert_contents(
        "tests/output/parse_error_is_a_problem.txt",
        &report_and_generated(&test_config(one_shard()), &analysis),
    );
}

#[test]
fn shards_partition_files() {
    let lib: String = (0..50).map(|i| format!("mod m{i};\n")).collect();
    let sources: Vec<(String, String)> =
        std::iter::once(("lib.rs".to_owned(), lib))
            .chain((0..50).map(|i| {
                (
                    format!("m{i}.rs"),
                    format!(
                        "impl DataStore {{ pub async fn f{i}(&self) {{}} }}"
                    ),
                )
            }))
            .collect();

    let count = 4;
    let mut all = Vec::new();
    for index in 0..count {
        let shard = ShardIndex::new(index, count).expect("valid shard");
        let analysis = analyze_sources(shard, &sources);
        assert!(
            !analysis.preinstantiated.is_empty(),
            "shard {index} of {count} has no methods",
        );
        assert_eq!(analysis.problems, vec![]);
        all.extend(
            analysis.preinstantiated.into_iter().map(|m| m.name.to_string()),
        );
    }
    all.sort();
    let mut expected: Vec<String> = (0..50).map(|i| format!("f{i}")).collect();
    expected.sort();
    assert_eq!(all, expected);
}

#[test]
fn shard_assignment_ignores_how_the_path_is_spelled() {
    // Shard assignment hashes a path's components rather than its text, so that
    // a file goes to the same shard on every platform.
    //
    // For example, the shard for `db\datastore\vpc.rs` on Windows should match
    // the shard for `db/datastore/vpc.rs` on Unix. Otherwise the same file
    // could be compiled by (say) shard 2 on one machine and shard 5 on another,
    // which is confusing when comparing reports or build timings.
    //
    // The `//` and `./` spellings below exercise the same property on Linux,
    // since the Windows case only runs on Windows.
    let canonical = Utf8Path::new("db/datastore/vpc.rs");
    let mut spellings = vec![Utf8PathBuf::from("db//datastore/./vpc.rs")];
    if cfg!(windows) {
        spellings.push(Utf8PathBuf::from(r"db\datastore\vpc.rs"));
    }
    for count in 1..=8 {
        for index in 0..count {
            let shard = ShardIndex::new(index, count).expect("valid shard");
            for spelling in &spellings {
                assert_eq!(
                    shard.contains(spelling),
                    shard.contains(canonical),
                    "{spelling} in shard {index} of {count}",
                );
            }
        }
    }
}

#[test]
fn invalid_shard_index() {
    for (index, count) in [(3, 3), (0, 0)] {
        let error = ShardIndex::new(index, count)
            .expect_err("the shard index is out of range");
        assert_eq!((error.index, error.count), (index, count));
    }
}

#[test]
fn render_output() {
    let analysis = analyze_sources(
        one_shard(),
        &[(
            "lib.rs",
            "impl DataStore {\n    \
             pub async fn a(&self) {}\n    \
             pub async fn r#type(&self, x: u8) {}\n    \
             pub async fn gen(&self) {}\n\
             }\n",
        )],
    );
    expectorate::assert_contents(
        "tests/output/render.rs.txt",
        &render(
            &validate(&test_config(one_shard())).expect("config is valid"),
            &analysis,
        ),
    );
}

#[test]
fn multiple_types() {
    let config = Config::new("src/lib.rs", one_shard())
        .add_async_inherent_methods("dq::DataStore")
        .add_async_inherent_methods("::other::r#Pool");
    let analysis = analyze_with(
        &config,
        &[(
            "lib.rs",
            r#"
            impl DataStore { pub async fn ds(&self) {} }
            impl crate::Pool { pub async fn pool(&self, x: u8) {} }
            impl Unlisted { pub async fn unlisted(&self) {} }
            "#,
        )],
    )
    .expect("analysis succeeded");
    expectorate::assert_contents(
        "tests/output/multiple_types.txt",
        &report_and_generated(&config, &analysis),
    );
}

#[test]
fn unmatched_type_is_an_error() {
    let config = Config::new("src/lib.rs", one_shard())
        .add_async_inherent_methods("dq::DataStore")
        .add_async_inherent_methods("dq::DataStor");
    // The unsafe method `u` is skipped, but it must not appear in the error's
    // list of things that weren't analyzed. That list is for modules and files
    // the walk never read, and the walk did read `u`.
    let error = analyze_with(
        &config,
        &[(
            "lib.rs",
            "impl DataStore { \
             pub async fn ds(&self) {} \
             pub async unsafe fn u(&self) {} \
             } \
             impl SomeTrait for DataStor { async fn f(&self) {} }",
        )],
    )
    .expect_err("a type with only a trait impl block fails the analysis");
    expectorate::assert_contents(
        "tests/output/unmatched_type_is_an_error.txt",
        &error.to_string(),
    );
}

#[test]
fn types_not_found_lists_skips_from_every_shard() {
    // Each cfg'd module is in a file that only one of the shards owns, so both
    // shards listing both modules shows that the list contains every shard's
    // files, not just the current one's.
    let owners = two_shards().map(|shard| module_owned_by(shard, "holder"));
    let lib_rs = format!(
        "mod {}; mod {}; mod missing; mod broken; mod cfg_file;",
        owners[0], owners[1]
    );
    let sources = [
        ("lib.rs".to_owned(), lib_rs),
        (format!("{}.rs", owners[0]), "#[cfg(test)] mod db;".to_owned()),
        (format!("{}.rs", owners[1]), "#[cfg(test)] mod db;".to_owned()),
        ("broken.rs".to_owned(), "impl DataStore {".to_owned()),
        ("cfg_file.rs".to_owned(), "#![cfg(test)]".to_owned()),
    ];
    let [message_0, message_1] = two_shards().map(|shard| {
        analyze_with(&test_config(shard), &sources)
            .expect_err("a type with no inherent impl block fails the analysis")
            .to_string()
    });
    assert_eq!(message_0, message_1, "both shards report the same error");
    expectorate::assert_contents(
        "tests/output/types_not_found_lists_skips_from_every_shard.txt",
        &message_0,
    );
}

#[test]
fn type_matched_in_another_shard_is_found() {
    // `DataStore`'s only impl block is in `only.rs`, and only one of the two
    // shards is responsible for that file, say shard 1. Shard 0 doesn't collect
    // `f`, but it still has to notice `impl DataStore` in `only.rs`. Otherwise
    // it would conclude that `DataStore` doesn't exist and fail with
    // TypesNotFound. So this checks that:
    //
    // * Both shards succeed.
    // * Only one of them collects `f`.
    let sources = [
        ("lib.rs", "mod only;"),
        ("only.rs", "impl DataStore { pub async fn f(&self) {} }"),
    ];
    let mut method_counts = Vec::new();
    for shard in two_shards() {
        let analysis = analyze_with(&test_config(shard), &sources)
            .expect("neither shard fails with TypesNotFound");
        method_counts.push(analysis.preinstantiated.len());
    }
    method_counts.sort();
    assert_eq!(method_counts, vec![0, 1]);
}

#[test]
fn no_types_is_a_config_error() {
    let error = validate(&Config::new("src/lib.rs", one_shard()))
        .expect_err("a config with no types fails");
    expectorate::assert_contents(
        "tests/output/no_types_is_a_config_error.txt",
        &error.to_string(),
    );
}

#[test]
fn config_errors_are_all_reported() {
    let config = Config::new("", one_shard())
        .add_async_inherent_methods("not a path")
        .add_async_inherent_methods("a::Wrapper<u8>")
        .add_async_inherent_methods("a::crate::B")
        .add_async_inherent_methods("a::DataStore")
        .add_async_inherent_methods("a::DataStore")
        .add_async_inherent_methods("b::DataStore")
        .add_async_inherent_methods("up::S")
        .add_async_inherent_methods("::up::S")
        // This shows two things:
        //
        // * No error is reported for `skip_method("b::DataStore", "f")`,
        //   because `b::DataStore` ends in the same name as `a::DataStore`, and
        //   that's already reported.
        // * `skip_method("S", "not an identifier")` has both a bad type path
        //   and a bad method name, and both are reported.
        .skip_method("unconfigured::Type", "f")
        .skip_method("not a path", "f")
        .skip_method("a::Wrapper<u8>", "f")
        .skip_method("b::DataStore", "f")
        .skip_method("DataStore", "f")
        .skip_method("S", "not an identifier")
        .skip_method("up::S", "not an identifier")
        .skip_method("up::S", "self")
        .skip_method("up::S", "crate")
        .skip_method("up::S", "super")
        .skip_method("up::S", "Self")
        .skip_method("up::S", "_")
        .skip_method("up::S", "")
        .skip_method("up::S", " g ")
        .skip_method("up::S", "g")
        .skip_method("::up::S", "g")
        .skip_method("up::S", "type")
        .skip_method("up::S", "r#type");
    let error = validate(&config).expect_err("the config is invalid");
    expectorate::assert_contents(
        "tests/output/config_errors_are_all_reported.txt",
        &error.to_string(),
    );
}

#[test]
fn skipped_methods() {
    // This shows how method names passed to skip_method are matched:
    //
    // * `"r#type"` matches `pub async fn r#type`, and `"match"` matches
    //   `pub async fn r#match`.
    // * `u` is unsafe and isn't passed to skip_method, so the report gives "is
    //   unsafe" as its reason.
    // * `split` and `late` each have a second copy in a cfg'd impl block. The
    //   cfg'd copy keeps the cfg reason, and the plain copy is reported as
    //   passed to skip_method.
    // * `late`'s cfg'd copy comes before its plain one, and `split`'s comes
    //   after, which shows that the order doesn't matter.
    let config = test_config(one_shard())
        .skip_method("dq::DataStore", "b")
        .skip_method("::dq::DataStore", "r#type")
        .skip_method("dq::DataStore", "match")
        .skip_method("dq::DataStore", "split")
        .skip_method("dq::DataStore", "late");
    let analysis = analyze_with(
        &config,
        &[(
            "lib.rs",
            r#"
            #[cfg(test)]
            impl DataStore {
                pub async fn late(&self) {}
            }
            impl DataStore {
                pub async fn a(&self) {}
                pub async fn b(&self) {}
                pub async fn r#type(&self) {}
                pub async fn r#match(&self) {}
                pub async unsafe fn u(&self) {}
                pub async fn split(&self) {}
                pub async fn late(&self) {}
            }
            #[cfg(test)]
            impl DataStore {
                pub async fn split(&self) {}
            }
            "#,
        )],
    )
    .expect("analysis succeeded");
    expectorate::assert_contents(
        "tests/output/skipped_methods.txt",
        &report_and_generated(&config, &analysis),
    );
}

#[test]
fn unused_skipped_methods_are_an_error() {
    // This shows which skip_method calls are listed in the error:
    //
    // * `a` would otherwise be pre-instantiated, so `skip_method(..., "a")` has
    //   an effect and isn't listed.
    // * Each of the other methods passed to skip_method is either not a
    //   `pub async fn` in an analyzed inherent impl block, or already left out
    //   for another reason.
    // * `hidden` is a `pub async fn`, but it's in a module the walk doesn't
    //   read.
    let config = test_config(one_shard())
        .skip_method("dq::DataStore", "a")
        .skip_method("dq::DataStore", "missing")
        .skip_method("dq::DataStore", "private")
        .skip_method("dq::DataStore", "crate_visible")
        .skip_method("dq::DataStore", "not_async")
        .skip_method("dq::DataStore", "trait_method")
        .skip_method("dq::DataStore", "hidden")
        .skip_method("dq::DataStore", "u")
        .skip_method("dq::DataStore", "generic")
        .skip_method("dq::DataStore", "cfg_split");
    let error = analyze_with(
        &config,
        &[
            (
                "lib.rs",
                "#[cfg(test)] mod tests; \
                 impl DataStore { \
                 pub async fn a(&self) {} \
                 async fn private(&self) {} \
                 pub(crate) async fn crate_visible(&self) {} \
                 pub fn not_async(&self) {} \
                 pub async unsafe fn u(&self) {} \
                 pub async fn generic<T>(&self, t: T) {} \
                 } \
                 impl SomeTrait for DataStore { \
                 async fn trait_method(&self) {} \
                 } \
                 #[cfg(unix)] impl DataStore { \
                 pub async fn cfg_split(&self) {} \
                 } \
                 #[cfg(windows)] impl DataStore { \
                 pub async fn cfg_split(&self) {} \
                 }",
            ),
            ("tests.rs", "impl DataStore { pub async fn hidden(&self) {} }"),
        ],
    )
    .expect_err("skip_method calls with no effect fail the analysis");
    expectorate::assert_contents(
        "tests/output/unused_skipped_methods_are_an_error.txt",
        &error.to_string(),
    );
}

#[test]
fn already_left_out_error_omits_unanalyzed_modules() {
    // This shows that when every method in the error was found, the error
    // doesn't list modules that weren't analyzed.
    //
    // `u` is in `lib.rs` and is left out because it's unsafe, so `mod tests`
    // has nothing to do with the error.
    let config = test_config(one_shard()).skip_method("dq::DataStore", "u");
    let error = analyze_with(
        &config,
        &[(
            "lib.rs",
            "#[cfg(test)] mod tests; \
             impl DataStore { pub async unsafe fn u(&self) {} }",
        )],
    )
    .expect_err("a skip_method call for a method that's already left out fails the analysis");
    expectorate::assert_contents(
        "tests/output/already_left_out_error_omits_unanalyzed_modules.txt",
        &error.to_string(),
    );
}

#[test]
fn skipped_method_in_another_shards_file_is_used() {
    // This shows that every shard records a skipped method as seen:
    //
    // * `f` is only in `only.rs`, and only one of the two shards is responsible
    //   for that file.
    // * The other shard's analysis still has to record `f` as seen. Otherwise,
    //   that shard's build would fail with an error saying that
    //   `skip_method(..., "f")` has no effect.
    let sources = [
        ("lib.rs", "mod only;"),
        ("only.rs", "impl DataStore { pub async fn f(&self) {} }"),
    ];
    for shard in two_shards() {
        let config = test_config(shard).skip_method("dq::DataStore", "f");
        analyze_with(&config, &sources).expect(
            "neither shard reports the skip_method call as having no effect",
        );
    }
}

#[test]
fn skipped_methods_apply_to_one_type() {
    // This shows that skip_method applies to one type.
    //
    // * Both types have a method `f`.
    // * Only `Other::f` is passed to skip_method, so `DataStore::f` is still
    //   pre-instantiated.
    let config = test_config(one_shard())
        .add_async_inherent_methods("dq::Other")
        .skip_method("dq::Other", "f");
    let analysis = analyze_with(
        &config,
        &[(
            "lib.rs",
            "impl DataStore { pub async fn f(&self) {} } \
             impl Other { pub async fn f(&self) {} }",
        )],
    )
    .expect("analysis succeeded");
    expectorate::assert_contents(
        "tests/output/skipped_methods_apply_to_one_type.txt",
        &report_and_generated(&config, &analysis),
    );
}

#[test]
fn types_not_found_is_reported_before_unused_skipped_methods() {
    // When a type isn't found, the crate root or a type path is usually wrong,
    // so the error only says that. More specifically, we don't list
    // `skip_method(..., "stale")` or the `skip_method` call for `dq::Missing`,
    // since they would be noise until the type is detected.
    let config = test_config(one_shard())
        .add_async_inherent_methods("dq::Missing")
        .skip_method("dq::DataStore", "stale")
        .skip_method("dq::Missing", "f");
    let error = analyze_with(
        &config,
        &[("lib.rs", "impl DataStore { pub async fn a(&self) {} }")],
    )
    .expect_err("a type with no inherent impl block fails the analysis");
    expectorate::assert_contents(
        "tests/output/types_not_found_is_reported_before_unused_skipped_methods.txt",
        &error.to_string(),
    );
}

#[test]
fn unused_skipped_methods_are_reported_by_every_shard() {
    // This shows that each shard's analysis takes every file into account, not
    // only its own:
    //
    // * `u` and the cfg'd module are in files that different shards are
    //   responsible for.
    // * Both shards report the same error.
    let owners = two_shards().map(|shard| module_owned_by(shard, "holder"));
    let sources = [
        ("lib.rs".to_owned(), format!("mod {}; mod {};", owners[0], owners[1])),
        (
            format!("{}.rs", owners[0]),
            "impl DataStore { pub async unsafe fn u(&self) {} }".to_owned(),
        ),
        (format!("{}.rs", owners[1]), "#[cfg(test)] mod db;".to_owned()),
    ];
    let [message_0, message_1] = two_shards().map(|shard| {
        let config = test_config(shard)
            .skip_method("dq::DataStore", "u")
            .skip_method("dq::DataStore", "missing");
        analyze_with(&config, &sources)
            .expect_err("skip_method calls with no effect fail the analysis")
            .to_string()
    });
    assert_eq!(message_0, message_1, "both shards report the same error");
    expectorate::assert_contents(
        "tests/output/unused_skipped_methods_are_reported_by_every_shard.txt",
        &message_0,
    );
}

#[test]
fn keyword_segments() {
    // The configured path goes into the shard's generated code as written, so
    // each of these works as long as it the last component is the type from
    // where the shard include!s the generated file. For example,
    // `crate::DataStore` works if the shard's lib.rs has `use
    // nexus_db_queries::db::DataStore;`.
    let mut out = String::new();
    for path in [
        "DataStore",
        "crate::DataStore",
        "self::DataStore",
        "super::DataStore",
        "super::super::DataStore",
        "self::super::DataStore",
        "::crate::A",
        "::super::A",
        "a::super::B",
        "crate::super::A",
        "self::self::A",
        "super::self::A",
        "a::Self::B",
        "Self::DataStore",
    ] {
        let config = Config::new("src/lib.rs", one_shard())
            .add_async_inherent_methods(path);
        let result = match validate(&config) {
            Ok(_) => "ok".to_owned(),
            Err(error) => error.to_string(),
        };
        out.push_str(&format!("{path}: {result}\n"));
    }
    expectorate::assert_contents("tests/output/keyword_segments.txt", &out);
}

#[test]
fn read_errors_are_all_reported() {
    let config = Config::new("../dq/src/lib.rs", one_shard())
        .add_async_inherent_methods("dq::DataStore");
    let error = analyze_fn(&config, |path| match path.as_str() {
        "lib.rs" => {
            Ok("mod a; mod b; mod c; mod d; impl DataStore {}".to_owned())
        }
        "c.rs" => Ok("mod deep;".to_owned()),
        "a.rs" | "b/mod.rs" | "c/deep.rs" => {
            Err(std::io::ErrorKind::PermissionDenied.into())
        }
        "d/mod.rs" => Err(std::io::ErrorKind::NotADirectory.into()),
        _ => Err(std::io::ErrorKind::NotFound.into()),
    })
    .expect_err("read errors other than a missing file fail the analysis");
    expectorate::assert_contents(
        "tests/output/read_errors_are_all_reported.txt",
        &error.to_string(),
    );
}

#[test]
fn missing_crate_root_is_a_read_error() {
    let error = analyze_fn(&test_config(one_shard()), |_| {
        Err(std::io::ErrorKind::NotFound.into())
    })
    .expect_err("a missing crate root fails the analysis");
    expectorate::assert_contents(
        "tests/output/missing_crate_root_is_a_read_error.txt",
        &error.to_string(),
    );
}

#[test]
fn each_warning_and_skip_comes_from_one_shard() {
    let shards = two_shards();
    let broken = shards.map(|shard| module_owned_by(shard, "broken"));
    // `mod missing;` has no associated file. That's a problem for every shard's
    // analysis, but it shouldn't produce a warning in any shard, because rustc
    // already reports it.
    let lib_rs = format!(
        "mod {}; mod {}; mod missing; #[cfg(test)] mod tests; \
         impl DataStore {{}}",
        broken[0], broken[1]
    );
    let sources = [
        ("lib.rs".to_owned(), lib_rs),
        (format!("{}.rs", broken[0]), "impl DataStore {".to_owned()),
        (format!("{}.rs", broken[1]), "impl DataStore {".to_owned()),
    ];

    let mut all_skipped = Vec::new();
    for (shard, owned) in shards.iter().zip(&broken) {
        let config = test_config(*shard);
        let analysis =
            analyze_with(&config, &sources).expect("analysis succeeded");
        let problem_files: Vec<&str> =
            analysis.problems.iter().map(|p| p.file.as_str()).collect();
        let mut expected_problem_files = vec![
            format!("{}.rs", broken[0]),
            format!("{}.rs", broken[1]),
            "lib.rs".to_owned(),
        ];
        expected_problem_files.sort();
        assert_eq!(
            problem_files, expected_problem_files,
            "every shard sees every problem"
        );
        let warnings = problem_warnings(
            &validate(&config).expect("config is valid"),
            &analysis,
        );
        let [warning] = warnings.as_slice() else {
            panic!("expected one warning, for {owned}.rs, got {warnings:?}");
        };
        assert!(
            warning.starts_with(&format!(
                "preinstantiate: src/{owned}.rs: failed to parse at line "
            )),
            "got {warning:?}",
        );
        all_skipped.extend(analysis.skipped);
    }
    assert_eq!(
        all_skipped,
        vec![Skipped {
            file: "lib.rs".into(),
            item: SkippedItem::Module {
                path: "tests".to_owned(),
                reason: ModuleSkipReason::Cfg,
            },
        }],
    );
}

#[test]
fn problem_warnings_are_one_line() {
    let config = Config::new("../dq/src/lib.rs", one_shard())
        .add_async_inherent_methods("dq::DataStore");
    let analysis = Analysis {
        problems: vec![SourceProblem {
            file: "lib.rs".into(),
            kind: SourceProblemKind::ParseError {
                line: 3,
                column: 7,
                message: "first line\nsecond line".to_owned(),
            },
        }],
        ..Analysis::default()
    };
    assert_eq!(
        problem_warnings(
            &validate(&config).expect("config is valid"),
            &analysis
        ),
        vec![
            "preinstantiate: ../dq/src/lib.rs: failed to parse at line 3, \
             column 7: first line second line, so methods in it and its \
             submodules aren't pre-instantiated"
        ],
    );
}

#[test]
fn raw_module_names() {
    let analysis = analyze_sources(
        one_shard(),
        &[
            (
                "lib.rs",
                r#"
                mod r#async;
                mod r#match {
                    mod inner;
                }
                "#,
            ),
            (
                "async.rs",
                "impl DataStore { pub async fn in_raw_file(&self) {} }",
            ),
            (
                "match/inner.rs",
                "impl DataStore { pub async fn in_raw_inline_child(&self) {} }",
            ),
        ],
    );
    expectorate::assert_contents(
        "tests/output/raw_module_names.txt",
        &report_and_generated(&test_config(one_shard()), &analysis),
    );
}

struct BuildDirs {
    _temp: Utf8TempDir,
    manifest_dir: Utf8PathBuf,
    out_dir: Utf8PathBuf,
}

fn build_dirs(lib_rs: &str) -> BuildDirs {
    let temp = Utf8TempDir::new().expect("created temp dir");
    let manifest_dir = temp.path().join("manifest");
    let out_dir = temp.path().join("out");
    std::fs::create_dir_all(manifest_dir.join("src")).expect("created src dir");
    std::fs::create_dir_all(&out_dir).expect("created out dir");
    std::fs::write(manifest_dir.join("src/lib.rs"), lib_rs)
        .expect("wrote lib.rs");
    BuildDirs { _temp: temp, manifest_dir, out_dir }
}

impl BuildDirs {
    fn read_outputs(&self) -> (String, String) {
        let generated = std::fs::read_to_string(
            self.out_dir.join("preinstantiate_async_fns.rs"),
        )
        .expect("read generated file");
        let report = std::fs::read_to_string(
            self.out_dir.join("preinstantiate_async_fns-report.txt"),
        )
        .expect("read report");
        (generated, report)
    }
}

fn build_env(dirs: &BuildDirs, opt_level: OptLevel) -> BuildEnv {
    BuildEnv {
        out_dir: dirs.out_dir.clone(),
        manifest_dir: dirs.manifest_dir.clone(),
        opt_level,
    }
}

#[test]
fn build_script_validates_config_when_share_generics_is_off() {
    let dirs = build_dirs("impl DataStore {}");
    let config = Config::new("src/lib.rs", one_shard());
    let error = run_with_env(&config, &build_env(&dirs, OptLevel::O3))
        .expect_err("an invalid config fails even at opt-level 3");
    let Error::InvalidConfig(_) = error else {
        panic!("expected InvalidConfig, got {error:?}");
    };
}

#[test]
fn stale_skipped_method_is_only_caught_when_share_generics_is_on() {
    // At opt-level 3 (which we use a proxy for share-generics), the build
    // script doesn't analyze the source, so there's no way to tell that
    // `skip_method(..., "missing")` is stale. The build has to succeed anyway.
    let dirs = build_dirs("impl DataStore { pub async fn f(&self) {} }");
    let config =
        test_config(one_shard()).skip_method("dq::DataStore", "missing");
    run_with_env(&config, &build_env(&dirs, OptLevel::O3)).expect(
        "a stale skip_method call doesn't fail a build without analysis",
    );
    let error = run_with_env(&config, &build_env(&dirs, OptLevel::O0))
        .expect_err("a stale skip_method call fails a build with analysis");
    let Error::UnusedSkippedMethods { .. } = error else {
        panic!("expected UnusedSkippedMethods, got {error:?}");
    };
}

#[test]
fn build_script_with_share_generics_off() {
    let dirs = build_dirs("impl DataStore { pub async fn f(&self) {} }");
    run_with_env(&test_config(one_shard()), &build_env(&dirs, OptLevel::O3))
        .expect("build script succeeded");
    let (generated, report) = dirs.read_outputs();
    // Code that calls preinstantiate_async_fns, like the e2e tests, has to
    // compile in release builds too. So the file has to define an empty
    // function..
    let config = test_config(one_shard());
    let validated = validate(&config).expect("config is valid");
    assert_eq!(generated, render(&validated, &Analysis::default()));
    assert_eq!(
        report,
        "analysis skipped: share-generics is off, because opt-level is 3\n"
    );
}

#[test]
fn build_script_with_share_generics_on() {
    let dirs = build_dirs(
        "mod missing; \
         impl DataStore { \
         pub async fn f(&self) {} \
         pub async unsafe fn u(&self) {} \
         }",
    );
    run_with_env(&test_config(one_shard()), &build_env(&dirs, OptLevel::O0))
        .expect("build script succeeded");
    let (generated, report) = dirs.read_outputs();
    expectorate::assert_contents(
        "tests/output/build_script_with_share_generics_on.txt",
        &format!("# report\n{report}\n# generated\n{generated}"),
    );
}

#[test]
fn write_failure_keeps_its_cause() {
    let temp = Utf8TempDir::new().expect("created temp dir");
    let path = temp.path().join("missing/out.txt");
    let error = write_if_changed(&path, "x")
        .expect_err("the parent directory is missing");
    let Error::WriteOutput { path: error_path, .. } = &error else {
        panic!("expected WriteOutput, got {error:?}");
    };
    assert_eq!(error_path, &path);
    assert!(std::error::Error::source(&error).is_some(), "got {error:?}");
}

#[test]
fn write_if_changed_leaves_identical_files_alone() {
    let temp = Utf8TempDir::new().expect("created temp dir");
    let path = temp.path().join("out.txt");
    std::fs::write(&path, "same").expect("wrote file");
    // Some filesystems store modification times in whole seconds, so a rewrite
    // within the same second could leave the time unchanged. Setting the time
    // to 1970 first means any rewrite would always show up as a different time.
    let old = SystemTime::UNIX_EPOCH + Duration::from_secs(1_000_000);
    std::fs::File::options()
        .write(true)
        .open(&path)
        .expect("opened file")
        .set_modified(old)
        .expect("set mtime");

    write_if_changed(&path, "same").expect("write succeeded");
    let modified = std::fs::metadata(&path)
        .expect("read metadata")
        .modified()
        .expect("read mtime");
    assert_eq!(modified, old);

    write_if_changed(&path, "different").expect("write succeeded");
    assert_eq!(std::fs::read_to_string(&path).expect("read file"), "different");
}
