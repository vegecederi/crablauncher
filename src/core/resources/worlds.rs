use crate::core::resources::nbt::{read_nbt_file, write_nbt_file, NbtTag};
use crate::core::resources::packs::dir_size;
use crate::error::{LauncherError, Result};
use std::io::Write;
use std::path::{Component, Path, PathBuf};
use zip::write::SimpleFileOptions;
use zip::{ZipArchive, ZipWriter};

/// Metadados de um mundo local em `.minecraft/saves/<folder>`.
#[derive(Debug, Clone)]
pub struct LocalWorldInfo {
    pub folder_name: String,
    pub display_name: String,
    pub mc_version: String,
    pub game_mode: String,
    pub last_played_ms: i64,
    pub size_bytes: u64,
    pub path: PathBuf,
    pub icon_png: Option<Vec<u8>>,
}

pub fn saves_dir(minecraft_dir: &Path) -> PathBuf {
    minecraft_dir.join("saves")
}

/// Lista todos os mundos locais em `.minecraft/saves/`, lendo metadados de `level.dat`.
pub fn list_worlds(minecraft_dir: &Path) -> Vec<LocalWorldInfo> {
    let dir = saves_dir(minecraft_dir);
    let Ok(entries) = std::fs::read_dir(&dir) else {
        return Vec::new();
    };
    let mut worlds = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let level_dat = path.join("level.dat");
        if !level_dat.is_file() {
            continue;
        }
        let folder_name = entry.file_name().to_string_lossy().to_string();
        let mut display_name = folder_name.clone();
        let mut mc_version = String::new();
        let mut game_mode = String::from("Survival");
        let mut last_played_ms = 0i64;

        if let Ok((_, root)) = read_nbt_file(&level_dat) {
            if let Some(data) = root.get("Data") {
                if let Some(name) = data.get("LevelName").and_then(|t| t.as_str()) {
                    if !name.trim().is_empty() {
                        display_name = name.to_string();
                    }
                }
                if let Some(lp) = data.get("LastPlayed").and_then(|t| t.as_i64()) {
                    last_played_ms = lp;
                }
                if let Some(gm) = data.get("GameType").and_then(|t| t.as_i32()) {
                    game_mode = match gm {
                        1 => "Creative".into(),
                        2 => "Adventure".into(),
                        3 => "Spectator".into(),
                        _ => "Survival".into(),
                    };
                }
                if let Some(ver) = data
                    .get("Version")
                    .and_then(|v| v.get("Name"))
                    .and_then(|t| t.as_str())
                {
                    mc_version = ver.to_string();
                }
            }
        }

        let icon_png = std::fs::read(path.join("icon.png")).ok();
        let size_bytes = dir_size(&path);

        worlds.push(LocalWorldInfo {
            folder_name,
            display_name,
            mc_version,
            game_mode,
            last_played_ms,
            size_bytes,
            path,
            icon_png,
        });
    }
    worlds.sort_by_key(|a| std::cmp::Reverse(a.last_played_ms));
    worlds
}

/// Renomeia o `LevelName` em `level.dat` preservando todos os dados do mundo.
pub fn rename_world(world_dir: &Path, new_name: &str, instance_running: bool) -> Result<()> {
    if instance_running {
        return Err(LauncherError::InvalidMetadata(
            "Cannot rename a world while the instance is running.".into(),
        ));
    }
    let trimmed = new_name.trim();
    if trimmed.is_empty() {
        return Err(LauncherError::InvalidMetadata(
            "World name cannot be empty.".into(),
        ));
    }
    let level_dat = world_dir.join("level.dat");
    let (root_name, mut root) = read_nbt_file(&level_dat)?;
    if let Some(entries) = root.as_compound_mut() {
        if let Some((_, data)) = entries.iter_mut().find(|(k, _)| k == "Data") {
            data.set_compound_field("LevelName", NbtTag::String(trimmed.to_string()));
        }
    }
    write_nbt_file(&level_dat, &root_name, &root, true)
}

/// Exclui um mundo local (bloqueado se a instância estiver em execução).
pub fn delete_world(world_dir: &Path, minecraft_dir: &Path, instance_running: bool) -> Result<()> {
    if instance_running {
        return Err(LauncherError::InvalidMetadata(
            "Cannot delete a world while the instance is running.".into(),
        ));
    }
    let saves = saves_dir(minecraft_dir);
    if !world_dir.starts_with(&saves) {
        return Err(LauncherError::InvalidMetadata(
            "Refusing to delete path outside saves directory.".into(),
        ));
    }
    std::fs::remove_dir_all(world_dir).map_err(|e| LauncherError::Io {
        path: world_dir.to_path_buf(),
        source: e,
    })
}

/// Exporta ou faz backup atômico e verificado de um mundo para um arquivo `.zip`,
/// preservando integralmente o diretório do mundo original.
pub fn export_world_zip(world_dir: &Path, dest_zip: &Path) -> Result<()> {
    if !world_dir.is_dir() {
        return Err(LauncherError::InvalidMetadata(format!(
            "World directory not found: {}",
            world_dir.display()
        )));
    }
    if let Some(parent) = dest_zip.parent() {
        std::fs::create_dir_all(parent).map_err(|e| LauncherError::Io {
            path: parent.to_path_buf(),
            source: e,
        })?;
    }

    let part_path = dest_zip.with_extension("zip.part");
    let create_res = (|| -> Result<()> {
        let file = std::fs::File::create(&part_path).map_err(|e| LauncherError::Io {
            path: part_path.clone(),
            source: e,
        })?;
        let mut zip = ZipWriter::new(file);
        add_dir_to_zip(&mut zip, world_dir, world_dir)?;
        zip.finish()
            .map_err(|e| LauncherError::InvalidMetadata(format!("ZIP finish error: {e}")))?;
        Ok(())
    })();

    if let Err(err) = create_res {
        let _ = std::fs::remove_file(&part_path);
        return Err(err);
    }

    // Valida integridade do arquivo ZIP gerado antes de promovê-lo ao destino final
    let expect_level_dat = world_dir.join("level.dat").is_file();
    if let Err(err) = verify_world_zip(&part_path, expect_level_dat) {
        let _ = std::fs::remove_file(&part_path);
        return Err(err);
    }

    std::fs::rename(&part_path, dest_zip).map_err(|e| LauncherError::Io {
        path: dest_zip.to_path_buf(),
        source: e,
    })?;
    Ok(())
}

/// Verifica se o backup ZIP gerado é legível, não vazio e contém os dados do mundo.
pub fn verify_world_zip(zip_path: &Path, expect_level_dat: bool) -> Result<usize> {
    let file = std::fs::File::open(zip_path).map_err(|e| LauncherError::Io {
        path: zip_path.to_path_buf(),
        source: e,
    })?;
    let mut archive = ZipArchive::new(file)
        .map_err(|e| LauncherError::InvalidMetadata(format!("Invalid backup ZIP: {e}")))?;
    let count = archive.len();
    if count == 0 {
        return Err(LauncherError::InvalidMetadata(
            "Generated world backup ZIP is empty.".into(),
        ));
    }
    if expect_level_dat {
        let mut has_level_dat = false;
        for i in 0..count {
            if let Ok(entry) = archive.by_index(i) {
                let name = entry.name().replace('\\', "/");
                if name == "level.dat" || name.ends_with("/level.dat") {
                    has_level_dat = true;
                    break;
                }
            }
        }
        if !has_level_dat {
            return Err(LauncherError::InvalidMetadata(
                "Generated world backup ZIP is missing level.dat.".into(),
            ));
        }
    }
    Ok(count)
}

/// Importa um mundo a partir de um `.zip` com proteção contra Zip Slip.
pub fn import_world_zip(zip_path: &Path, minecraft_dir: &Path) -> Result<PathBuf> {
    let saves = saves_dir(minecraft_dir);
    std::fs::create_dir_all(&saves).map_err(|e| LauncherError::Io {
        path: saves.clone(),
        source: e,
    })?;
    let stem = zip_path
        .file_stem()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| "Imported World".into());
    let dest = saves.join(&stem);
    if dest.exists() {
        return Err(LauncherError::InvalidMetadata(format!(
            "A world folder named '{stem}' already exists."
        )));
    }
    std::fs::create_dir_all(&dest).map_err(|e| LauncherError::Io {
        path: dest.clone(),
        source: e,
    })?;

    let file = std::fs::File::open(zip_path).map_err(|e| LauncherError::Io {
        path: zip_path.to_path_buf(),
        source: e,
    })?;
    let mut archive = ZipArchive::new(file)
        .map_err(|e| LauncherError::InvalidMetadata(format!("Invalid world ZIP: {e}")))?;

    for i in 0..archive.len() {
        let mut entry = archive
            .by_index(i)
            .map_err(|e| LauncherError::InvalidMetadata(format!("ZIP entry error: {e}")))?;
        let rel = Path::new(entry.name());
        if rel.components().any(|c| {
            matches!(
                c,
                Component::ParentDir | Component::RootDir | Component::Prefix(_)
            )
        }) {
            let _ = std::fs::remove_dir_all(&dest);
            return Err(LauncherError::ZipSlipAttempt {
                entry: entry.name().to_string(),
            });
        }
        let out_path = dest.join(rel);
        if entry.is_dir() {
            std::fs::create_dir_all(&out_path).map_err(|e| LauncherError::Io {
                path: out_path,
                source: e,
            })?;
        } else {
            if let Some(p) = out_path.parent() {
                std::fs::create_dir_all(p).map_err(|e| LauncherError::Io {
                    path: p.to_path_buf(),
                    source: e,
                })?;
            }
            let mut out = std::fs::File::create(&out_path).map_err(|e| LauncherError::Io {
                path: out_path,
                source: e,
            })?;
            std::io::copy(&mut entry, &mut out).map_err(|e| LauncherError::Io {
                path: zip_path.to_path_buf(),
                source: e,
            })?;
        }
    }
    Ok(dest)
}

fn add_dir_to_zip(zip: &mut ZipWriter<std::fs::File>, base: &Path, current: &Path) -> Result<()> {
    let opts = SimpleFileOptions::default();
    for entry in std::fs::read_dir(current)
        .map_err(|e| LauncherError::Io {
            path: current.to_path_buf(),
            source: e,
        })?
        .flatten()
    {
        let path = entry.path();
        let rel = path
            .strip_prefix(base)
            .unwrap_or(&path)
            .to_string_lossy()
            .replace('\\', "/");
        if path.is_dir() {
            add_dir_to_zip(zip, base, &path)?;
        } else {
            zip.start_file(&rel, opts)
                .map_err(|e| LauncherError::InvalidMetadata(format!("ZIP error: {e}")))?;
            let data = std::fs::read(&path).map_err(|e| LauncherError::Io {
                path: path.clone(),
                source: e,
            })?;
            zip.write_all(&data)
                .map_err(|e| LauncherError::Io { path, source: e })?;
        }
    }
    Ok(())
}

/// Cria imediatamente uma cópia independente do mundo em `.minecraft/saves/` com o sufixo "(Backup)",
/// incrementando o sufixo numérico se já existir, sem nunca sobrescrever arquivos existentes.
pub fn backup_world_copy(world_dir: &Path, minecraft_dir: &Path) -> Result<PathBuf> {
    let saves = saves_dir(minecraft_dir);
    if !world_dir.starts_with(&saves) || !world_dir.is_dir() {
        return Err(LauncherError::InvalidMetadata(
            "Invalid world directory for backup.".into(),
        ));
    }
    let base_folder = world_dir
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("World");

    let mut suffix = " (Backup)".to_string();
    let mut dest = saves.join(format!("{base_folder}{suffix}"));
    let mut counter = 2u32;
    while dest.exists() {
        suffix = format!(" (Backup {counter})");
        dest = saves.join(format!("{base_folder}{suffix}"));
        counter += 1;
    }

    copy_world_dir_recursive(world_dir, &dest)?;

    // Atualiza LevelName no level.dat da cópia para refletir o sufixo "(Backup)"
    let level_dat = dest.join("level.dat");
    if level_dat.is_file() {
        if let Ok((root_name, mut root)) = read_nbt_file(&level_dat) {
            let current_name = root
                .get("Data")
                .and_then(|d| d.get("LevelName"))
                .and_then(|t| t.as_str())
                .filter(|s| !s.trim().is_empty())
                .unwrap_or(base_folder)
                .to_string();
            let new_level_name = format!("{current_name}{suffix}");
            if let Some(entries) = root.as_compound_mut() {
                if let Some((_, data)) = entries.iter_mut().find(|(k, _)| k == "Data") {
                    data.set_compound_field("LevelName", NbtTag::String(new_level_name));
                }
            }
            let _ = write_nbt_file(&level_dat, &root_name, &root, true);
        }
    }

    Ok(dest)
}

fn copy_world_dir_recursive(src: &Path, dst: &Path) -> Result<()> {
    std::fs::create_dir_all(dst).map_err(|e| LauncherError::Io {
        path: dst.to_path_buf(),
        source: e,
    })?;
    for entry in std::fs::read_dir(src)
        .map_err(|e| LauncherError::Io {
            path: src.to_path_buf(),
            source: e,
        })?
        .flatten()
    {
        let fname = entry.file_name();
        if fname.to_string_lossy() == "session.lock" {
            continue;
        }
        let p = entry.path();
        let target = dst.join(&fname);
        if p.is_dir() {
            copy_world_dir_recursive(&p, &target)?;
        } else {
            std::fs::copy(&p, &target).map_err(|e| LauncherError::Io {
                path: target,
                source: e,
            })?;
        }
    }
    Ok(())
}

/// Lê a seed do mundo a partir de `level.dat` (`WorldGenSettings.seed` em 1.16+ ou `RandomSeed` em versões antigas).
pub fn read_world_seed(world_dir: &Path) -> Result<i64> {
    let level_dat = world_dir.join("level.dat");
    let (_, root) = read_nbt_file(&level_dat)?;
    let data = root
        .get("Data")
        .ok_or_else(|| LauncherError::InvalidMetadata("Missing Data tag in level.dat".into()))?;
    if let Some(seed) = data
        .get("WorldGenSettings")
        .and_then(|wgs| wgs.get("seed"))
        .and_then(|t| t.as_i64())
    {
        return Ok(seed);
    }
    if let Some(seed) = data.get("RandomSeed").and_then(|t| t.as_i64()) {
        return Ok(seed);
    }
    Err(LauncherError::InvalidMetadata(
        "World seed not found in level.dat".into(),
    ))
}

/// Remove `icon.png` do mundo para restaurar o ícone padrão.
pub fn reset_world_icon(world_dir: &Path) -> Result<bool> {
    let icon_path = world_dir.join("icon.png");
    if icon_path.is_file() {
        std::fs::remove_file(&icon_path).map_err(|e| LauncherError::Io {
            path: icon_path,
            source: e,
        })?;
        Ok(true)
    } else {
        Ok(false)
    }
}
