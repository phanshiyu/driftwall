//! Type aliases followed like re-exports, see "Type aliases" in "Re-exports" in `docs/DESIGN.md`.

use super::reexport::{site, sites};

/// A crate with a `service` module that aliases a type its `store` defines, and an `app` using it.
fn got(service: &str, app: &str) -> Vec<(u32, String, String)> {
    let files = [
        ("src/lib.rs", "mod service;\nmod store;\nmod app;\n"),
        ("src/service.rs", service),
        (
            "src/store.rs",
            "pub struct Review;\npub struct Page<T>(T);\n",
        ),
        ("src/app.rs", app),
    ];
    sites(&files, "src/app.rs")
}

#[test]
fn rust_alias_is_followed_to_the_defining_file() {
    let found = got(
        "pub type Review = crate::store::Review;\n",
        "use crate::service::Review;\n",
    );
    assert_eq!(found, [site(1, "src/store.rs", "Review")]);
}

#[test]
fn rust_alias_of_any_visibility_is_followed() {
    for visibility in ["", "pub(crate) ", "pub(super) "] {
        let service = format!("{visibility}type Review = crate::store::Review;\n");
        let found = got(&service, "use crate::service::Review;\n");
        assert_eq!(found, [site(1, "src/store.rs", "Review")], "{visibility}");
    }
}

#[test]
fn rust_alias_with_generics_is_followed() {
    let found = got(
        "pub type Reviews<T> = crate::store::Page<T>;\n",
        "use crate::service::Reviews;\n",
    );
    assert_eq!(found, [site(1, "src/store.rs", "Page")]);
}

#[test]
fn rust_alias_keeps_its_own_edge_and_its_generic_arguments() {
    let files = [
        ("src/lib.rs", "mod service;\nmod store;\n"),
        (
            "src/service.rs",
            "pub type Reviews = crate::store::Page<crate::store::Review>;\n",
        ),
        (
            "src/store.rs",
            "pub struct Review;\npub struct Page<T>(T);\n",
        ),
    ];
    assert_eq!(
        sites(&files, "src/service.rs"),
        [site(1, "src/store.rs", "Page,Review")]
    );
}

#[test]
fn rust_alias_through_an_imported_name_or_module_is_followed() {
    let by_item = got(
        "use crate::store::Review;\npub type Mine = Review;\n",
        "use crate::service::Mine;\n",
    );
    assert_eq!(by_item, [site(1, "src/store.rs", "Review")]);
    let by_module = got(
        "use crate::store;\ntype Mine = store::Review;\n",
        "use crate::service::Mine;\n",
    );
    assert_eq!(by_module, [site(1, "src/store.rs", "Review")]);
}

#[test]
fn rust_chain_of_aliases_is_followed_to_the_end() {
    let files = [
        ("src/lib.rs", "mod a;\nmod b;\nmod store;\nmod app;\n"),
        ("src/a.rs", "pub type One = crate::b::Two;\n"),
        ("src/b.rs", "pub type Two = crate::store::Review;\n"),
        ("src/store.rs", "pub struct Review;\n"),
        ("src/app.rs", "use crate::a::One;\n"),
    ];
    assert_eq!(
        sites(&files, "src/app.rs"),
        [site(1, "src/store.rs", "Review")]
    );
}

#[test]
fn rust_alias_mixed_with_a_pub_use_chain_is_followed() {
    let files = [
        ("src/lib.rs", "mod a;\nmod b;\nmod store;\nmod app;\n"),
        ("src/a.rs", "pub use crate::b::Two;\n"),
        ("src/b.rs", "pub type Two = crate::store::Review;\n"),
        ("src/store.rs", "pub struct Review;\n"),
        ("src/app.rs", "use crate::a::Two;\n"),
    ];
    assert_eq!(
        sites(&files, "src/app.rs"),
        [site(1, "src/store.rs", "Review")]
    );
}

#[test]
fn rust_alias_to_std_or_a_local_item_stays_defined_here() {
    for service in [
        "pub type Review = std::collections::BTreeMap<String, String>;\n",
        "struct Inner;\npub type Review = Inner;\n",
        "struct Inner;\npub type Review = self::Inner;\n",
    ] {
        let found = got(service, "use crate::service::Review;\n");
        assert_eq!(found, [site(1, "src/service.rs", "Review")], "{service}");
    }
}

#[test]
fn rust_alias_that_is_not_a_bare_path_defines_a_new_type() {
    for body in [
        "&'static crate::store::Review",
        "(crate::store::Review, u8)",
        "Box<dyn Fn()>",
        "[crate::store::Review; 2]",
    ] {
        let found = got(
            &format!("pub type Review = {body};\n"),
            "use crate::service::Review;\n",
        );
        assert_eq!(found, [site(1, "src/service.rs", "Review")], "{body}");
    }
}

#[test]
fn rust_alias_inside_a_module_block_is_not_read() {
    let found = got(
        "mod inner {\n    pub type Review = crate::store::Review;\n}\n",
        "use crate::service::Review;\n",
    );
    assert_eq!(found, [site(1, "src/service.rs", "Review")]);
}

/// A TypeScript pair: `service.ts` with `service`, `store.ts` defining `Review`, `app.ts` with `app`.
fn ts(service: &str, app: &str) -> Vec<(u32, String, String)> {
    let files = [
        (
            "store.ts",
            "export interface Review {}\nexport interface Page<T> {}\n",
        ),
        ("service.ts", service),
        ("app.ts", app),
    ];
    sites(&files, "app.ts")
}

#[test]
fn ts_alias_of_an_imported_type_is_followed() {
    for service in [
        "import type { Review } from './store';\nexport type Mine = Review;\n",
        "import { Review } from './store';\ntype Mine = Review;\nexport { Mine };\n",
        "import type { Review as R } from './store';\nexport type Mine = R;\n",
        "import type { Review } from './store';\nexport type Mine<T> = Review<T>;\n",
        "export type Mine = import('./store').Review;\n",
    ] {
        let found = ts(service, "import type { Mine } from './service';\n");
        assert_eq!(found, [site(1, "store.ts", "Review")], "{service}");
    }
}

#[test]
fn ts_chain_of_aliases_is_followed_to_the_end() {
    let files = [
        ("store.ts", "export interface Review {}\n"),
        (
            "b.ts",
            "import type { Review } from './store';\nexport type Two = Review;\n",
        ),
        (
            "a.ts",
            "import type { Two } from './b';\nexport type One = Two;\n",
        ),
        ("app.ts", "import type { One } from './a';\n"),
    ];
    assert_eq!(sites(&files, "app.ts"), [site(1, "store.ts", "Review")]);
}

#[test]
fn ts_alias_that_builds_a_new_type_is_not_followed() {
    for body in [
        "Review | null",
        "Review & { extra: string }",
        "{ review: Review }",
        "Review[]",
        "keyof Review",
        "{ [K in keyof Review]: string }",
        "Review extends object ? Review : never",
    ] {
        let service =
            format!("import type {{ Review }} from './store';\nexport type Mine = {body};\n");
        let found = ts(&service, "import type { Mine } from './service';\n");
        assert_eq!(found, [site(1, "service.ts", "Mine")], "{body}");
    }
}

#[test]
fn ts_alias_of_a_local_or_unimported_type_is_not_followed() {
    for service in [
        "interface Local {}\nexport type Mine = Local;\n",
        "export type Mine = string;\n",
        "import * as store from './store';\nexport type Mine = store.Review;\n",
        "import type { Review } from 'some-package';\nexport type Mine = Review;\n",
    ] {
        let found = ts(service, "import type { Mine } from './service';\n");
        assert_eq!(found, [site(1, "service.ts", "Mine")], "{service}");
    }
}
