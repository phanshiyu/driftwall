//! Shell: the files a script loads with `source` and `.`.

use super::fixtures::{pairs, repo};
use super::*;
use crate::model::{ImportKind, SkipReason};

/// The resolved targets of `scripts/run.sh` holding `text`, next to the library files every test
/// shares, as (line, target).
fn run(text: &str) -> Vec<(u32, String)> {
    repo(&shared(text)).edges("scripts/run.sh")
}

fn shared(text: &str) -> Vec<(&str, &str)> {
    vec![
        ("scripts/run.sh", text),
        ("scripts/helper.sh", ""),
        ("scripts/sub/deep.sh", ""),
        ("device/lib/net.sh", ""),
        ("device/lib/disk.sh", ""),
        ("device/lib/notes.txt", ""),
        ("top.sh", ""),
        ("pkg/pkg.sh", ""),
        ("settings.env", ""),
    ]
}

fn edge(line: u32, target: &str) -> (u32, String) {
    (line, target.to_owned())
}

#[test]
fn a_literal_path_is_tried_beside_the_script_then_at_the_repository_root() {
    assert_eq!(
        run(
            "source helper.sh\n. ./sub/deep.sh\nsource 'helper.sh'\nsource \"device/lib/net.sh\"\nsource top.sh\n"
        ),
        [
            edge(1, "scripts/helper.sh"),
            edge(2, "scripts/sub/deep.sh"),
            edge(3, "scripts/helper.sh"),
            edge(4, "device/lib/net.sh"),
            edge(5, "top.sh"),
        ]
    );
}

#[test]
fn the_script_directory_wins_over_the_repository_root() {
    let r = repo(&[
        ("scripts/run.sh", "source common.sh\n"),
        ("scripts/common.sh", ""),
        ("common.sh", ""),
    ]);
    assert_eq!(r.edges("scripts/run.sh"), [edge(1, "scripts/common.sh")]);
}

#[test]
fn a_literal_path_outside_the_repository_is_ignored() {
    let r = repo(&[(
        "scripts/run.sh",
        "source /etc/profile\nsource ~/.bashrc\n. ../../../../elsewhere.sh\n",
    )]);
    assert_eq!(r.imports("scripts/run.sh"), FileImports::default());
}

#[test]
fn a_literal_path_that_finds_nothing_is_unresolved_unless_it_names_an_asset() {
    let r = repo(&shared(
        "source missing.sh\n. ./settings.env\nsource ../settings.env\nsource device/lib/notes.txt\n",
    ));
    assert_eq!(r.unresolved("scripts/run.sh"), pairs(&[(1, "missing.sh")]));
    assert_eq!(r.edges("scripts/run.sh"), Vec::<(u32, String)>::new());
    assert_eq!(r.skipped("scripts/run.sh"), pairs(&[]));
}

#[test]
fn the_directory_of_the_script_is_written_in_every_supported_form() {
    for form in [
        "\"$(dirname \"$0\")/helper.sh\"",
        "\"$(dirname \"${BASH_SOURCE[0]}\")/helper.sh\"",
        "\"$(dirname \"$BASH_SOURCE\")/helper.sh\"",
        "\"$(dirname \"${0}\")/helper.sh\"",
        "$(dirname $0)/helper.sh",
        "\"$(dirname -- \"$0\")/helper.sh\"",
        "\"`dirname \"$0\"`/helper.sh\"",
        "\"${BASH_SOURCE%/*}/helper.sh\"",
        "\"${BASH_SOURCE[0]%/*}/helper.sh\"",
        "\"${0%/*}/helper.sh\"",
        "${0%/*}/helper.sh",
        "\"$(dirname \"$0\")\"/helper.sh",
        "\"$(cd \"$(dirname \"$0\")\" && pwd)/helper.sh\"",
        "\"$(cd \"$(dirname \"$0\")\" && pwd -P)/helper.sh\"",
        "\"$(cd -- \"$(dirname \"$0\")\" && pwd -L)/helper.sh\"",
        "\"$(dirname \"$0\")/sub/../helper.sh\"",
    ] {
        let found = repo(&shared(&format!("source {form}\n"))).imports("scripts/run.sh");
        assert_eq!(
            found.resolved.len(),
            1,
            "{form}: {:?} {:?}",
            found.skipped,
            found.unresolved
        );
        assert_eq!(
            found.resolved[0].target.display().to_string(),
            "scripts/helper.sh",
            "{form}"
        );
    }
}

#[test]
fn dots_after_the_script_directory_climb_lexically() {
    assert_eq!(
        run("source \"$(dirname \"$0\")/../device/lib/net.sh\"\n. \"${0%/*}/../top.sh\"\n"),
        [edge(1, "device/lib/net.sh"), edge(2, "top.sh")]
    );
}

#[test]
fn a_root_variable_assigned_from_the_script_directory_anchors_later_paths() {
    let text = "ROOT=\"$(cd \"$(dirname \"$0\")/..\" && pwd)\"\n\
. \"$ROOT/device/lib/net.sh\"\n\
source \"${ROOT}/top.sh\"\n\
source \"$ROOT\"/device/lib/disk.sh\n\
. $ROOT/pkg/pkg.sh\n";
    assert_eq!(
        run(text),
        [
            edge(2, "device/lib/net.sh"),
            edge(3, "top.sh"),
            edge(4, "device/lib/disk.sh"),
            edge(5, "pkg/pkg.sh"),
        ]
    );
}

#[test]
fn a_variable_may_be_built_from_another_and_exported_or_local() {
    let text = "HERE=\"${BASH_SOURCE%/*}\"\n\
export ROOT=\"$(cd \"$HERE/..\" && pwd)\"\n\
LIB=$ROOT/device/lib\n\
f() {\n\
  local D=\"$(dirname \"$0\")\"\n\
  source \"$D/helper.sh\"\n\
  source \"$LIB/net.sh\"\n\
}\n\
readonly UP=\"$(dirname \"$0\")/..\"\n\
source \"$UP/top.sh\"\n";
    assert_eq!(
        run(text),
        [
            edge(6, "scripts/helper.sh"),
            edge(7, "device/lib/net.sh"),
            edge(10, "top.sh"),
        ]
    );
}

#[test]
fn an_unquoted_command_substitution_value_is_read_like_a_quoted_one() {
    let text = "HERE=$(cd \"$(dirname \"${BASH_SOURCE[0]}\")\" && pwd)\n\
. \"$HERE/helper.sh\"\n\
TOP=$(dirname \"$0\")/..\n\
. $TOP/top.sh\n";
    assert_eq!(run(text), [edge(2, "scripts/helper.sh"), edge(4, "top.sh")]);
}

#[test]
fn the_nearest_earlier_assignment_decides_and_never_an_older_one() {
    let text = "ROOT=\"$(dirname \"$0\")/..\"\n\
source \"$ROOT/top.sh\"\n\
ROOT=\"$(dirname \"$0\")\"\n\
source \"$ROOT/helper.sh\"\n\
ROOT=$(git rev-parse --show-toplevel)\n\
source \"$ROOT/top.sh\"\n\
ROOT=\"$(dirname \"$0\")\"\n";
    let r = repo(&shared(text));
    assert_eq!(
        r.edges("scripts/run.sh"),
        [edge(2, "top.sh"), edge(4, "scripts/helper.sh")]
    );
    assert_eq!(
        r.skipped("scripts/run.sh"),
        pairs(&[(6, "\"$ROOT/top.sh\"")])
    );
}

#[test]
fn a_variable_that_is_not_assigned_from_the_script_directory_is_dynamic() {
    for text in [
        "source \"$ROOT/top.sh\"\n",
        "ROOT=.\nsource \"$ROOT/top.sh\"\n",
        "ROOT=\"$HOME/pm\"\nsource \"$ROOT/top.sh\"\n",
        "ROOT=\"$(pwd)\"\nsource \"$ROOT/top.sh\"\n",
        "source \"$HOME/top.sh\"\n",
        "for f in a b; do source \"$f\"; done\n",
        "source \"$(dirname \"$0\")/$name.sh\"\n",
        "source \"$(dirname \"$1\")/helper.sh\"\n",
        "source \"$(dirname \"$0\" x)/helper.sh\"\n",
        "source \"$(cd \"$(dirname \"$0\")\" || pwd)/helper.sh\"\n",
        "source \"$(realpath \"$0\")\"\n",
        "source \"${x:-top.sh}\"\n",
        "source \"$(git rev-parse --show-toplevel)/top.sh\"\n",
        "source \"$(dirname \"$0\")\"top.sh\n",
        "source \"$((1 + 1)).sh\"\n",
        "source top\\ x.sh\n",
        "source \"${0%%/*}/helper.sh\"\n",
        "source \"${0#*/}/helper.sh\"\n",
        "source \"${BASH_SOURCE[1]%/*}/helper.sh\"\n",
    ] {
        let r = repo(&shared(text));
        let imports = r.imports("scripts/run.sh");
        assert_eq!(imports.resolved, [], "{text}");
        assert_eq!(imports.unresolved, [], "{text}");
        assert_eq!(imports.skipped.len(), 1, "{text}");
        assert_eq!(
            imports.skipped[0].reason,
            SkipReason::DynamicSpecifier,
            "{text}"
        );
        assert_eq!(
            imports.skipped[0].at.line,
            u32::try_from(text.lines().count()).unwrap(),
            "{text}"
        );
    }
}

#[test]
fn a_skipped_path_is_reported_as_written() {
    let r = repo(&shared("\n\nsource \"$HOME/.profile\"\n. $f\n"));
    assert_eq!(
        r.skipped("scripts/run.sh"),
        pairs(&[(3, "\"$HOME/.profile\""), (4, "$f")])
    );
}

#[test]
fn a_glob_expands_to_every_matching_known_file_in_path_order() {
    let text = "ROOT=\"$(cd \"$(dirname \"$0\")/..\" && pwd)\"\n\
. \"$ROOT/device/lib/*.sh\"\n\
source scripts/hel?er.sh\n\
source sub/*.sh\n";
    assert_eq!(
        run(text),
        [
            edge(2, "device/lib/disk.sh"),
            edge(2, "device/lib/net.sh"),
            edge(3, "scripts/helper.sh"),
            edge(4, "scripts/sub/deep.sh"),
        ]
    );
}

#[test]
fn a_glob_stays_inside_one_path_segment_and_never_matches_the_script_itself() {
    let r = repo(&[
        (
            "scripts/run.sh",
            "source ./*.sh\nsource *\nsource ../*/helper.sh\n",
        ),
        ("scripts/helper.sh", ""),
        ("scripts/sub/deep.sh", ""),
    ]);
    assert_eq!(
        r.edges("scripts/run.sh"),
        [
            edge(1, "scripts/helper.sh"),
            edge(2, "scripts/helper.sh"),
            edge(3, "scripts/helper.sh"),
        ]
    );
}

#[test]
fn a_glob_that_matches_nothing_is_unresolved_and_other_pattern_syntax_is_dynamic() {
    let r = repo(&shared(
        "source lib/*.zsh\nsource device/**/net.sh\nsource device/lib/[nd]*.sh\nsource device/lib/{net,disk}.sh\n",
    ));
    assert_eq!(r.unresolved("scripts/run.sh"), pairs(&[(1, "lib/*.zsh")]));
    assert_eq!(
        r.skipped("scripts/run.sh"),
        pairs(&[
            (2, "device/**/net.sh"),
            (3, "device/lib/[nd]*.sh"),
            (4, "device/lib/{net,disk}.sh"),
        ])
    );
}

#[test]
fn only_source_and_dot_commands_are_imports() {
    let text = "./helper.sh\n\
bash helper.sh\n\
sh helper.sh\n\
echo source helper.sh\n\
# source helper.sh\n\
sourced helper.sh\n\
.helper.sh\n\
source\n\
x=\"source helper.sh\"\n\
cat <<EOF\n\
source helper.sh\n\
EOF\n\
command source helper.sh\n\
. helper.sh extra arguments\n";
    assert_eq!(run(text), [edge(14, "scripts/helper.sh")]);
}

#[test]
fn commands_are_found_in_functions_conditions_and_loops() {
    let text = "if [ -f x ]; then\n  . helper.sh\nfi\n\
while true; do source ./sub/deep.sh; done\n\
f() { source ../top.sh; }\n\
[ -r helper.sh ] && . helper.sh\n";
    assert_eq!(
        run(text),
        [
            edge(2, "scripts/helper.sh"),
            edge(4, "scripts/sub/deep.sh"),
            edge(5, "top.sh"),
            edge(6, "scripts/helper.sh"),
        ]
    );
}

#[test]
fn a_path_that_leaves_the_repository_is_ignored() {
    let r = repo(&[(
        "scripts/run.sh",
        "source \"$(dirname \"$0\")/../../x.sh\"\nsource \"${0%/*}/../../../y.sh\"\n",
    )]);
    assert_eq!(r.imports("scripts/run.sh"), FileImports::default());
}

#[test]
fn a_sourced_file_is_a_runtime_import_that_uses_no_symbol() {
    let r = repo(&shared("source helper.sh\n"));
    let imports = r.imports("scripts/run.sh");
    assert_eq!(imports.resolved.len(), 1);
    let import = &imports.resolved[0];
    assert_eq!(import.kind, ImportKind::Runtime);
    assert!(import.symbols.is_empty());
    assert!(!import.test_only);
}

#[test]
fn test_scripts_are_marked_test_only() {
    for (file, expected) in [
        ("scripts/run.sh", false),
        ("scripts/test_run.sh", true),
        ("scripts/run_test.sh", true),
        ("scripts/test/run.sh", true),
        ("tests/x/run.sh", true),
        ("scripts/testing.sh", false),
        ("scripts/contest_run.sh", false),
    ] {
        let r = repo(&[(file, "source top.sh\n"), ("top.sh", "")]);
        let imports = r.imports(file);
        assert_eq!(imports.resolved.len(), 1, "{file}");
        assert_eq!(imports.resolved[0].test_only, expected, "{file}");
    }
}

#[test]
fn bash_and_zsh_files_are_read_like_sh_files() {
    let r = repo(&[
        ("a/x.bash", "source ./y.zsh\n"),
        ("a/y.zsh", ". ./z.sh\n"),
        ("a/z.sh", ""),
    ]);
    assert_eq!(r.edges("a/x.bash"), [edge(1, "a/y.zsh")]);
    assert_eq!(r.edges("a/y.zsh"), [edge(1, "a/z.sh")]);
}
