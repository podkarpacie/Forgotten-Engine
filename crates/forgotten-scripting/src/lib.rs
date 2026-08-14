//! Transparent TFS Lua compatibility inventory.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Capability {
    Implemented,
    Planned,
    Unsupported,
}

impl Capability {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Implemented => "implemented",
            Self::Planned => "planned",
            Self::Unsupported => "unsupported",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ApiEntry {
    pub api: &'static str,
    pub capability: Capability,
    pub note: &'static str,
}

const MATRIX: &[ApiEntry] = &[
    ApiEntry {
        api: "Player:getLevel()",
        capability: Capability::Implemented,
        note: "Backed by forgotten-core Player level.",
    },
    ApiEntry {
        api: "Player:addExperience()",
        capability: Capability::Implemented,
        note: "Backed by forgotten-core progression.",
    },
    ApiEntry {
        api: "Player:getPosition()",
        capability: Capability::Implemented,
        note: "Backed by forgotten-core Position.",
    },
    ApiEntry {
        api: "Game.createItem()",
        capability: Capability::Planned,
        note: "Item-domain implementation required.",
    },
    ApiEntry {
        api: "Game.createMonster()",
        capability: Capability::Planned,
        note: "Creature spawning implementation required.",
    },
    ApiEntry {
        api: "addEvent()",
        capability: Capability::Planned,
        note: "Scheduler contract required.",
    },
    ApiEntry {
        api: "stopEvent()",
        capability: Capability::Planned,
        note: "Scheduler contract required.",
    },
];

pub fn compatibility_matrix() -> &'static [ApiEntry] {
    MATRIX
}

pub fn find_api(name: &str) -> Option<ApiEntry> {
    MATRIX.iter().copied().find(|entry| entry.api == name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn never_claims_unknown_api_as_supported() {
        assert_eq!(find_api("doCreatureSay()"), None);
        assert_eq!(
            find_api("Player:getLevel()").unwrap().capability,
            Capability::Implemented
        );
    }
}
