//! A type alias whose body is a reference to a type of another file is followed like a re-export
//! (see "Type aliases" in "Re-exports" in `docs/DESIGN.md`), so it cannot hide a dependency.

// why: clippy's allow-*-in-tests covers only cfg(test) code, and these helpers are not #[test] fns.
#[cfg(test)]
mod support {
    use std::fs;
    use std::process::{Command, Output};

    use tempfile::TempDir;

    /// A repository with `files`, linted against `contract`.
    pub fn lint(contract: &str, files: &[(&str, &str)]) -> Output {
        let dir = TempDir::new().unwrap();
        let all = std::iter::once(("architecture.mmd", contract)).chain(files.iter().copied());
        for (path, contents) in all {
            let full = dir.path().join(path);
            fs::create_dir_all(full.parent().unwrap()).unwrap();
            fs::write(full, contents).unwrap();
        }
        Command::new(env!("CARGO_BIN_EXE_driftwall"))
            .args(["lint", "--format", "text"])
            .arg(dir.path())
            .output()
            .unwrap()
    }

    pub fn stdout(output: &Output) -> String {
        String::from_utf8(output.stdout.clone()).unwrap()
    }
}

use support::{lint, stdout};

/// A feature in three parts: the controller may use the service, the service the store, and
/// nothing else is allowed.
const RUST_CONTRACT: &str = "flowchart LR
  root
  f-controller
  f-service
  f-store
  f-controller --> f-service
  f-service --> f-store
%% map root: src/lib.rs, src/f/mod.rs
%% map f-controller: src/f/controller.rs
%% map f-service: src/f/service.rs
%% map f-store: src/f/store.rs
";

/// The crate with `service` as the body of `service.rs` and `controller` as the body of
/// `controller.rs`.
fn rust(service: &str, controller: &str) -> String {
    let files = [
        (
            "Cargo.toml",
            "[package]\nname = \"demo\"\nversion = \"0.0.0\"\n",
        ),
        ("src/lib.rs", "mod f;\n"),
        (
            "src/f/mod.rs",
            "mod controller;\nmod service;\nmod store;\n",
        ),
        (
            "src/f/store.rs",
            "pub struct Review;\npub struct Page<T>(T);\n",
        ),
        ("src/f/service.rs", service),
        ("src/f/controller.rs", controller),
    ];
    stdout(&lint(RUST_CONTRACT, &files))
}

/// The error text for an import of `to` by `from`.
fn imports(from: &str, to: &str) -> String {
    format!("error[undeclared-edge]: `{from}` imports `{to}`")
}

#[test]
fn an_alias_to_another_file_cannot_hide_the_real_dependency() {
    let text = rust(
        "pub type Review = crate::f::store::Review;\n",
        "use crate::f::service::Review;\n",
    );
    assert!(text.contains(&imports("f-controller", "f-store")), "{text}");
    assert!(text.contains("src/f/controller.rs:1"), "{text}");
    assert!(
        !text.contains(&imports("f-controller", "f-service")),
        "{text}"
    );
}

#[test]
fn the_alias_keeps_its_own_allowed_edge() {
    let text = rust(
        "pub type Review = crate::f::store::Review;\n",
        "use crate::f::service::Review;\n",
    );
    assert!(!text.contains("f-service --> f-store"), "{text}");
}

#[test]
fn generics_and_any_visibility_are_followed() {
    for service in [
        "pub type Review<T> = crate::f::store::Page<T>;\n",
        "pub(crate) type Review<T> = crate::f::store::Page<T>;\n",
        "type Review<T> = crate::f::store::Page<T>;\n",
        "pub type Review = crate::f::store::Review;\n",
    ] {
        let text = rust(service, "use crate::f::service::Review;\n");
        assert!(
            text.contains(&imports("f-controller", "f-store")),
            "{service}\n{text}"
        );
    }
}

#[test]
fn a_chain_of_aliases_is_followed_to_the_end() {
    let contract =
        format!("{RUST_CONTRACT}  page\n  page --> f-controller\n%% map page: src/f/page.rs\n");
    let files = [
        (
            "Cargo.toml",
            "[package]\nname = \"demo\"\nversion = \"0.0.0\"\n",
        ),
        ("src/lib.rs", "mod f;\n"),
        (
            "src/f/mod.rs",
            "mod controller;\nmod page;\nmod service;\nmod store;\n",
        ),
        ("src/f/store.rs", "pub struct Review;\n"),
        (
            "src/f/service.rs",
            "pub type Step = crate::f::store::Review;\n",
        ),
        (
            "src/f/controller.rs",
            "pub type Last = crate::f::service::Step;\n",
        ),
        ("src/f/page.rs", "use crate::f::controller::Last;\n"),
    ];
    let text = stdout(&lint(&contract, &files));
    assert!(text.contains(&imports("page", "f-store")), "{text}");
    assert!(
        !text.contains(&imports("f-controller", "f-store")),
        "{text}"
    );
}

#[test]
fn an_alias_to_an_external_crate_or_a_local_item_is_not_followed() {
    for service in [
        "pub type Review = std::collections::BTreeMap<String, String>;\n",
        "pub struct Inner;\npub type Review = Inner;\n",
    ] {
        let text = rust(service, "use crate::f::service::Review;\n");
        assert!(
            !text.contains("error[undeclared-edge]"),
            "{service}\n{text}"
        );
    }
}

#[test]
fn a_facade_file_stops_the_walk_for_its_callers() {
    let contract = format!("{RUST_CONTRACT}%% facade f-service\n");
    let files = [
        (
            "Cargo.toml",
            "[package]\nname = \"demo\"\nversion = \"0.0.0\"\n",
        ),
        ("src/lib.rs", "mod f;\n"),
        (
            "src/f/mod.rs",
            "mod controller;\nmod service;\nmod store;\n",
        ),
        ("src/f/store.rs", "pub struct Review;\n"),
        (
            "src/f/service.rs",
            "pub type Review = crate::f::store::Review;\n",
        ),
        ("src/f/controller.rs", "use crate::f::service::Review;\n"),
    ];
    let text = stdout(&lint(&contract, &files));
    assert!(!text.contains("error[undeclared-edge]"), "{text}");
}

/// The TypeScript variant of the repro: `controller` reaches `store` through `service`.
fn typescript(service: &str) -> String {
    let contract = "flowchart LR
  controller
  service
  store
  controller --> service
  service --> store
%% map controller: controller.ts
%% map service: service.ts
%% map store: store.ts
";
    let files = [
        ("store.ts", "export interface Review {}\n"),
        ("service.ts", service),
        (
            "controller.ts",
            "import type { Review } from './service';\nexport const r: Review | undefined = undefined;\n",
        ),
    ];
    stdout(&lint(contract, &files))
}

#[test]
fn a_typescript_alias_of_an_imported_type_is_followed() {
    for service in [
        "import type { Review as R } from './store';\nexport type Review = R;\n",
        "import type { Review as Inner } from './store';\nexport type Review<T = 1> = Inner<T>;\n",
        "export type Review = import('./store').Review;\n",
    ] {
        let text = typescript(service);
        assert!(
            text.contains(&imports("controller", "store")),
            "{service}\n{text}"
        );
    }
}

#[test]
fn a_typescript_alias_that_builds_a_new_type_is_not_followed() {
    for service in [
        "import type { Review as Inner } from './store';\nexport type Review = Inner | null;\n",
        "import type { Review as Inner } from './store';\nexport type Review = Inner & { id: string };\n",
        "import type { Review as Inner } from './store';\nexport type Review = { inner: Inner };\n",
        "import type { Review as Inner } from './store';\nexport type Review = { [K in keyof Inner]: string };\n",
        "import type { Review as Inner } from './store';\nexport type Review = Inner extends object ? Inner : never;\n",
    ] {
        let text = typescript(service);
        assert!(
            !text.contains("error[undeclared-edge]"),
            "{service}\n{text}"
        );
    }
}
