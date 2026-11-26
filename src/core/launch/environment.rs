use crate::core::tweaks::model::ResolvedTweaks;
use std::collections::BTreeMap;

/// Variáveis de ambiente aplicadas exclusivamente ao processo filho do Minecraft.
/// Nunca modifica o ambiente global do processo do Crab.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct LaunchEnvironment {
    vars: BTreeMap<String, String>,
}

impl LaunchEnvironment {
    pub fn from_tweaks(tweaks: &ResolvedTweaks) -> Self {
        Self {
            vars: tweaks.env_vars.clone(),
        }
    }

    pub fn vars(&self) -> &BTreeMap<String, String> {
        &self.vars
    }

    pub fn is_empty(&self) -> bool {
        self.vars.is_empty()
    }
}
