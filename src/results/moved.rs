//! What `diff` reports beyond nodes and edges: files that changed owner and nodes that changed
//! boundary (see "Diff beyond the contract" in `docs/DESIGN.md`).

use std::path::{Path, PathBuf};

use serde::{Serialize, Serializer};

use crate::contract::NodeId;

/// A source file whose owning node differs between the base ref and the working tree.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Moved {
    /// The file's path now, relative to the repository root.
    pub file: PathBuf,
    /// The owner at the base ref, `None` for no owner.
    pub from_node: Option<NodeId>,
    /// The owner now, `None` for no owner.
    pub to_node: Option<NodeId>,
    /// Why the owner differs.
    pub cause: MoveCause,
}

/// Why a file's owner differs between the two trees. A move always has a cause: a file that kept
/// its path and its owner is not a move.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MoveCause {
    /// The path is the same and the mapping (`%% map` lines or `.arch` tags) gives it another owner.
    MappingChanged,
    /// Git detected a rename and the old rules would have given the new path the owner it has now.
    Renamed {
        /// The path at the base ref.
        from: PathBuf,
    },
    /// Git detected a rename and the mapping changed as well.
    RenamedAndMappingChanged {
        /// The path at the base ref.
        from: PathBuf,
    },
}

impl MoveCause {
    /// The cause of a move from `from_path` to `path`, given whether the old rules would have
    /// given `path` another owner than the new rules do.
    #[must_use]
    pub fn of(from_path: &Path, path: &Path, old_rules_differ: bool) -> Self {
        if from_path == path {
            Self::MappingChanged
        } else if old_rules_differ {
            Self::RenamedAndMappingChanged {
                from: from_path.to_path_buf(),
            }
        } else {
            Self::Renamed {
                from: from_path.to_path_buf(),
            }
        }
    }

    /// The path the file had at the base ref, when it was renamed.
    #[must_use]
    pub fn renamed_from(&self) -> Option<&Path> {
        match self {
            Self::MappingChanged => None,
            Self::Renamed { from } | Self::RenamedAndMappingChanged { from } => Some(from),
        }
    }

    /// Whether the mapping would give the path a different owner than it did at the base ref.
    #[must_use]
    pub const fn mapping_changed(&self) -> bool {
        !matches!(self, Self::Renamed { .. })
    }

    /// The JSON name of the cause.
    #[must_use]
    pub const fn name(&self) -> &'static str {
        match self {
            Self::MappingChanged => "mapping-changed",
            Self::Renamed { .. } => "renamed",
            Self::RenamedAndMappingChanged { .. } => "renamed-and-mapping-changed",
        }
    }
}

// why: a view struct keeps the old JSON keys and their order, with `cause` added last.
impl Serialize for Moved {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        #[derive(Serialize)]
        struct View<'a> {
            file: &'a Path,
            from_node: &'a Option<NodeId>,
            to_node: &'a Option<NodeId>,
            #[serde(skip_serializing_if = "Option::is_none")]
            renamed_from: Option<&'a Path>,
            mapping_changed: bool,
            cause: &'static str,
        }
        View {
            file: &self.file,
            from_node: &self.from_node,
            to_node: &self.to_node,
            renamed_from: self.cause.renamed_from(),
            mapping_changed: self.cause.mapping_changed(),
            cause: self.cause.name(),
        }
        .serialize(serializer)
    }
}

/// A node that is inside different boundaries in the two contracts.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct BoundaryMove {
    /// The node.
    pub node: NodeId,
    /// Its boundaries before, outermost first joined by `/`, `None` for no boundary.
    pub from: Option<String>,
    /// Its boundaries now, in the same form.
    pub to: Option<String>,
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn moved(cause: MoveCause) -> Moved {
        Moved {
            file: PathBuf::from("beta/widget.ts"),
            from_node: NodeId::new("alpha").ok(),
            to_node: None,
            cause,
        }
    }

    #[test]
    fn a_mapping_change_serialises_without_renamed_from() {
        let value = serde_json::to_value(moved(MoveCause::MappingChanged)).unwrap();
        assert_eq!(
            value,
            json!({"file": "beta/widget.ts", "from_node": "alpha", "to_node": null,
                   "mapping_changed": true, "cause": "mapping-changed"})
        );
    }

    #[test]
    fn a_plain_rename_keeps_mapping_changed_false() {
        let from = PathBuf::from("alpha/widget.ts");
        let value = serde_json::to_value(moved(MoveCause::Renamed { from })).unwrap();
        assert_eq!(value["renamed_from"], "alpha/widget.ts");
        assert_eq!(value["mapping_changed"], false);
        assert_eq!(value["cause"], "renamed");
    }

    #[test]
    fn a_rename_with_a_mapping_change_reports_both() {
        let from = PathBuf::from("alpha/widget.ts");
        let value =
            serde_json::to_value(moved(MoveCause::RenamedAndMappingChanged { from })).unwrap();
        assert_eq!(value["renamed_from"], "alpha/widget.ts");
        assert_eq!(value["mapping_changed"], true);
        assert_eq!(value["cause"], "renamed-and-mapping-changed");
    }

    #[test]
    fn json_keys_keep_their_order_with_cause_last() {
        let text = serde_json::to_string(&moved(MoveCause::Renamed {
            from: PathBuf::from("a.ts"),
        }))
        .unwrap();
        assert_eq!(
            text,
            r#"{"file":"beta/widget.ts","from_node":"alpha","to_node":null,"renamed_from":"a.ts","mapping_changed":false,"cause":"renamed"}"#
        );
    }

    #[test]
    fn the_cause_follows_the_path_and_the_old_rules() {
        let a = Path::new("a.ts");
        let b = Path::new("b.ts");
        assert_eq!(MoveCause::of(a, a, false), MoveCause::MappingChanged);
        assert_eq!(MoveCause::of(a, a, true), MoveCause::MappingChanged);
        assert!(matches!(
            MoveCause::of(a, b, false),
            MoveCause::Renamed { .. }
        ));
        assert!(matches!(
            MoveCause::of(a, b, true),
            MoveCause::RenamedAndMappingChanged { .. }
        ));
    }
}
