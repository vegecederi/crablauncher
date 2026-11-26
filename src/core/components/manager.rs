use crate::core::instance::InstanceMetadata;
use crate::error::{LauncherError, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::io::Write;
use std::path::{Path, PathBuf};
use zip::write::SimpleFileOptions;
use zip::{ZipArchive, ZipWriter};

/// Tipo de componente persistido na aba Version de uma instância.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ManagedComponentKind {
    Minecraft,
    Lwjgl,
    Modloader,
    JarMod,
    CustomJar,
    JavaAgent,
    CustomPatch,
}

impl ManagedComponentKind {
    pub fn label(self) -> &'static str {
        match self {
            Self::Minecraft => "Minecraft",
            Self::Lwjgl => "LWJGL",
            Self::Modloader => "Modloader",
            Self::JarMod => "Jar Mod",
            Self::CustomJar => "Custom Jar",
            Self::JavaAgent => "Java Agent",
            Self::CustomPatch => "Custom Patch",
        }
    }
}

/// Entrada persistida de um componente em `<instance_dir>/components.json`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ManagedComponent {
    pub uid: String,
    pub name: String,
    pub version: String,
    pub kind: ManagedComponentKind,
    pub enabled: bool,
    pub customized: bool,
    pub required: bool,
    /// Caminho relativo dentro de `<instance_dir>` (ex.: `jarmods/optifine.jar`, `agents/agent.jar`, `patches/custom.json`).
    pub local_file: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ComponentManifest {
    pub schema_version: u32,
    pub components: Vec<ManagedComponent>,
}

impl ComponentManifest {
    pub fn path_for(instance_dir: &Path) -> PathBuf {
        instance_dir.join("components.json")
    }

    /// Carrega `<instance_dir>/components.json` ou inicializa a partir dos metadados da instância.
    pub fn load_or_init(instance_dir: &Path, inst: &InstanceMetadata) -> Self {
        let path = Self::path_for(instance_dir);
        if let Ok(raw) = std::fs::read_to_string(&path) {
            if let Ok(mut parsed) = serde_json::from_str::<Self>(&raw) {
                // Garante que a versão do Minecraft esteja sincronizada
                if let Some(mc) = parsed
                    .components
                    .iter_mut()
                    .find(|c| c.kind == ManagedComponentKind::Minecraft)
                {
                    mc.version = inst.minecraft_version.clone();
                }
                return parsed;
            }
        }
        let mut comps = vec![
            ManagedComponent {
                uid: "org.lwjgl".into(),
                name: "LWJGL".into(),
                version: "Auto".into(),
                kind: ManagedComponentKind::Lwjgl,
                enabled: true,
                customized: false,
                required: true,
                local_file: None,
            },
            ManagedComponent {
                uid: "net.minecraft".into(),
                name: "Minecraft".into(),
                version: inst.minecraft_version.clone(),
                kind: ManagedComponentKind::Minecraft,
                enabled: true,
                customized: false,
                required: true,
                local_file: None,
            },
        ];
        if let Some(ref ml) = inst.modloader_spec {
            comps.push(ManagedComponent {
                uid: format!("loader.{}", ml.kind.short_name().to_lowercase()),
                name: ml.kind.label().to_string(),
                version: ml.version.clone(),
                kind: ManagedComponentKind::Modloader,
                enabled: true,
                customized: false,
                required: false,
                local_file: None,
            });
        }
        Self {
            schema_version: 1,
            components: comps,
        }
    }

    pub fn save(&self, instance_dir: &Path) -> Result<()> {
        std::fs::create_dir_all(instance_dir).map_err(|source| LauncherError::Io {
            path: instance_dir.to_path_buf(),
            source,
        })?;
        let path = Self::path_for(instance_dir);
        let tmp = instance_dir.join("components.json.tmp");
        let json = serde_json::to_string_pretty(self)?;
        std::fs::write(&tmp, json).map_err(|source| LauncherError::Io {
            path: tmp.clone(),
            source,
        })?;
        std::fs::rename(&tmp, &path).map_err(|source| LauncherError::Io { path, source })
    }

    pub fn move_up(&mut self, uid: &str) -> bool {
        if let Some(pos) = self.components.iter().position(|c| c.uid == uid) {
            if pos > 0 {
                self.components.swap(pos, pos - 1);
                return true;
            }
        }
        false
    }

    pub fn move_down(&mut self, uid: &str) -> bool {
        if let Some(pos) = self.components.iter().position(|c| c.uid == uid) {
            if pos + 1 < self.components.len() {
                self.components.swap(pos, pos + 1);
                return true;
            }
        }
        false
    }

    pub fn remove(&mut self, instance_dir: &Path, uid: &str) -> Result<bool> {
        let Some(pos) = self.components.iter().position(|c| c.uid == uid) else {
            return Ok(false);
        };
        if self.components[pos].required {
            return Err(LauncherError::InvalidMetadata(
                "Core required components cannot be removed.".into(),
            ));
        }
        let removed = self.components.remove(pos);
        if let Some(rel) = removed.local_file {
            let p = instance_dir.join(rel);
            if p.is_file() {
                let _ = std::fs::remove_file(p);
            }
        }
        Ok(true)
    }

    /// Cria um patch JSON editável em `<instance_dir>/patches/<uid>.json` para customizar o componente.
    pub fn customize(&mut self, instance_dir: &Path, uid: &str) -> Result<PathBuf> {
        let patches_dir = instance_dir.join("patches");
        std::fs::create_dir_all(&patches_dir).map_err(|source| LauncherError::Io {
            path: patches_dir.clone(),
            source,
        })?;
        let comp = self
            .components
            .iter_mut()
            .find(|c| c.uid == uid)
            .ok_or_else(|| {
                LauncherError::InvalidMetadata(format!("Component '{uid}' not found"))
            })?;
        let patch_path = patches_dir.join(format!("{uid}.json"));
        if !patch_path.is_file() {
            let stub = serde_json::json!({
                "uid": comp.uid,
                "name": comp.name,
                "version": comp.version,
                "formatVersion": 1
            });
            std::fs::write(&patch_path, serde_json::to_string_pretty(&stub)?).map_err(
                |source| LauncherError::Io {
                    path: patch_path.clone(),
                    source,
                },
            )?;
        }
        comp.customized = true;
        Ok(patch_path)
    }

    /// Reverte customizações de um componente removendo seu patch em `<instance_dir>/patches/<uid>.json`.
    pub fn revert(&mut self, instance_dir: &Path, uid: &str) -> Result<bool> {
        let comp = self
            .components
            .iter_mut()
            .find(|c| c.uid == uid)
            .ok_or_else(|| {
                LauncherError::InvalidMetadata(format!("Component '{uid}' not found"))
            })?;
        let patch_path = instance_dir.join("patches").join(format!("{uid}.json"));
        if patch_path.is_file() {
            let _ = std::fs::remove_file(&patch_path);
        }
        comp.customized = false;
        Ok(true)
    }

    /// Adiciona um JAR/ZIP para ser mesclado em `minecraft.jar` (exclusivo desta instância, nunca altera o original).
    pub fn add_jar_mod(&mut self, instance_dir: &Path, source: &Path) -> Result<()> {
        let fname = source
            .file_name()
            .ok_or_else(|| LauncherError::InvalidMetadata("Invalid JAR path".into()))?
            .to_string_lossy()
            .to_string();
        let dir = instance_dir.join("jarmods");
        std::fs::create_dir_all(&dir).map_err(|source| LauncherError::Io {
            path: dir.clone(),
            source,
        })?;
        let dest = dir.join(&fname);
        std::fs::copy(source, &dest).map_err(|source| LauncherError::Io {
            path: dest.clone(),
            source,
        })?;
        let uid = format!("jarmod.{}", sanitize_uid(&fname));
        self.components.retain(|c| c.uid != uid);
        self.components.push(ManagedComponent {
            uid,
            name: format!("{fname} (jar mod)"),
            version: "(Custom)".into(),
            kind: ManagedComponentKind::JarMod,
            enabled: true,
            customized: true,
            required: false,
            local_file: Some(format!("jarmods/{fname}")),
        });
        Ok(())
    }

    /// Substitui o `minecraft.jar` exclusivamente para esta instância (reversível via Remove/Revert).
    pub fn replace_minecraft_jar(&mut self, instance_dir: &Path, source: &Path) -> Result<()> {
        let fname = source
            .file_name()
            .ok_or_else(|| LauncherError::InvalidMetadata("Invalid JAR path".into()))?
            .to_string_lossy()
            .to_string();
        let dir = instance_dir.join("jarmods");
        std::fs::create_dir_all(&dir).map_err(|source| LauncherError::Io {
            path: dir.clone(),
            source,
        })?;
        let dest = dir.join(&fname);
        std::fs::copy(source, &dest).map_err(|source| LauncherError::Io {
            path: dest.clone(),
            source,
        })?;
        // Remove substituição anterior, mantendo apenas uma ativa
        self.components
            .retain(|c| c.kind != ManagedComponentKind::CustomJar);
        self.components.push(ManagedComponent {
            uid: "custom.minecraft.jar".into(),
            name: format!("{fname} (custom jar)"),
            version: "(Custom)".into(),
            kind: ManagedComponentKind::CustomJar,
            enabled: true,
            customized: true,
            required: false,
            local_file: Some(format!("jarmods/{fname}")),
        });
        Ok(())
    }

    /// Adiciona um Java Agent (`-javaagent:<path>`) exclusivo desta instância.
    pub fn add_java_agent(&mut self, instance_dir: &Path, source: &Path) -> Result<()> {
        let fname = source
            .file_name()
            .ok_or_else(|| LauncherError::InvalidMetadata("Invalid agent JAR path".into()))?
            .to_string_lossy()
            .to_string();
        let dir = instance_dir.join("agents");
        std::fs::create_dir_all(&dir).map_err(|source| LauncherError::Io {
            path: dir.clone(),
            source,
        })?;
        let dest = dir.join(&fname);
        std::fs::copy(source, &dest).map_err(|source| LauncherError::Io {
            path: dest.clone(),
            source,
        })?;
        let uid = format!("agent.{}", sanitize_uid(&fname));
        self.components.retain(|c| c.uid != uid);
        self.components.push(ManagedComponent {
            uid,
            name: format!("{fname} (agent)"),
            version: "(Agent)".into(),
            kind: ManagedComponentKind::JavaAgent,
            enabled: true,
            customized: true,
            required: false,
            local_file: Some(format!("agents/{fname}")),
        });
        Ok(())
    }

    /// Cria um componente JSON vazio em `<instance_dir>/patches/`.
    pub fn add_empty_component(&mut self, instance_dir: &Path, name: &str) -> Result<PathBuf> {
        let uid = format!("custom.{}", sanitize_uid(name));
        let patches_dir = instance_dir.join("patches");
        std::fs::create_dir_all(&patches_dir).map_err(|source| LauncherError::Io {
            path: patches_dir.clone(),
            source,
        })?;
        let patch_path = patches_dir.join(format!("{uid}.json"));
        let stub = serde_json::json!({
            "uid": uid,
            "name": name,
            "version": "1.0.0",
            "formatVersion": 1
        });
        std::fs::write(&patch_path, serde_json::to_string_pretty(&stub)?).map_err(|source| {
            LauncherError::Io {
                path: patch_path.clone(),
                source,
            }
        })?;
        self.components.push(ManagedComponent {
            uid: uid.clone(),
            name: name.to_string(),
            version: "1.0.0".into(),
            kind: ManagedComponentKind::CustomPatch,
            enabled: true,
            customized: true,
            required: false,
            local_file: Some(format!("patches/{uid}.json")),
        });
        Ok(patch_path)
    }

    /// Importa um arquivo JSON de componente para `<instance_dir>/patches/`.
    pub fn import_component_json(&mut self, instance_dir: &Path, source: &Path) -> Result<()> {
        let raw = std::fs::read_to_string(source).map_err(|e| LauncherError::Io {
            path: source.to_path_buf(),
            source: e,
        })?;
        let val: serde_json::Value = serde_json::from_str(&raw)?;
        let name = val
            .get("name")
            .and_then(|v| v.as_str())
            .unwrap_or("Imported Component")
            .to_string();
        let uid = val
            .get("uid")
            .and_then(|v| v.as_str())
            .map(sanitize_uid)
            .unwrap_or_else(|| format!("imported.{}", sanitize_uid(&name)));
        let version = val
            .get("version")
            .and_then(|v| v.as_str())
            .unwrap_or("Custom")
            .to_string();

        let patches_dir = instance_dir.join("patches");
        std::fs::create_dir_all(&patches_dir).map_err(|e| LauncherError::Io {
            path: patches_dir.clone(),
            source: e,
        })?;
        let dest = patches_dir.join(format!("{uid}.json"));
        std::fs::write(&dest, raw).map_err(|e| LauncherError::Io {
            path: dest.clone(),
            source: e,
        })?;

        self.components.retain(|c| c.uid != uid);
        self.components.push(ManagedComponent {
            uid: uid.clone(),
            name,
            version,
            kind: ManagedComponentKind::CustomPatch,
            enabled: true,
            customized: true,
            required: false,
            local_file: Some(format!("patches/{uid}.json")),
        });
        Ok(())
    }

    /// Retorna argumentos `-javaagent:<caminho>` para todos os Java Agents ativos desta instância.
    pub fn java_agent_args(&self, instance_dir: &Path) -> Vec<String> {
        self.components
            .iter()
            .filter(|c| c.enabled && c.kind == ManagedComponentKind::JavaAgent)
            .filter_map(|c| c.local_file.as_deref())
            .map(|rel| instance_dir.join(rel))
            .filter(|p| p.is_file())
            .map(|p| format!("-javaagent:{}", p.display()))
            .collect()
    }
}

/// Constrói um `minecraft-patched.jar` isolado em `<instance_dir>/patched/` aplicando
/// `CustomJar` (se existir) e sobrepondo todos os `JarMod`s ativos em ordem, removendo
/// assinaturas `META-INF/*.SF`/`*.DSA`/`*.RSA`. Nunca modifica `base_jar` compartilhado.
pub fn build_patched_client_jar(
    base_jar: &Path,
    instance_dir: &Path,
    manifest: &ComponentManifest,
) -> Result<PathBuf> {
    let custom_jar = manifest
        .components
        .iter()
        .find(|c| c.enabled && c.kind == ManagedComponentKind::CustomJar)
        .and_then(|c| c.local_file.as_deref())
        .map(|rel| instance_dir.join(rel))
        .filter(|p| p.is_file());

    let jar_mods: Vec<PathBuf> = manifest
        .components
        .iter()
        .filter(|c| c.enabled && c.kind == ManagedComponentKind::JarMod)
        .filter_map(|c| c.local_file.as_deref())
        .map(|rel| instance_dir.join(rel))
        .filter(|p| p.is_file())
        .collect();

    // Se não há substituição nem jar mods, usa o base_jar diretamente sem cópia extra
    if custom_jar.is_none() && jar_mods.is_empty() {
        return Ok(base_jar.to_path_buf());
    }

    let source_base = custom_jar.as_deref().unwrap_or(base_jar);
    if jar_mods.is_empty() {
        return Ok(source_base.to_path_buf());
    }

    let patched_dir = instance_dir.join("patched");
    std::fs::create_dir_all(&patched_dir).map_err(|e| LauncherError::Io {
        path: patched_dir.clone(),
        source: e,
    })?;
    let out_jar = patched_dir.join("minecraft-patched.jar");
    let out_file = std::fs::File::create(&out_jar).map_err(|e| LauncherError::Io {
        path: out_jar.clone(),
        source: e,
    })?;
    let mut writer = ZipWriter::new(out_file);
    let mut written_entries: HashSet<String> = HashSet::new();
    let opts = SimpleFileOptions::default();

    // Primeiro escreve os JarMods em ordem reversa de prioridade (último da lista vence)
    for mod_jar in jar_mods.iter().rev() {
        copy_zip_entries_filtered(mod_jar, &mut writer, &mut written_entries, opts, true)?;
    }
    // Depois escreve o JAR base (ignorando arquivos já sobrescritos e assinaturas META-INF)
    copy_zip_entries_filtered(source_base, &mut writer, &mut written_entries, opts, true)?;

    writer
        .finish()
        .map_err(|e| LauncherError::InvalidMetadata(format!("ZIP finish error: {e}")))?;
    Ok(out_jar)
}

fn copy_zip_entries_filtered(
    src_zip: &Path,
    writer: &mut ZipWriter<std::fs::File>,
    seen: &mut HashSet<String>,
    opts: SimpleFileOptions,
    strip_meta_inf_sigs: bool,
) -> Result<()> {
    let f = std::fs::File::open(src_zip).map_err(|e| LauncherError::Io {
        path: src_zip.to_path_buf(),
        source: e,
    })?;
    let mut archive = ZipArchive::new(f)
        .map_err(|e| LauncherError::InvalidMetadata(format!("Invalid JAR archive: {e}")))?;
    for i in 0..archive.len() {
        let mut entry = archive
            .by_index(i)
            .map_err(|e| LauncherError::InvalidMetadata(format!("JAR entry error: {e}")))?;
        let name = entry.name().to_string();
        if entry.is_dir() {
            continue;
        }
        if strip_meta_inf_sigs && is_meta_inf_signature(&name) {
            continue;
        }
        if !seen.insert(name.clone()) {
            continue;
        }
        writer
            .start_file(&name, opts)
            .map_err(|e| LauncherError::InvalidMetadata(format!("ZIP write error: {e}")))?;
        let mut buf = Vec::new();
        std::io::copy(&mut entry, &mut buf).map_err(|e| LauncherError::Io {
            path: src_zip.to_path_buf(),
            source: e,
        })?;
        writer.write_all(&buf).map_err(|e| LauncherError::Io {
            path: src_zip.to_path_buf(),
            source: e,
        })?;
    }
    Ok(())
}

fn is_meta_inf_signature(name: &str) -> bool {
    let upper = name.to_ascii_uppercase();
    upper.starts_with("META-INF/")
        && (upper.ends_with(".SF") || upper.ends_with(".DSA") || upper.ends_with(".RSA"))
}

fn sanitize_uid(s: &str) -> String {
    s.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '.' || c == '-' {
                c.to_ascii_lowercase()
            } else {
                '_'
            }
        })
        .collect()
}
