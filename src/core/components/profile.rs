use crate::core::components::model::{ComponentKind, InstanceComponent, ModloaderSpec};
use crate::core::instance::InstanceMetadata;
use crate::core::minecraft::metadata::{Library, VersionArguments, VersionMetadata};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Perfil adicional produzido por um modloader (Fabric, Forge ou NeoForge)
/// salvo em `instances/<dir>/loader-profile.json` para composição com o perfil Vanilla.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LoaderProfile {
    pub id: String,
    #[serde(default)]
    pub inherits_from: Option<String>,
    #[serde(default)]
    pub main_class: Option<String>,
    #[serde(default)]
    pub arguments: Option<VersionArguments>,
    #[serde(default)]
    pub minecraft_arguments: Option<String>,
    #[serde(default)]
    pub libraries: Vec<Library>,
}

/// Compõe os metadados base do Minecraft Vanilla com um `LoaderProfile` opcional,
/// aplicando precedência do modloader em `mainClass`, deduplicação de bibliotecas por `group:artifact`
/// e mesclagem ordenada de argumentos JVM e de jogo.
pub fn compose_version_metadata(
    base: &VersionMetadata,
    loader: Option<&LoaderProfile>,
) -> VersionMetadata {
    let Some(loader) = loader else {
        return base.clone();
    };

    let mut composed = base.clone();

    if let Some(mc) = &loader.main_class {
        composed.main_class = Some(mc.clone());
    }

    // Mescla bibliotecas: bibliotecas do modloader têm precedência sobre a mesma coordenada `group:artifact`.
    let mut loader_keys: HashMap<String, usize> = HashMap::new();
    let mut merged_libs: Vec<Library> =
        Vec::with_capacity(loader.libraries.len() + base.libraries.len());

    for lib in &loader.libraries {
        if let Some(key) = maven_group_artifact(&lib.name) {
            loader_keys.insert(key, merged_libs.len());
        }
        merged_libs.push(lib.clone());
    }

    for lib in &base.libraries {
        if let Some(key) = maven_group_artifact(&lib.name) {
            if loader_keys.contains_key(&key) {
                continue;
            }
        }
        merged_libs.push(lib.clone());
    }
    composed.libraries = merged_libs;

    // Mescla argumentos modernos (JVM e Game)
    if let Some(loader_args) = &loader.arguments {
        let mut base_args = composed.arguments.take().unwrap_or_default();
        base_args.jvm.extend(loader_args.jvm.clone());
        base_args.game.extend(loader_args.game.clone());
        composed.arguments = Some(base_args);
    }

    if let Some(legacy_args) = &loader.minecraft_arguments {
        composed.minecraft_arguments = Some(legacy_args.clone());
    }

    composed
}

/// Constrói a lista real de componentes presentes na instância (Minecraft, LWJGL 2/3, Modloader).
pub fn resolve_instance_components(
    instance: &InstanceMetadata,
    version_meta: Option<&VersionMetadata>,
) -> Vec<InstanceComponent> {
    let mut components = vec![InstanceComponent {
        uid: "net.minecraft".to_string(),
        name: "Minecraft".to_string(),
        version: instance.minecraft_version.clone(),
        kind: ComponentKind::Minecraft,
        required: true,
        status: instance.installation.status.label().to_string(),
    }];

    // Detecta LWJGL 3 ou LWJGL 2 a partir das bibliotecas reais da versão instalada.
    if let Some(meta) = version_meta {
        if let Some((lwjgl_name, lwjgl_ver)) = detect_lwjgl_component(&meta.libraries) {
            components.push(InstanceComponent {
                uid: if lwjgl_name.contains('3') {
                    "org.lwjgl3".to_string()
                } else {
                    "org.lwjgl".to_string()
                },
                name: lwjgl_name,
                version: lwjgl_ver,
                kind: ComponentKind::Lwjgl,
                required: true,
                status: "Resolved".to_string(),
            });
        }
    }

    if let Some(ModloaderSpec { kind, version }) = &instance.modloader_spec {
        components.push(InstanceComponent {
            uid: kind.uid().to_string(),
            name: kind.label().to_string(),
            version: version.clone(),
            kind: ComponentKind::Modloader(*kind),
            required: false,
            status: "Installed".to_string(),
        });
    }

    components
}

fn detect_lwjgl_component(libraries: &[Library]) -> Option<(String, String)> {
    for lib in libraries {
        let parts: Vec<&str> = lib.name.split(':').collect();
        if parts.len() >= 3 {
            if parts[0] == "org.lwjgl" && parts[1] == "lwjgl" {
                return Some(("LWJGL 3".to_string(), parts[2].to_string()));
            }
            if parts[0] == "org.lwjgl.lwjgl" && parts[1] == "lwjgl" {
                return Some(("LWJGL 2".to_string(), parts[2].to_string()));
            }
        }
    }
    None
}

fn maven_group_artifact(coord: &str) -> Option<String> {
    let mut parts = coord.split(':');
    let group = parts.next()?;
    let artifact = parts.next()?;
    Some(format!("{group}:{artifact}"))
}
