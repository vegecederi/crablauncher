use crate::core::java::runtime::JavaRuntime;
use crate::core::minecraft::metadata::VersionMetadata;

/// Determina a major version do Java exigida por uma versão do Minecraft.
/// Fonte primária: campo oficial `javaVersion.majorVersion` nos metadados da Mojang.
/// Fallback documentado: Java 8 para versões legadas sem o campo `javaVersion` (anteriores a 1.17).
pub fn required_java_major(metadata: &VersionMetadata) -> u32 {
    if let Some(req) = &metadata.java_version {
        return req.major_version;
    }
    // Versões anteriores à 1.17 não incluíam o campo `javaVersion` e exigiam Java 8.
    8
}

/// Seleciona o runtime Java mais adequado para a major version exigida.
/// Prioriza correspondência exata; caso não exista e seja Java >= 17, aceita uma versão mais recente compatível.
pub fn select_compatible_runtime(
    runtimes: &[JavaRuntime],
    required_major: u32,
) -> Option<&JavaRuntime> {
    // 1. Correspondência exata (preferindo runtimes gerenciados pelo Crab em caso de empate)
    if let Some(exact) = runtimes
        .iter()
        .find(|rt| rt.major_version == required_major)
    {
        return Some(exact);
    }

    // 2. Para versões modernas (Java 17+), aceita o menor runtime mais novo disponível.
    if required_major >= 17 {
        return runtimes
            .iter()
            .filter(|rt| rt.major_version > required_major)
            .min_by_key(|rt| rt.major_version);
    }

    None
}

/// Resultado da verificação real de compatibilidade entre Java, Minecraft e Modloader.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum JavaCompatibilityStatus {
    Compatible,
    Incompatible {
        required_major: u32,
        recommended_label: String,
        reason: String,
    },
    Unknown {
        required_major: u32,
        recommended_label: String,
        reason: String,
    },
}

/// Avalia a compatibilidade de um runtime Java considerando os metadados do Minecraft,
/// requisitos de versões antigas e restrições conhecidas dos modloaders.
pub fn evaluate_java_compatibility(
    mc_version: &str,
    metadata_required_major: u32,
    modloader_kind: Option<crate::core::components::model::ModloaderKind>,
    probed: Option<&JavaRuntime>,
) -> JavaCompatibilityStatus {
    use crate::core::components::model::ModloaderKind;

    let mut min_major = metadata_required_major.max(8);
    let mut exact_only = metadata_required_major <= 8;

    // Restrições específicas de modloaders e gerações legadas
    if let Some(kind) = modloader_kind {
        match kind {
            ModloaderKind::Forge | ModloaderKind::LiteLoader if min_major <= 8 => {
                // Forge 1.12.2 e anteriores (e LiteLoader) dependem do URLClassLoader do Java 8.
                exact_only = true;
            }
            ModloaderKind::NeoForge => {
                min_major = min_major.max(17);
            }
            ModloaderKind::Fabric | ModloaderKind::Quilt => {
                min_major = min_major.max(8);
            }
            _ => {}
        }
    }

    let recommended_label = if exact_only {
        format!("Java {min_major}")
    } else {
        format!("Java {min_major}+")
    };

    let Some(rt) = probed else {
        return JavaCompatibilityStatus::Unknown {
            required_major: min_major,
            recommended_label,
            reason: "Could not reliably detect the version of the selected Java executable. Verify the path or select a known runtime.".to_string(),
        };
    };

    if rt.major_version == 0 {
        return JavaCompatibilityStatus::Unknown {
            required_major: min_major,
            recommended_label,
            reason: format!(
                "Unable to determine the Java version from '{}'.",
                rt.path.display()
            ),
        };
    }

    if exact_only {
        if rt.major_version == min_major {
            return JavaCompatibilityStatus::Compatible;
        }
        return JavaCompatibilityStatus::Incompatible {
            required_major: min_major,
            recommended_label,
            reason: format!(
                "Minecraft {mc_version} requires Java {min_major} (legacy runtime/classloader compatibility), but the selected runtime is Java {}.",
                rt.major_version
            ),
        };
    }

    if rt.major_version < min_major {
        return JavaCompatibilityStatus::Incompatible {
            required_major: min_major,
            recommended_label,
            reason: format!(
                "Minecraft {mc_version} requires at least Java {min_major}, but the selected runtime is Java {}.",
                rt.major_version
            ),
        };
    }

    // Minecraft 1.17..1.20.4 (Java 16/17) frequentemente quebra com Forge em Java 22+
    if min_major <= 17
        && rt.major_version > 21
        && matches!(modloader_kind, Some(ModloaderKind::Forge))
    {
        return JavaCompatibilityStatus::Incompatible {
            required_major: min_major,
            recommended_label: format!("Java {min_major}"),
            reason: format!(
                "Forge for Minecraft {mc_version} targets Java {min_major} and may fail to boot on Java {}.",
                rt.major_version
            ),
        };
    }

    JavaCompatibilityStatus::Compatible
}
