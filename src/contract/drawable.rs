//! What Mermaid can draw as a node id, checked against Mermaid 11.

use super::NodeId;

/// The words Mermaid reads as syntax wherever they stand alone as an id.
const KEYWORDS: [&str; 9] = [
    "class",
    "style",
    "graph",
    "click",
    "call",
    "href",
    "interpolate",
    "linkStyle",
    "classDef",
];

/// The words that, followed by `-`, Mermaid still reads as syntax (`click-x` is an ordinary id).
const HYPHEN_KEYWORDS: [&str; 9] = [
    "class",
    "style",
    "graph",
    "interpolate",
    "linkStyle",
    "classDef",
    "end",
    "subgraph",
    "flowchart",
];

impl NodeId {
    /// Whether Mermaid cannot parse `id` as the id of a node.
    ///
    /// The ids it rejects: a keyword on its own, the keywords that stay keywords before a `-`
    /// (`end-user`, `style-guide`), and any id with `--`. Case matters: `End` and `styles` are
    /// ordinary ids.
    #[must_use]
    pub fn mermaid_rejects(id: &str) -> bool {
        if id.contains("--")
            || KEYWORDS.contains(&id)
            || matches!(id, "end" | "subgraph" | "flowchart")
        {
            return true;
        }
        HYPHEN_KEYWORDS.iter().any(|word| {
            id.strip_prefix(word)
                .is_some_and(|rest| rest.starts_with('-'))
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_ids_mermaid_rejects_are_the_keywords_their_hyphen_forms_and_double_dashes() {
        for bad in [
            "class",
            "style",
            "graph",
            "click",
            "call",
            "href",
            "interpolate",
            "linkStyle",
            "classDef",
            "end",
            "subgraph",
            "flowchart",
            "end-user",
            "graph-x",
            "style-guide",
            "subgraph-a",
            "flowchart-a",
            "interpolate-a",
            "linkStyle-a",
            "classDef-a",
            "class-a",
            "a--b",
            "x---y",
            "a--",
        ] {
            assert!(NodeId::mermaid_rejects(bad), "{bad}");
        }
        for good in [
            "End",
            "endx",
            "a-end",
            "default",
            "styles",
            "end_user",
            "click-x",
            "call-x",
            "href-x",
            "Class",
            "class_x",
            "classes",
            "x-class",
            "a-b",
            "direction",
            "default-x",
        ] {
            assert!(!NodeId::mermaid_rejects(good), "{good}");
        }
    }
}
