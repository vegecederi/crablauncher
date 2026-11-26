use crate::core::minecraft::metadata::{DownloadArtifact, Library, LibraryRule, RuleAction};
use regex::Regex;
use std::collections::HashMap;

const DEFAULT_MOJANG_MAVEN_REPO: &str = "https://libraries.minecraft.net/";

/// Representa a plataforma alvo para avaliação das regras de bibliotecas do Minecraft.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TargetPlatform {
    pub os_name: String,
    pub arch: String,
    pub arch_bits: String,
    pub os_version: String,
}

impl TargetPlatform {
    /// Detecta a plataforma atual em tempo de compilação/execução.
    pub fn current() -> Self {
        let os_name = match std::env::consts::OS {
            "linux" => "linux",
            "macos" => "osx",
            "windows" => "windows",
            other => other,
        }
        .to_string();

        let (arch, arch_bits) = match std::env::consts::ARCH {
            "x86_64" => ("x86_64", "64"),
            "x86" => ("x86", "32"),
            "aarch64" => ("arm64", "64"),
            other => (other, "64"),
        };

        Self {
            os_name,
            arch: arch.to_string(),
            arch_bits: arch_bits.to_string(),
            os_version: String::new(),
        }
    }

    /// Identificador compacto usado para isolar diretórios de natives por plataforma.
    pub fn tag(&self) -> String {
        format!("{}-{}", self.os_name, self.arch)
    }
}

/// Artefato de biblioteca resolvido para a plataforma atual.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedLibrary {
    pub maven_name: String,
    pub relative_path: String,
    pub url: String,
    pub sha1: Option<String>,
    pub size: Option<u64>,
    pub is_native: bool,
    pub extract_exclude: Vec<String>,
}

/// Avalia se uma lista de regras da Mojang permite a inclusão da biblioteca.
pub fn rules_allow(
    rules: &[LibraryRule],
    platform: &TargetPlatform,
    enabled_features: &HashMap<String, bool>,
) -> bool {
    if rules.is_empty() {
        return true;
    }

    // Por convenção da Mojang, quando existem regras, o padrão inicial é disallow.
    let mut allowed = false;
    for rule in rules {
        if rule_matches(rule, platform, enabled_features) {
            allowed = rule.action == RuleAction::Allow;
        }
    }
    allowed
}

fn rule_matches(
    rule: &LibraryRule,
    platform: &TargetPlatform,
    enabled_features: &HashMap<String, bool>,
) -> bool {
    if let Some(os) = &rule.os {
        if let Some(name) = &os.name {
            if name != &platform.os_name {
                return false;
            }
        }
        if let Some(arch) = &os.arch {
            if !arch_matches(arch, &platform.arch) {
                return false;
            }
        }
        if let Some(version_pattern) = &os.version {
            if let Ok(re) = Regex::new(version_pattern) {
                if !re.is_match(&platform.os_version) {
                    return false;
                }
            } else {
                return false;
            }
        }
    }

    if let Some(features) = &rule.features {
        for (key, required_value) in features {
            let actual = enabled_features.get(key).copied().unwrap_or(false);
            if actual != *required_value {
                return false;
            }
        }
    }

    true
}

fn arch_matches(rule_arch: &str, platform_arch: &str) -> bool {
    match rule_arch {
        "x86" | "i386" => platform_arch == "x86",
        "x86_64" | "amd64" => platform_arch == "x86_64",
        "arm64" | "aarch64" => platform_arch == "arm64",
        other => other == platform_arch,
    }
}

/// Converte uma coordenada Maven (`group:artifact:version[:classifier][@ext]`)
/// em caminho relativo padrão Maven.
pub fn maven_coordinate_to_path(coord: &str, extra_classifier: Option<&str>) -> Option<String> {
    let (base, ext) = match coord.split_once('@') {
        Some((b, e)) => (b, e),
        None => (coord, "jar"),
    };

    let parts: Vec<&str> = base.split(':').collect();
    if parts.len() < 3 {
        return None;
    }

    let group_path = parts[0].replace('.', "/");
    let artifact = parts[1];
    let version = parts[2];
    let classifier = extra_classifier.or_else(|| parts.get(3).copied());

    let filename = match classifier {
        Some(c) if !c.is_empty() => format!("{artifact}-{version}-{c}.{ext}"),
        _ => format!("{artifact}-{version}.{ext}"),
    };

    Some(format!("{group_path}/{artifact}/{version}/{filename}"))
}

/// Resolve todas as bibliotecas Java e nativas aplicáveis à plataforma atual.
pub fn resolve_libraries(
    libraries: &[Library],
    platform: &TargetPlatform,
    enabled_features: &HashMap<String, bool>,
) -> Vec<ResolvedLibrary> {
    let mut resolved = Vec::new();

    for lib in libraries {
        if !rules_allow(&lib.rules, platform, enabled_features) {
            continue;
        }

        let extract_exclude = lib
            .extract
            .as_ref()
            .map(|e| e.exclude.clone())
            .unwrap_or_default();

        // 1. Artefato Java principal (downloads.artifact ou fallback Maven)
        if let Some(downloads) = &lib.downloads {
            if let Some(artifact) = &downloads.artifact {
                if let Some(item) = build_resolved(
                    &lib.name,
                    artifact,
                    None,
                    false,
                    Vec::new(),
                    lib.url.as_deref(),
                ) {
                    resolved.push(item);
                }
            }

            // 2. Classificador nativo (para versões que usam mapa `natives` + `classifiers`)
            if let (Some(natives_map), Some(classifiers)) = (&lib.natives, &downloads.classifiers) {
                if let Some(raw_classifier) = natives_map.get(&platform.os_name) {
                    let classifier = raw_classifier.replace("${arch}", &platform.arch_bits);
                    if let Some(native_artifact) = classifiers.get(&classifier) {
                        if let Some(item) = build_resolved(
                            &lib.name,
                            native_artifact,
                            Some(&classifier),
                            true,
                            extract_exclude.clone(),
                            lib.url.as_deref(),
                        ) {
                            resolved.push(item);
                        }
                    }
                }
            }
        } else if let Some(rel_path) = maven_coordinate_to_path(&lib.name, None) {
            // Fallback para bibliotecas sem bloco `downloads` explícito (ex.: repositórios Maven legados)
            let repo = lib.url.as_deref().unwrap_or(DEFAULT_MOJANG_MAVEN_REPO);
            let url = format!("{}/{rel_path}", repo.trim_end_matches('/'));
            resolved.push(ResolvedLibrary {
                maven_name: lib.name.clone(),
                relative_path: rel_path,
                url,
                sha1: None,
                size: None,
                is_native: false,
                extract_exclude: Vec::new(),
            });
        }
    }

    resolved
}

fn build_resolved(
    maven_name: &str,
    artifact: &DownloadArtifact,
    classifier: Option<&str>,
    is_native: bool,
    extract_exclude: Vec<String>,
    repo_override: Option<&str>,
) -> Option<ResolvedLibrary> {
    let relative_path = artifact
        .path
        .clone()
        .or_else(|| maven_coordinate_to_path(maven_name, classifier))?;

    let url = if !artifact.url.is_empty() {
        artifact.url.clone()
    } else if let Some(repo) = repo_override {
        format!("{}/{}", repo.trim_end_matches('/'), relative_path)
    } else {
        // Quando downloads.artifact.url é explicitamente vazio sem repositório Maven
        // alternativo (comum no Forge/NeoForge moderno, ex.: forge-*-client.jar), o
        // artefato não está hospedado em libraries.minecraft.net — é extraído do
        // installer.jar ou gerado localmente pelos processors.
        String::new()
    };

    Some(ResolvedLibrary {
        maven_name: maven_name.to_string(),
        relative_path,
        url,
        sha1: artifact.sha1.clone(),
        size: artifact.size,
        is_native,
        extract_exclude,
    })
}
