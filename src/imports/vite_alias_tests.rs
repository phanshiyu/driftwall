//! Vite `resolve.alias` through [`collect`] on repositories written to disk.

use super::fixtures::{pairs, repo};
use crate::model::SkipReason;

const OBJECT_CONFIG: &str = "import path from 'node:path';\nexport default defineConfig({\n  resolve: {\n    alias: {\n      'mrc-wasm': path.resolve(__dirname, 'engine/wasm/pkg'),\n      '@': './src',\n    },\n  },\n});\n";

const ARRAY_CONFIG: &str = "export default {\n  resolve: {\n    alias: [\n      { find: 'mrc-wasm', replacement: fileURLToPath(new URL('./engine/wasm/pkg', import.meta.url)) },\n      { find: '@', replacement: './src' },\n    ],\n  },\n};\n";

fn skipped(
    repo: &super::fixtures::Repo,
    file: &str,
) -> Vec<(u32, String, SkipReason, Option<String>)> {
    repo.imports(file)
        .skipped
        .iter()
        .map(|s| (s.at.line, s.specifier.clone(), s.reason, s.message.clone()))
        .collect()
}

const MISSING: &str =
    "alias `mrc-wasm` points to `engine/wasm/pkg`, which has no file in the repository";

#[test]
fn an_alias_to_a_missing_build_output_is_skipped_with_its_own_reason() {
    for config in [OBJECT_CONFIG, ARRAY_CONFIG] {
        let repo = repo(&[
            ("vite.config.ts", config),
            ("src/player/Canvas.tsx", "import init from 'mrc-wasm';\n"),
        ]);
        assert_eq!(repo.edges("src/player/Canvas.tsx"), Vec::new());
        assert_eq!(
            skipped(&repo, "src/player/Canvas.tsx"),
            [(
                1,
                "mrc-wasm".to_owned(),
                SkipReason::AliasTargetMissing,
                Some(MISSING.to_owned())
            )],
            "{config}"
        );
    }
}

#[test]
fn an_alias_with_a_path_below_its_key_resolves_like_a_relative_path() {
    for config in [OBJECT_CONFIG, ARRAY_CONFIG] {
        let repo = repo(&[
            ("vite.config.ts", config),
            ("src/lib/index.ts", "export const a = 1;\n"),
            ("src/lib/util.ts", "export const b = 1;\n"),
            (
                "src/app/main.ts",
                "import { a } from '@/lib';\nimport { b } from '@/lib/util';\nimport { c } from '@/lib/gone';\n",
            ),
        ]);
        assert_eq!(
            repo.edges("src/app/main.ts"),
            pairs(&[(1, "src/lib/index.ts"), (2, "src/lib/util.ts")]),
            "{config}"
        );
        assert_eq!(repo.skipped("src/app/main.ts"), Vec::new(), "{config}");
    }
}

#[test]
fn an_alias_applies_below_the_config_directory_only() {
    let repo = repo(&[
        (
            "app/vite.config.ts",
            "export default { resolve: { alias: { shared: './lib' } } };\n",
        ),
        ("app/lib/index.ts", "export const a = 1;\n"),
        ("app/src/x.ts", "import 'shared';\n"),
        ("other/y.ts", "import 'shared';\n"),
    ]);
    assert_eq!(
        repo.edges("app/src/x.ts"),
        pairs(&[(1, "app/lib/index.ts")])
    );
    assert_eq!(repo.edges("other/y.ts"), Vec::new());
    assert_eq!(repo.skipped("other/y.ts"), pairs(&[(1, "shared")]));
    assert_eq!(
        skipped(&repo, "other/y.ts")[0].2,
        SkipReason::UndeclaredPackage
    );
}

#[test]
fn the_nearest_config_decides_and_a_farther_one_still_applies_to_other_keys() {
    let repo = repo(&[
        (
            "vite.config.ts",
            "export default { resolve: { alias: { a: './one', b: './two' } } };\n",
        ),
        (
            "app/vite.config.ts",
            "export default { resolve: { alias: { a: './three' } } };\n",
        ),
        ("one/index.ts", ""),
        ("two/index.ts", ""),
        ("app/three/index.ts", ""),
        ("app/x.ts", "import 'a';\nimport 'b';\n"),
    ]);
    assert_eq!(
        repo.edges("app/x.ts"),
        pairs(&[(1, "app/three/index.ts"), (2, "two/index.ts")])
    );
}

#[test]
fn a_tsconfig_path_that_finds_a_file_wins_over_an_alias() {
    let repo = repo(&[
        (
            "tsconfig.json",
            r#"{"compilerOptions":{"paths":{"lib":["./real/index.ts"]}}}"#,
        ),
        (
            "vite.config.ts",
            "export default { resolve: { alias: { lib: './other' } } };\n",
        ),
        ("real/index.ts", ""),
        ("other/index.ts", ""),
        ("x.ts", "import 'lib';\n"),
    ]);
    assert_eq!(repo.edges("x.ts"), pairs(&[(1, "real/index.ts")]));
}

#[test]
fn an_alias_to_an_existing_directory_without_the_file_is_silent() {
    let repo = repo(&[
        (
            "vite.config.ts",
            "export default { resolve: { alias: { lib: './lib' } } };\n",
        ),
        ("lib/readme.md", ""),
        ("x.ts", "import 'lib/missing';\nimport 'lib';\n"),
    ]);
    assert_eq!(repo.edges("x.ts"), Vec::new());
    assert_eq!(repo.skipped("x.ts"), Vec::new());
    assert_eq!(repo.unresolved("x.ts"), Vec::new());
}

#[test]
fn a_key_that_only_shares_a_prefix_is_not_the_alias() {
    let repo = repo(&[
        (
            "vite.config.ts",
            "export default { resolve: { alias: { lib: './lib' } } };\n",
        ),
        ("lib/index.ts", ""),
        ("x.ts", "import 'library';\n"),
    ]);
    assert_eq!(repo.skipped("x.ts"), pairs(&[(1, "library")]));
    assert_eq!(skipped(&repo, "x.ts")[0].2, SkipReason::UndeclaredPackage);
}
