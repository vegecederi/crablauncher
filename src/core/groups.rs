use crate::error::{LauncherError, Result};
use crate::platform::paths::AppPaths;
use crate::services::storage::{read_json, write_json_atomic};
use serde::{Deserialize, Serialize};
use tracing::warn;
use uuid::Uuid;

pub const GROUPS_SCHEMA_VERSION: u32 = 1;
const MAX_GROUP_NAME_LEN: usize = 48;

/// Grupo visual/lógico de instâncias (inspirado no Prism Launcher).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InstanceGroup {
    pub id: String,
    pub name: String,
    pub order: usize,
    #[serde(default)]
    pub collapsed: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GroupsFile {
    #[serde(default = "default_schema_version")]
    pub schema_version: u32,
    #[serde(default)]
    pub groups: Vec<InstanceGroup>,
}

fn default_schema_version() -> u32 {
    GROUPS_SCHEMA_VERSION
}

impl Default for GroupsFile {
    fn default() -> Self {
        Self {
            schema_version: GROUPS_SCHEMA_VERSION,
            groups: Vec::new(),
        }
    }
}

/// Gerencia a persistência e ordenação dos grupos de instâncias em `groups.json`.
#[derive(Clone)]
pub struct GroupManager {
    paths: AppPaths,
}

impl GroupManager {
    pub fn new(paths: AppPaths) -> Self {
        Self { paths }
    }

    pub async fn load(&self) -> Vec<InstanceGroup> {
        let path = self.paths.groups_file();
        if !path.exists() {
            return Vec::new();
        }
        match read_json::<GroupsFile>(&path).await {
            Ok(mut file) => {
                file.groups.sort_by_key(|g| g.order);
                file.groups
            }
            Err(err) => {
                warn!(
                    "Invalid groups.json at {}: {err}. Using empty list.",
                    path.display()
                );
                Vec::new()
            }
        }
    }

    pub async fn save(&self, groups: &[InstanceGroup]) -> Result<()> {
        let mut normalized: Vec<InstanceGroup> = groups.to_vec();
        for (idx, g) in normalized.iter_mut().enumerate() {
            g.order = idx;
        }
        let file = GroupsFile {
            schema_version: GROUPS_SCHEMA_VERSION,
            groups: normalized,
        };
        write_json_atomic(&self.paths.groups_file(), &file).await
    }

    pub async fn create_group(&self, raw_name: &str) -> Result<InstanceGroup> {
        let name = validate_group_name(raw_name)?;
        let mut groups = self.load().await;
        if groups.iter().any(|g| g.name.eq_ignore_ascii_case(&name)) {
            return Err(LauncherError::InvalidGroupName(format!(
                "A group named '{name}' already exists."
            )));
        }

        let group = InstanceGroup {
            id: Uuid::new_v4().to_string(),
            name,
            order: groups.len(),
            collapsed: false,
        };
        groups.push(group.clone());
        self.save(&groups).await?;
        Ok(group)
    }

    pub async fn rename_group(&self, id: &str, raw_name: &str) -> Result<InstanceGroup> {
        let name = validate_group_name(raw_name)?;
        let mut groups = self.load().await;
        if groups
            .iter()
            .any(|g| g.id != id && g.name.eq_ignore_ascii_case(&name))
        {
            return Err(LauncherError::InvalidGroupName(format!(
                "A group named '{name}' already exists."
            )));
        }

        let target = groups
            .iter_mut()
            .find(|g| g.id == id)
            .ok_or_else(|| LauncherError::GroupNotFound(id.to_string()))?;
        target.name = name;
        let updated = target.clone();
        self.save(&groups).await?;
        Ok(updated)
    }

    pub async fn delete_group(&self, id: &str) -> Result<()> {
        let mut groups = self.load().await;
        let before = groups.len();
        groups.retain(|g| g.id != id);
        if groups.len() == before {
            return Err(LauncherError::GroupNotFound(id.to_string()));
        }
        self.save(&groups).await
    }

    pub async fn set_collapsed(&self, id: &str, collapsed: bool) -> Result<()> {
        let mut groups = self.load().await;
        let target = groups
            .iter_mut()
            .find(|g| g.id == id)
            .ok_or_else(|| LauncherError::GroupNotFound(id.to_string()))?;
        target.collapsed = collapsed;
        self.save(&groups).await
    }

    pub async fn move_group(&self, id: &str, up: bool) -> Result<Vec<InstanceGroup>> {
        let mut groups = self.load().await;
        let Some(pos) = groups.iter().position(|g| g.id == id) else {
            return Err(LauncherError::GroupNotFound(id.to_string()));
        };

        if up && pos > 0 {
            groups.swap(pos, pos - 1);
        } else if !up && pos + 1 < groups.len() {
            groups.swap(pos, pos + 1);
        }
        self.save(&groups).await?;
        Ok(groups)
    }
}

pub fn validate_group_name(raw: &str) -> Result<String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err(LauncherError::InvalidGroupName(
            "Group name cannot be empty.".to_string(),
        ));
    }
    if trimmed.chars().count() > MAX_GROUP_NAME_LEN {
        return Err(LauncherError::InvalidGroupName(format!(
            "Group name cannot exceed {MAX_GROUP_NAME_LEN} characters."
        )));
    }
    if trimmed.chars().any(|c| c.is_control()) {
        return Err(LauncherError::InvalidGroupName(
            "Group name cannot contain control characters.".to_string(),
        ));
    }
    Ok(trimmed.to_string())
}
