use std::collections::HashMap;

use crate::skills::manifest::SkillManifest;

#[derive(Debug, Default)]
pub struct Registry {
    skills: HashMap<String, SkillManifest>,
    capability_index: HashMap<String, Vec<String>>,
}

impl Registry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn register(&mut self, manifest: SkillManifest) -> Result<(), String> {
        if self.skills.contains_key(&manifest.id) {
            return Err(format!("Skill already registered: {}", manifest.id));
        }

        let skill_id = manifest.id.clone();
        for cap in &manifest.capabilities {
            self.capability_index
                .entry(cap.clone())
                .or_default()
                .push(skill_id.clone());
        }
        self.skills.insert(skill_id, manifest);
        Ok(())
    }

    pub fn get(&self, skill_id: &str) -> Option<&SkillManifest> {
        self.skills.get(skill_id)
    }

    pub fn find_by_capability(&self, capability: &str) -> Vec<&SkillManifest> {
        let Some(ids) = self.capability_index.get(capability) else {
            return Vec::new();
        };
        ids.iter().filter_map(|id| self.skills.get(id)).collect()
    }

    pub fn summary(&self) -> serde_json::Value {
        let mut list = self
            .skills
            .values()
            .map(|m| {
                serde_json::json!({
                    "id": m.id,
                    "version": m.version,
                    "capabilities": m.capabilities,
                })
            })
            .collect::<Vec<_>>();
        list.sort_by_key(|v| v["id"].as_str().unwrap_or_default().to_string());

        serde_json::json!({
            "total_skills": self.skills.len(),
            "total_capabilities": self.capability_index.len(),
            "skills": list,
        })
    }
}
