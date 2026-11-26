use crate::error::{LauncherError, Result};
use std::fs::File;
use std::io;
use std::path::Path;
use zip::ZipArchive;

/// Extrai um arquivo JAR/ZIP de bibliotecas nativas para `dest_dir`,
/// respeitando prefixos de exclusão e protegendo contra Zip Slip / path traversal.
pub fn extract_native_archive(
    archive_path: &Path,
    dest_dir: &Path,
    exclude_prefixes: &[String],
) -> Result<usize> {
    std::fs::create_dir_all(dest_dir).map_err(|source| LauncherError::Io {
        path: dest_dir.to_path_buf(),
        source,
    })?;

    let file = File::open(archive_path).map_err(|source| LauncherError::Io {
        path: archive_path.to_path_buf(),
        source,
    })?;

    let mut archive = ZipArchive::new(file).map_err(|err| LauncherError::ArchiveExtract {
        path: archive_path.to_path_buf(),
        reason: err.to_string(),
    })?;

    let mut extracted_count = 0usize;

    for idx in 0..archive.len() {
        let mut entry = archive
            .by_index(idx)
            .map_err(|err| LauncherError::ArchiveExtract {
                path: archive_path.to_path_buf(),
                reason: err.to_string(),
            })?;

        let raw_name = entry.name().to_string();

        // Ignora entradas excluídas pelas regras dos metadados (ex.: META-INF/).
        if exclude_prefixes
            .iter()
            .any(|prefix| raw_name.starts_with(prefix))
        {
            continue;
        }

        // `enclosed_name()` rejeita caminhos absolutos e componentes `..` (Zip Slip).
        let Some(rel_path) = entry.enclosed_name() else {
            return Err(LauncherError::UnsafePath(format!(
                "Zip Slip / unsafe archive entry '{}' in {}",
                raw_name,
                archive_path.display()
            )));
        };

        let out_path = dest_dir.join(rel_path);
        if entry.is_dir() {
            std::fs::create_dir_all(&out_path).map_err(|source| LauncherError::Io {
                path: out_path,
                source,
            })?;
            continue;
        }

        if let Some(parent) = out_path.parent() {
            std::fs::create_dir_all(parent).map_err(|source| LauncherError::Io {
                path: parent.to_path_buf(),
                source,
            })?;
        }

        let mut out_file = File::create(&out_path).map_err(|source| LauncherError::Io {
            path: out_path.clone(),
            source,
        })?;
        io::copy(&mut entry, &mut out_file).map_err(|source| LauncherError::Io {
            path: out_path,
            source,
        })?;
        extracted_count += 1;
    }

    Ok(extracted_count)
}
