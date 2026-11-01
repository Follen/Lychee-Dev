use serde::{Deserialize, Serialize};

/// Stable publishing-service identities; game products are selected separately.
#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Provider {
    Newbee,
    Dd,
    Curseforge,
    Blackbox,
    Modus,
}

impl Provider {
    pub fn id(self) -> &'static str {
        match self {
            Self::Newbee => "newbee",
            Self::Dd => "dd",
            Self::Curseforge => "curseforge",
            Self::Blackbox => "blackbox",
            Self::Modus => "modus",
        }
    }
}
