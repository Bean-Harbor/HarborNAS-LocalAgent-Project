use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillManifest {
    pub id: String,
    pub version: String,
    #[serde(default)]
    pub capabilities: Vec<String>,
}

pub fn parse_manifest(input: &str) -> Result<SkillManifest, serde_json::Error> {
    serde_json::from_str(input)
}
