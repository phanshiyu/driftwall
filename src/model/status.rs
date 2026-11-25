//! The status of an edge: one function from what the contract declares and what code does to the
//! state both `lint` and `brief` report (see "Edge status" in `docs/DESIGN.md`).

use serde::Serialize;

use super::{EdgeKind, ImportKind, ViolationKind};

/// What the contract declares for a pair of nodes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Declaration {
    /// No edge allows the pair, and no open chain does.
    None,
    /// No edge, but an `%% order open` chain allows the pair like a solid edge.
    Open,
    /// An edge of this kind allows the pair.
    Edge(EdgeKind),
}

/// What code does on the pairs one declared edge covers.
///
/// This is an enum, not three flags: a site that is allowed, or is not test-only, is a site, so
/// "allowed without realised" cannot be written. `allowed` and `production` stay two independent
/// facts (a `|type|` edge allows a production site, a `|test|` edge a test one, and a runtime
/// production site on a `|test|` edge is production but not allowed), so they are not levels.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Seen {
    /// No site.
    Unseen,
    /// At least one site.
    Sites {
        /// Some site the edge's kind allows.
        allowed: bool,
        /// Some site that is not test-only.
        production: bool,
    },
}

impl Seen {
    /// This observation with one more site of `import_kind`, `test_only` or not, judged against
    /// `kind`, the kind of the edge it is on.
    #[must_use]
    pub fn with_site(self, kind: EdgeKind, import_kind: ImportKind, test_only: bool) -> Self {
        let site = Self::Sites {
            allowed: kind.allows(import_kind, test_only),
            production: !test_only,
        };
        self.merge(site)
    }

    /// What both observations saw together.
    #[must_use]
    pub const fn merge(self, other: Self) -> Self {
        match (self, other) {
            (Self::Unseen, seen) | (seen, Self::Unseen) => seen,
            (
                Self::Sites {
                    allowed,
                    production,
                },
                Self::Sites {
                    allowed: other_allowed,
                    production: other_production,
                },
            ) => Self::Sites {
                allowed: allowed || other_allowed,
                production: production || other_production,
            },
        }
    }
}

/// Where an edge stands: the state `lint` and `brief` agree on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum EdgeStatus {
    /// Declared, and realised the way its kind wants. No finding.
    Ok,
    /// Allowed by an `%% order open` chain without an edge. No finding.
    Open,
    /// Code uses the pair and nothing declares it: `undeclared-edge`.
    Undeclared,
    /// Declared solid, and no site its kind allows: `stale-edge`.
    Stale,
    /// Declared dotted and not realised, which is fine. No finding.
    Planned,
    /// Declared dotted, and code now realises it: `planned-realized`.
    PlannedRealized,
    /// Declared, and only test code uses it where the app should: `test-only-edge`.
    TestOnly,
}

impl EdgeStatus {
    /// The finding `lint` reports for an edge in this status, if any. Total on purpose: a new
    /// status must say what its finding is.
    #[must_use]
    pub const fn finding(self) -> Option<ViolationKind> {
        match self {
            Self::Ok | Self::Open | Self::Planned => None,
            Self::Undeclared => Some(ViolationKind::UndeclaredEdge),
            Self::Stale => Some(ViolationKind::StaleEdge),
            Self::PlannedRealized => Some(ViolationKind::PlannedRealized),
            Self::TestOnly => Some(ViolationKind::TestOnlyEdge),
        }
    }
}

/// The status of an edge with `declaration`, given what code does on it. The one place this is
/// decided; see the table in "Edge status" in `docs/DESIGN.md`.
#[must_use]
pub const fn edge_status(declaration: Declaration, seen: Seen) -> EdgeStatus {
    match declaration {
        Declaration::None => EdgeStatus::Undeclared,
        Declaration::Open => EdgeStatus::Open,
        Declaration::Edge(kind) => declared_status(kind, seen),
    }
}

const fn declared_status(kind: EdgeKind, seen: Seen) -> EdgeStatus {
    let (realised, allowed, production) = match seen {
        Seen::Unseen => (false, false, false),
        Seen::Sites {
            allowed,
            production,
        } => (true, allowed, production),
    };
    match (kind.is_planned(), kind.is_restricted()) {
        // why: an unrestricted solid edge allows every site, so `production` implies `allowed`.
        (false, false) if !realised => EdgeStatus::Stale,
        (false, false) if !production => EdgeStatus::TestOnly,
        (false, false) => EdgeStatus::Ok,
        (false, true) if allowed => EdgeStatus::Ok,
        (false, true) => EdgeStatus::Stale,
        // why: a test using the target is not the app using it, so only production promotes it.
        (true, false) if production => EdgeStatus::PlannedRealized,
        (true, false) if realised => EdgeStatus::TestOnly,
        (true, true) if allowed => EdgeStatus::PlannedRealized,
        (true, _) => EdgeStatus::Planned,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SEEN: [Seen; 5] = [
        Seen::Unseen,
        Seen::Sites {
            allowed: false,
            production: false,
        },
        Seen::Sites {
            allowed: true,
            production: false,
        },
        Seen::Sites {
            allowed: false,
            production: true,
        },
        Seen::Sites {
            allowed: true,
            production: true,
        },
    ];

    fn status(kind: EdgeKind, seen: Seen) -> EdgeStatus {
        edge_status(Declaration::Edge(kind), seen)
    }

    #[test]
    fn solid_unrestricted_edge() {
        let [unseen, _, _, _, production] = SEEN;
        let test_only = Seen::Sites {
            allowed: true,
            production: false,
        };
        assert_eq!(status(EdgeKind::Implemented, unseen), EdgeStatus::Stale);
        assert_eq!(
            status(EdgeKind::Implemented, test_only),
            EdgeStatus::TestOnly
        );
        assert_eq!(status(EdgeKind::Implemented, production), EdgeStatus::Ok);
    }

    #[test]
    fn restricted_edges_follow_allowed_sites_only() {
        for kind in [EdgeKind::Test, EdgeKind::Type, EdgeKind::TypeTest] {
            for seen in SEEN {
                let allowed = matches!(seen, Seen::Sites { allowed: true, .. });
                let expected = if allowed {
                    EdgeStatus::Ok
                } else {
                    EdgeStatus::Stale
                };
                assert_eq!(status(kind, seen), expected, "{kind:?} {seen:?}");
            }
        }
        for kind in [
            EdgeKind::PlannedTest,
            EdgeKind::PlannedType,
            EdgeKind::PlannedTypeTest,
        ] {
            for seen in SEEN {
                let allowed = matches!(seen, Seen::Sites { allowed: true, .. });
                let expected = if allowed {
                    EdgeStatus::PlannedRealized
                } else {
                    EdgeStatus::Planned
                };
                assert_eq!(status(kind, seen), expected, "{kind:?} {seen:?}");
            }
        }
    }

    #[test]
    fn dotted_unrestricted_edge() {
        let [unseen, _, test_only, _, production] = SEEN;
        assert_eq!(status(EdgeKind::Planned, unseen), EdgeStatus::Planned);
        assert_eq!(status(EdgeKind::Planned, test_only), EdgeStatus::TestOnly);
        assert_eq!(
            status(EdgeKind::Planned, production),
            EdgeStatus::PlannedRealized
        );
    }

    #[test]
    fn undeclared_and_open_ignore_the_sites() {
        for seen in SEEN {
            assert_eq!(edge_status(Declaration::None, seen), EdgeStatus::Undeclared);
            assert_eq!(edge_status(Declaration::Open, seen), EdgeStatus::Open);
        }
    }

    #[test]
    fn merge_is_a_union() {
        let [unseen, none, allowed, production, both] = SEEN;
        assert_eq!(unseen.merge(allowed), allowed);
        assert_eq!(allowed.merge(production), both);
        assert_eq!(none.merge(none), none);
    }
}
