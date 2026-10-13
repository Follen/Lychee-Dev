use serde::{Deserialize, Serialize};
#[derive(Clone, Deserialize, Serialize)]
pub struct Definition {
    pub path: String,
    pub summary: String,
    pub mutates: bool,
    pub arguments: Vec<String>,
    pub flags: Vec<Flag>,
}
#[derive(Clone, Deserialize, Serialize)]
#[serde(transparent)]
pub struct Flag(pub String);
impl Flag {
    pub fn name(&self) -> &str {
        self.0.split_whitespace().next().unwrap_or("")
    }
    pub fn takes_value(&self) -> bool {
        self.0.split_whitespace().nth(1).is_some_and(|s| {
            s.starts_with('<') || s == "text|json|jsonl" || s == "precise|exploratory" || s == "csv"
        })
    }
}
pub fn all() -> Vec<Definition> {
    serde_json::from_str(include_str!("commands.json")).expect("checked command contract")
}
pub fn implemented() -> Vec<Definition> {
    all()
        .into_iter()
        .filter(|d| match d.path.as_str() {
            "version" | "describe" | "project init" | "project status" | "project lock"
            | "target available" => true,
            // Each business crate publishes only completed handlers; inventory remains exhaustive separately.
            value if value.starts_with("source ") => lycheedev_source::commands().contains(&value),
            value if value.starts_with("data ") || value.starts_with("asset ") => {
                lycheedev_data::commands().contains(&value)
            }
            value if value.starts_with("live ") => lycheedev_live::commands().contains(&value),
            value
                if value.starts_with("addon ")
                    || value.starts_with("skill ")
                    || value == "update" =>
            {
                lycheedev_delivery::commands().contains(&value)
            }
            _ => {
                lycheedev_core::workspace::commands().contains(&d.path.as_str())
                    || lycheedev_core::evidence::commands().contains(&d.path.as_str())
                    || lycheedev_core::cache::commands().contains(&d.path.as_str())
            }
        })
        .collect()
}
