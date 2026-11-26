use crate::core::config::detect_conflicting_jvm_args;
use crate::error::{LauncherError, Result};
use serde::{Deserialize, Serialize};

pub const JVM_PRESET_SCHEMA_VERSION: u32 = 1;

/// Definição declarativa de um preset de argumentos JVM (oficial ou personalizado).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct JvmPreset {
    #[serde(default = "default_schema_version")]
    pub schema_version: u32,
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default = "default_min_java")]
    pub min_java_major: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_java_major: Option<u32>,
    #[serde(default)]
    pub supported_arches: Vec<String>,
    #[serde(default = "default_source")]
    pub source: String,
    #[serde(default = "default_version")]
    pub version: String,
    #[serde(skip)]
    pub is_official: bool,
}

fn default_schema_version() -> u32 {
    JVM_PRESET_SCHEMA_VERSION
}

fn default_min_java() -> u32 {
    8
}

fn default_source() -> String {
    "Custom".to_string()
}

fn default_version() -> String {
    "1.0.0".to_string()
}

impl JvmPreset {
    /// Retorna os argumentos unidos por espaço para exibição no editor.
    pub fn args_string(&self) -> String {
        self.args.join(" ")
    }

    /// Resumo legível dos requisitos de compatibilidade do preset.
    pub fn compatibility_summary(&self) -> String {
        let java_req = match self.max_java_major {
            Some(max) => format!("Java {}-{}", self.min_java_major, max),
            None => format!("Java {}+", self.min_java_major),
        };
        if self.supported_arches.is_empty() {
            format!("{java_req} • All architectures")
        } else {
            format!("{java_req} • 64-bit ({})", self.supported_arches.join(", "))
        }
    }

    /// Verifica se o preset é compatível com a versão e arquitetura do runtime Java selecionado.
    pub fn check_compatibility(
        &self,
        java_major: u32,
        arch: &str,
    ) -> std::result::Result<(), String> {
        if java_major < self.min_java_major {
            return Err(format!(
                "Preset '{}' requires Java {}+, but selected runtime is Java {}.",
                self.name, self.min_java_major, java_major
            ));
        }
        if let Some(max) = self.max_java_major {
            if java_major > max {
                return Err(format!(
                    "Preset '{}' supports up to Java {}, but selected runtime is Java {}.",
                    self.name, max, java_major
                ));
            }
        }
        if !self.supported_arches.is_empty() {
            let norm = arch.trim().to_ascii_lowercase();
            let ok = self
                .supported_arches
                .iter()
                .any(|a| a.eq_ignore_ascii_case(&norm));
            if !ok {
                return Err(format!(
                    "Preset '{}' requires architecture [{}], but selected Java is '{}'.",
                    self.name,
                    self.supported_arches.join(", "),
                    arch
                ));
            }
        }
        Ok(())
    }

    /// Valida o preset antes de salvar ou importar, impedindo flags de memória conflitantes,
    /// múltiplos GCs incompatíveis e caracteres de controle/shell perigosos.
    pub fn validate(&self) -> Result<()> {
        if self.id.trim().is_empty()
            || self.id.contains('/')
            || self.id.contains('\\')
            || self.id.contains("..")
        {
            return Err(LauncherError::Launch(format!(
                "Invalid JVM preset identifier '{}'.",
                self.id
            )));
        }
        if self.name.trim().is_empty() {
            return Err(LauncherError::Launch(
                "JVM preset name cannot be empty.".to_string(),
            ));
        }
        if self.min_java_major < 8 {
            return Err(LauncherError::Launch(
                "Minimum Java version must be at least 8.".to_string(),
            ));
        }
        if let Some(max) = self.max_java_major {
            if max < self.min_java_major {
                return Err(LauncherError::Launch(
                    "Maximum Java version cannot be lower than minimum Java version.".to_string(),
                ));
            }
        }

        let joined = self.args_string();
        let mem_conflicts = detect_conflicting_jvm_args(&joined);
        if !mem_conflicts.is_empty() {
            return Err(LauncherError::Launch(format!(
                "JVM presets must not include memory flags ({}); memory is managed in the Memory section.",
                mem_conflicts.join(", ")
            )));
        }

        let gc_flags: Vec<&str> = self
            .args
            .iter()
            .map(String::as_str)
            .filter(|a| is_gc_selector_flag(a))
            .collect();
        if gc_flags.len() > 1 {
            return Err(LauncherError::Launch(format!(
                "Incompatible garbage collector flags cannot be combined: {}",
                gc_flags.join(", ")
            )));
        }

        for arg in &self.args {
            if arg.chars().any(|c| c.is_control())
                || arg.contains(';')
                || arg.contains('|')
                || arg.contains('&')
                || arg.contains('`')
                || arg.contains("$(")
            {
                return Err(LauncherError::Launch(format!(
                    "Disallowed characters in JVM argument '{arg}'."
                )));
            }
        }

        Ok(())
    }
}

fn is_gc_selector_flag(arg: &str) -> bool {
    matches!(
        arg,
        "-XX:+UseG1GC"
            | "-XX:+UseZGC"
            | "-XX:+UseParallelGC"
            | "-XX:+UseSerialGC"
            | "-XX:+UseShenandoahGC"
            | "-XX:+UseConcMarkSweepGC"
    )
}
