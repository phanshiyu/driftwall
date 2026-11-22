//! Guard: no library error repeats its cause in its own message.
//!
//! The CLI prints an error and then every `source()` below it. An error whose message already
//! contains its source's message would therefore print the cause twice. Each error words only its
//! own context and leaves the cause to `source()`. This file builds one value of every variant of
//! every library error enum and checks the whole chain.
//!
//! Adding a variant to a listed enum fails to compile until it is named in its `witness!` list
//! (the `match` behind it has no wildcard), and then fails the coverage test until `samples` builds
//! it. Adding a new error type fails `every_error_type_is_covered`.

#[cfg(test)]
mod witness {
    use std::collections::BTreeSet;
    use std::error::Error;
    use std::fs;
    use std::io;
    use std::path::{Path, PathBuf};

    use driftwall::diagram::DiagramError;
    use driftwall::imports::ImportsError;
    use driftwall::init::InitError;
    use driftwall::lint::LintError;
    use driftwall::model::{Location, ModelError, NodeId};
    use driftwall::ownership::{ArchProblem, OwnershipError};
    use driftwall::report::ReportError;

    /// An error enum with one sample per variant and a variant-name list the compiler keeps whole.
    trait Witness: Error + Build + 'static {
        /// The enum's own name, as written in the source.
        const TYPE: &'static str;
        /// Every variant name, generated from the same list as the exhaustive `match`.
        const NAMES: &'static [&'static str];
        /// The variant name of `self`. An exhaustive `match`, so a new variant breaks the build.
        fn name(&self) -> &'static str;
    }

    /// At least one value of every variant, wrapping real causes where a variant has one.
    trait Build: Sized {
        fn build() -> Vec<Self>;
    }

    macro_rules! witness {
        ($ty:ident: $($variant:ident),+ $(,)?) => {
            impl Witness for $ty {
                const TYPE: &'static str = stringify!($ty);
                const NAMES: &'static [&'static str] = &[$(stringify!($variant)),+];
                fn name(&self) -> &'static str {
                    match self {
                        $(Self::$variant { .. } => stringify!($variant),)+
                    }
                }
            }
        };
    }

    witness!(ModelError: InvalidNodeId, InvalidPrefix);
    witness!(
        ArchProblem: MissingNode,
        DuplicateNode,
        InvalidId,
        DuplicateScope,
        InvalidScope,
        UnrecognisedLine,
    );
    witness!(
        DiagramError: MissingHeader,
        BadHeader,
        MisplacedHeader,
        Unsupported,
        InvalidId,
        ReservedWord,
        UndeclaredNode,
        DuplicateNode,
        DuplicateEdge,
        SelfEdge,
        DuplicateBoundary,
        NestedSubgraph,
        UnclosedSubgraph,
        StrayEnd,
        MalformedExports,
        DuplicateExports,
        UndeclaredExportsNode,
    );
    witness!(OwnershipError: Io, InvalidArch, InvalidModule, Walk, OutsideRoot);
    witness!(ImportsError: Io, Grammar, Query);
    witness!(LintError: Io, Diagram, Ownership, Imports, OldContract, NewContract);
    witness!(
        InitError: Io,
        Ownership,
        Imports,
        Conflict,
        NoSources,
        Diagram,
        RoundTrip,
    );
    witness!(ReportError: Serialize);

    /// Every error type the guard covers, by name. `every_error_type_is_covered` compares it with
    /// the source tree.
    const COVERED: &[&str] = &[
        ModelError::TYPE,
        ArchProblem::TYPE,
        DiagramError::TYPE,
        OwnershipError::TYPE,
        ImportsError::TYPE,
        LintError::TYPE,
        InitError::TYPE,
        ReportError::TYPE,
    ];

    fn io_error() -> io::Error {
        io::Error::other("the cause")
    }

    fn path() -> PathBuf {
        PathBuf::from("some/path")
    }

    fn id() -> NodeId {
        NodeId::new("a").unwrap()
    }

    fn model_error() -> ModelError {
        ModelError::InvalidNodeId("1".to_owned())
    }

    fn ownership_error() -> OwnershipError {
        OwnershipError::Io {
            path: path(),
            source: io_error(),
        }
    }

    fn imports_error() -> ImportsError {
        ImportsError::Io {
            path: path(),
            source: io_error(),
        }
    }

    fn diagram_error() -> DiagramError {
        DiagramError::InvalidId {
            line: 4,
            source: model_error(),
        }
    }

    impl Build for ModelError {
        fn build() -> Vec<Self> {
            vec![model_error(), Self::InvalidPrefix("1".to_owned())]
        }
    }

    impl Build for ArchProblem {
        fn build() -> Vec<Self> {
            vec![
                Self::MissingNode,
                Self::DuplicateNode,
                Self::InvalidId(model_error()),
                Self::DuplicateScope,
                Self::InvalidScope("x".to_owned()),
                Self::UnrecognisedLine("x".to_owned()),
            ]
        }
    }

    impl Build for DiagramError {
        fn build() -> Vec<Self> {
            vec![
                Self::MissingHeader,
                Self::BadHeader {
                    line: 1,
                    found: "x".to_owned(),
                },
                Self::MisplacedHeader { line: 2 },
                Self::Unsupported {
                    line: 3,
                    statement: "x".to_owned(),
                },
                diagram_error(),
                Self::ReservedWord {
                    line: 5,
                    word: "end".to_owned(),
                },
                Self::UndeclaredNode { line: 6, id: id() },
                Self::DuplicateNode {
                    line: 7,
                    id: id(),
                    first_line: 2,
                },
                Self::DuplicateEdge {
                    line: 8,
                    from: id(),
                    to: id(),
                    first_line: 2,
                },
                Self::SelfEdge { line: 9, id: id() },
                Self::DuplicateBoundary {
                    line: 10,
                    id: "x".to_owned(),
                    first_line: 2,
                },
                Self::NestedSubgraph {
                    line: 11,
                    open: "x".to_owned(),
                    open_line: 2,
                },
                Self::UnclosedSubgraph {
                    line: 12,
                    id: "x".to_owned(),
                },
                Self::StrayEnd { line: 13 },
                Self::MalformedExports {
                    line: 14,
                    reason: "x".to_owned(),
                },
                Self::DuplicateExports {
                    line: 15,
                    id: id(),
                    first_line: 2,
                },
                Self::UndeclaredExportsNode { line: 16, id: id() },
            ]
        }
    }

    impl Build for OwnershipError {
        fn build() -> Vec<Self> {
            vec![
                ownership_error(),
                Self::InvalidArch {
                    at: Location {
                        file: path(),
                        line: 2,
                    },
                    problem: ArchProblem::MissingNode,
                },
                Self::InvalidModule {
                    at: Location {
                        file: path(),
                        line: 2,
                    },
                    file: path(),
                    source: model_error(),
                },
                Self::Walk {
                    root: path(),
                    source: ignore::Error::Io(io_error()),
                },
                Self::OutsideRoot {
                    path: path(),
                    root: path(),
                },
            ]
        }
    }

    impl Build for ImportsError {
        fn build() -> Vec<Self> {
            vec![
                imports_error(),
                Self::Grammar(tree_sitter::LanguageError::NotParseable),
                Self::Query(tree_sitter::QueryError {
                    row: 0,
                    column: 0,
                    offset: 0,
                    message: "the cause".to_owned(),
                    kind: tree_sitter::QueryErrorKind::Syntax,
                }),
            ]
        }
    }

    impl Build for LintError {
        fn build() -> Vec<Self> {
            vec![
                Self::Io {
                    path: path(),
                    source: io_error(),
                },
                Self::Diagram {
                    path: path(),
                    source: diagram_error(),
                },
                Self::Ownership(ownership_error()),
                Self::Imports(imports_error()),
                Self::OldContract {
                    source: diagram_error(),
                },
                Self::NewContract {
                    source: diagram_error(),
                },
            ]
        }
    }

    impl Build for InitError {
        fn build() -> Vec<Self> {
            vec![
                Self::Io {
                    path: path(),
                    source: io_error(),
                },
                Self::Ownership(ownership_error()),
                Self::Imports(imports_error()),
                Self::Conflict {
                    paths: vec![path()],
                },
                Self::NoSources,
                Self::Diagram(diagram_error()),
                Self::RoundTrip,
            ]
        }
    }

    impl Build for ReportError {
        fn build() -> Vec<Self> {
            vec![Self::Serialize(
                serde_json::from_str::<u8>("not json").unwrap_err(),
            )]
        }
    }

    /// Panics naming the first error in the chain whose message contains its source's message.
    fn assert_no_repeated_cause(error: &(dyn Error + 'static)) {
        let mut current = error;
        while let Some(source) = current.source() {
            let (outer, inner) = (current.to_string(), source.to_string());
            assert!(
                !outer.contains(&inner),
                "`{outer}` repeats its source `{inner}`: leave the cause to source()"
            );
            current = source;
        }
    }

    fn check<E: Witness>() {
        let samples = E::build();
        let built: BTreeSet<&str> = samples.iter().map(Witness::name).collect();
        let all: BTreeSet<&str> = E::NAMES.iter().copied().collect();
        assert_eq!(built, all, "{}: build a sample of every variant", E::TYPE);
        for sample in &samples {
            assert_no_repeated_cause(sample);
        }
    }

    #[test]
    fn model_error_does_not_repeat_its_cause() {
        check::<ModelError>();
    }

    #[test]
    fn arch_problem_does_not_repeat_its_cause() {
        check::<ArchProblem>();
    }

    #[test]
    fn diagram_error_does_not_repeat_its_cause() {
        check::<DiagramError>();
    }

    #[test]
    fn ownership_error_does_not_repeat_its_cause() {
        check::<OwnershipError>();
    }

    #[test]
    fn imports_error_does_not_repeat_its_cause() {
        check::<ImportsError>();
    }

    #[test]
    fn lint_error_does_not_repeat_its_cause() {
        check::<LintError>();
    }

    #[test]
    fn init_error_does_not_repeat_its_cause() {
        check::<InitError>();
    }

    #[test]
    fn report_error_does_not_repeat_its_cause() {
        check::<ReportError>();
    }

    #[test]
    #[should_panic(expected = "repeats its source")]
    fn the_check_catches_a_repeated_cause() {
        #[derive(Debug, thiserror::Error)]
        #[error("cannot read x: {source}")]
        struct Repeats {
            source: io::Error,
        }
        let error = Repeats { source: io_error() };
        assert_no_repeated_cause(&error);
    }

    fn rust_files(dir: &Path, out: &mut Vec<PathBuf>) {
        let Ok(entries) = fs::read_dir(dir) else {
            return;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                rust_files(&path, out);
            } else if path.extension().is_some_and(|ext| ext == "rs") {
                out.push(path);
            }
        }
    }

    fn words(line: &str) -> impl Iterator<Item = &str> {
        line.split(|c: char| !c.is_alphanumeric() && c != '_')
            .filter(|word| !word.is_empty())
    }

    /// Names of the types in `text` that derive `Error` or implement `Error` by hand.
    fn error_types(text: &str) -> BTreeSet<String> {
        let mut found = BTreeSet::new();
        let mut derives_error = false;
        for line in text.lines().map(str::trim_start) {
            if line.starts_with("#[derive(") && words(line).any(|word| word == "Error") {
                derives_error = true;
            }
            let mut tokens = words(line).skip_while(|word| !matches!(*word, "enum" | "struct"));
            if let (true, Some(_), Some(name)) = (derives_error, tokens.next(), tokens.next()) {
                found.insert(name.to_owned());
                derives_error = false;
            }
            if line.starts_with("impl") {
                let tokens: Vec<&str> = words(line).collect();
                for window in tokens.windows(3) {
                    if let ["Error", "for", name] = window {
                        found.insert((*name).to_owned());
                    }
                }
            }
        }
        found
    }

    #[test]
    fn every_error_type_is_covered() {
        let mut files = Vec::new();
        rust_files(
            &Path::new(env!("CARGO_MANIFEST_DIR")).join("src"),
            &mut files,
        );
        let mut found = BTreeSet::new();
        for file in files {
            found.extend(error_types(&fs::read_to_string(file).unwrap()));
        }
        let covered: BTreeSet<String> = COVERED.iter().map(|name| (*name).to_owned()).collect();
        assert_eq!(
            found, covered,
            "an error type has no witness! entry in tests/error_sources.rs"
        );
    }
}
