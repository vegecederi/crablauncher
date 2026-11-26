use crate::core::launch::console::redact_sensitive_text;
use crate::core::launch::environment::LaunchEnvironment;
use crate::core::launch::planner::LaunchPlan;
use crate::core::tweaks::model::ResolvedTweaks;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::PathBuf;

/// Comando final e estruturado de execução do Minecraft (Java + wrappers Linux + env).
/// Nunca utiliza concatenação de strings nem execução via shell.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LaunchCommand {
    pub program: PathBuf,
    pub args: Vec<String>,
    pub env: BTreeMap<String, String>,
    pub working_dir: PathBuf,
    pub applied_tweaks: Vec<String>,
    pub warnings: Vec<String>,
}

impl LaunchCommand {
    /// Constrói um `tokio::process::Command` pronto para execução direta sem shell.
    pub fn to_tokio_command(&self) -> tokio::process::Command {
        let mut cmd = tokio::process::Command::new(&self.program);
        cmd.current_dir(&self.working_dir)
            .args(&self.args)
            .envs(&self.env);
        cmd
    }

    /// Constrói uma representação legível e segura para logs/resumo (sem expor tokens).
    pub fn redacted_display(&self, secrets: &[String]) -> String {
        let mut parts = Vec::new();
        for (k, v) in &self.env {
            parts.push(format!("{k}={v}"));
        }
        parts.push(self.program.to_string_lossy().into_owned());
        for a in &self.args {
            parts.push(a.clone());
        }
        redact_sensitive_text(&parts.join(" "), secrets)
    }
}

/// Centraliza a composição do comando de execução do Minecraft unindo `LaunchPlan` e `ResolvedTweaks`.
/// Ordem dos wrappers quando combinados: switcherooctl -> gamemoderun -> mangohud -> java ...
pub fn build_launch_command(plan: &LaunchPlan, resolved_tweaks: &ResolvedTweaks) -> LaunchCommand {
    let env = LaunchEnvironment::from_tweaks(resolved_tweaks);

    let mut full_chain: Vec<String> = Vec::new();
    for wrapper in &resolved_tweaks.wrappers {
        full_chain.push(wrapper.program.clone());
        full_chain.extend(wrapper.args.clone());
    }

    full_chain.push(plan.java_executable.to_string_lossy().into_owned());
    full_chain.extend(plan.jvm_args.clone());
    full_chain.push(plan.main_class.clone());
    full_chain.extend(plan.game_args.clone());

    let program = PathBuf::from(full_chain.remove(0));
    let mut warnings = plan.warnings.clone();
    warnings.extend(resolved_tweaks.warnings.clone());

    LaunchCommand {
        program,
        args: full_chain,
        env: env.vars().clone(),
        working_dir: plan.working_dir.clone(),
        applied_tweaks: resolved_tweaks.applied_descriptions.clone(),
        warnings,
    }
}
