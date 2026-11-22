//! Parses and renders the architecture contract (a strict Mermaid flowchart subset).
//!
//! The accepted grammar is specified in `docs/DESIGN.md` under "Contract format". Anything
//! outside it is a hard [`DiagramError`] carrying a line number.

mod error;
mod parse;
mod render;

pub use error::DiagramError;
pub use parse::parse;
pub use render::render;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Contract, NodeId};

    const OWN_CONTRACT: &str = include_str!("../../architecture.mmd");

    fn boundary_of(contract: &Contract, node: &str) -> Option<String> {
        contract.nodes[&NodeId::new(node).unwrap()].boundary.clone()
    }

    #[test]
    fn repository_contract_parses() {
        let contract = parse(OWN_CONTRACT).unwrap();
        assert_eq!(boundary_of(&contract, "model").as_deref(), Some("shared"));
        assert!(!contract.edges.is_empty());
    }

    #[test]
    fn repository_contract_survives_a_render_round_trip() {
        let contract = parse(OWN_CONTRACT).unwrap();
        let rendered = render(&contract);
        assert_eq!(parse(&rendered).as_ref(), Ok(&contract));
        assert_eq!(render(&contract), rendered);
    }

    #[test]
    fn a_contract_with_exports_survives_a_render_round_trip() {
        let src = format!("{OWN_CONTRACT}%% exports model: Edge, NodeId\n%% exports lint: run\n");
        let contract = parse(&src).unwrap();
        assert_eq!(contract.surfaces.len(), 2);
        let rendered = render(&contract);
        assert_eq!(parse(&rendered).as_ref(), Ok(&contract));
        assert_eq!(render(&parse(&rendered).unwrap()), rendered);
    }

    #[test]
    fn truncated_and_mutated_inputs_never_panic() {
        let junk = [
            "",
            "\"",
            "[",
            "]",
            "|",
            "-->",
            "-.->",
            "%%",
            "end",
            "subgraph ",
            "\u{e9}",
            "\r",
            "\u{feff}",
            " & ",
        ];
        let mut checked = 0_usize;
        let cuts = OWN_CONTRACT
            .char_indices()
            .map(|(i, _)| i)
            .chain([OWN_CONTRACT.len()]);
        for cut in cuts {
            let prefix = OWN_CONTRACT.get(..cut).unwrap_or_default();
            let suffix = OWN_CONTRACT.get(cut..).unwrap_or_default();
            let _ = parse(prefix);
            let _ = parse(suffix);
            for piece in junk {
                let _ = parse(&format!("{prefix}{piece}{suffix}"));
                let _ = parse(&format!("{prefix}{piece}"));
                checked += 2;
            }
            let dropped: String = OWN_CONTRACT
                .char_indices()
                .filter(|(i, _)| *i != cut)
                .map(|(_, c)| c)
                .collect();
            let _ = parse(&dropped);
            checked += 3;
        }
        assert!(checked > 1000, "only {checked} inputs exercised");
    }
}
