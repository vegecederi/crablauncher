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
