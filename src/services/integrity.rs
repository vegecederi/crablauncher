use crate::error::{LauncherError, Result};
use sha1::{Digest, Sha1};
use std::path::Path;
use tokio::fs::File;
use tokio::io::AsyncReadExt;

const HASH_BUFFER_SIZE: usize = 64 * 1024;

/// Resultado da verificação de integridade de um arquivo local.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileIntegrity {
    Valid,
    Missing,
    Corrupted,
}

/// Calcula o SHA-1 hexadecimal de um arquivo em streaming (baixo consumo de memória).
pub async fn compute_sha1(path: &Path) -> Result<String> {
    let mut file = File::open(path).await.map_err(|source| LauncherError::Io {
        path: path.to_path_buf(),
        source,
    })?;

    let mut hasher = Sha1::new();
    let mut buf = vec![0u8; HASH_BUFFER_SIZE];

    loop {
        let n = file
            .read(&mut buf)
            .await
            .map_err(|source| LauncherError::Io {
                path: path.to_path_buf(),
                source,
            })?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }

    Ok(hex::encode(hasher.finalize()))
}

/// Verifica se um arquivo existe e corresponde ao tamanho e SHA-1 esperados.
pub async fn verify_file(
    path: &Path,
    expected_sha1: Option<&str>,
    expected_size: Option<u64>,
) -> Result<FileIntegrity> {
    let meta = match tokio::fs::metadata(path).await {
        Ok(m) if m.is_file() => m,
        Ok(_) => return Ok(FileIntegrity::Corrupted),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
            return Ok(FileIntegrity::Missing);
        }
        Err(source) => {
            return Err(LauncherError::Io {
                path: path.to_path_buf(),
                source,
            });
        }
    };

    if let Some(size) = expected_size {
        if meta.len() != size {
            return Ok(FileIntegrity::Corrupted);
        }
    }

    if let Some(expected) = expected_sha1 {
        let actual = compute_sha1(path).await?;
        if !actual.eq_ignore_ascii_case(expected) {
            return Ok(FileIntegrity::Corrupted);
        }
    }

    Ok(FileIntegrity::Valid)
}
