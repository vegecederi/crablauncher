use crate::core::minecraft::libraries::{resolve_libraries, TargetPlatform};
use crate::core::minecraft::metadata::VersionMetadata;
use crate::platform::paths::AppPaths;
use std::collections::HashMap;
use std::path::PathBuf;

/// Separador de classpath nativo do sistema operacional (`:` no Unix, `;` no Windows).
pub fn classpath_separator() -> &'static str {
    #[cfg(windows)]
    {
        ";"
    }
    #[cfg(not(windows))]
    {
        ":"
    }
}

/// Constrói a lista ordenada de arquivos JAR do classpath (bibliotecas exigidas + client JAR).
pub fn build_classpath_entries(
    metadata: &VersionMetadata,
    paths: &AppPaths,
    platform: &TargetPlatform,
) -> Vec<PathBuf> {
    let resolved = resolve_libraries(&metadata.libraries, platform, &HashMap::new());
    let mut entries: Vec<PathBuf> = resolved
        .into_iter()
        .filter(|lib| !lib.is_native)
        .map(|lib| paths.library_file(&lib.relative_path))
        .collect();

    entries.push(paths.version_jar_file(&metadata.id));
    entries
}

/// Serializa as entradas do classpath em uma única string com o separador do sistema.
pub fn format_classpath(entries: &[PathBuf]) -> String {
    entries
        .iter()
        .map(|p| p.to_string_lossy().into_owned())
        .collect::<Vec<_>>()
        .join(classpath_separator())
}
