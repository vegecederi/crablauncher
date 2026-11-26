use crate::core::java::runtime::{probe_java_executable, JavaRuntime};
use crate::platform::paths::AppPaths;
use futures_util::stream::{self, StreamExt};
use std::collections::HashSet;
use std::path::{Path, PathBuf};

#[cfg(windows)]
const JAVA_BIN_NAME: &str = "java.exe";
#[cfg(not(windows))]
const JAVA_BIN_NAME: &str = "java";

/// Descobre instalações Java no sistema, no `JAVA_HOME`, no `PATH`, em diretórios padrão,
/// nos runtimes gerenciados pelo Crab e em caminhos personalizados.
pub async fn discover_java_runtimes(paths: &AppPaths, custom_paths: &[String]) -> Vec<JavaRuntime> {
    let mut candidates: Vec<(PathBuf, bool)> = Vec::new();

    // 1. Runtimes gerenciados pelo Crab em `runtimes/` (incluindo subdiretórios por provider)
    let runtimes_dir = paths.runtimes_dir();
    if let Ok(entries) = std::fs::read_dir(&runtimes_dir) {
        for entry in entries.flatten() {
            let p = entry.path();
            if p.is_dir() {
                let direct = find_java_bins_in_dir(&p);
                if !direct.is_empty() {
                    for bin in direct {
                        candidates.push((bin, true));
                    }
                } else if let Ok(sub_entries) = std::fs::read_dir(&p) {
                    for sub in sub_entries.flatten() {
                        let sp = sub.path();
                        if sp.is_dir() {
                            for bin in find_java_bins_in_dir(&sp) {
                                candidates.push((bin, true));
                            }
                        }
                    }
                }
            }
        }
    }

    // 2. Caminhos personalizados fornecidos pelo usuário
    for custom in custom_paths {
        let p = PathBuf::from(custom);
        if p.is_file() {
            candidates.push((p, false));
        } else if p.is_dir() {
            for bin in find_java_bins_in_dir(&p) {
                candidates.push((bin, false));
            }
        }
    }

    // 3. Variável de ambiente JAVA_HOME
    if let Ok(java_home) = std::env::var("JAVA_HOME") {
        let bin = PathBuf::from(java_home).join("bin").join(JAVA_BIN_NAME);
        if bin.is_file() {
            candidates.push((bin, false));
        }
    }

    // 4. Executáveis no PATH
    if let Ok(path_var) = std::env::var("PATH") {
        for dir in std::env::split_paths(&path_var) {
            let bin = dir.join(JAVA_BIN_NAME);
            if bin.is_file() {
                candidates.push((bin, false));
            }
        }
    }

    // 5. Diretórios convencionais do sistema operacional
    for root in conventional_jvm_roots() {
        if let Ok(entries) = std::fs::read_dir(&root) {
            for entry in entries.flatten() {
                let p = entry.path();
                if p.is_dir() {
                    for bin in find_java_bins_in_dir(&p) {
                        candidates.push((bin, false));
                    }
                }
            }
        }
    }

    // Deduplica por caminho canônico preservando a marcação `is_managed`.
    let mut seen = HashSet::new();
    let mut unique = Vec::new();
    for (path, is_managed) in candidates {
        let canonical = std::fs::canonicalize(&path).unwrap_or(path);
        if seen.insert(canonical.clone()) {
            unique.push((canonical, is_managed));
        }
    }

    // Executa sondagem em paralelo com limite razoável.
    let mut runtimes: Vec<JavaRuntime> = stream::iter(unique)
        .map(
            |(path, is_managed)| async move { probe_java_executable(&path, is_managed).await.ok() },
        )
        .buffer_unordered(8)
        .filter_map(|opt| async move { opt })
        .collect()
        .await;

    // Ordena por major version decrescente, priorizando runtimes gerenciados em empate.
    runtimes.sort_by(|a, b| {
        b.major_version
            .cmp(&a.major_version)
            .then_with(|| b.is_managed.cmp(&a.is_managed))
    });
    runtimes
}

fn find_java_bins_in_dir(dir: &Path) -> Vec<PathBuf> {
    let mut found = Vec::new();
    let direct = dir.join("bin").join(JAVA_BIN_NAME);
    if direct.is_file() {
        found.push(direct);
    }
    let macos = dir
        .join("Contents")
        .join("Home")
        .join("bin")
        .join(JAVA_BIN_NAME);
    if macos.is_file() {
        found.push(macos);
    }
    // Alguns tarballs (ex.: Temurin) criam um subdiretório interno de versão
    if found.is_empty() {
        if let Ok(entries) = std::fs::read_dir(dir) {
            for entry in entries.flatten() {
                let sub = entry.path();
                if sub.is_dir() {
                    let sub_bin = sub.join("bin").join(JAVA_BIN_NAME);
                    if sub_bin.is_file() {
                        found.push(sub_bin);
                    }
                }
            }
        }
    }
    found
}

fn conventional_jvm_roots() -> Vec<PathBuf> {
    #[cfg(target_os = "linux")]
    {
        vec![
            PathBuf::from("/usr/lib/jvm"),
            PathBuf::from("/usr/lib64/jvm"),
            PathBuf::from("/usr/java"),
        ]
    }
    #[cfg(target_os = "macos")]
    {
        vec![PathBuf::from("/Library/Java/JavaVirtualMachines")]
    }
    #[cfg(target_os = "windows")]
    {
        let mut roots = Vec::new();
        if let Ok(pf) = std::env::var("ProgramFiles") {
            roots.push(PathBuf::from(&pf).join("Java"));
            roots.push(PathBuf::from(&pf).join("Eclipse Adoptium"));
            roots.push(PathBuf::from(&pf).join("Microsoft"));
        }
        roots
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
    {
        Vec::new()
    }
}
