use crate::error::{LauncherError, Result};
use std::path::PathBuf;

pub const APP_ID: &str = "org.crablauncher.Crab";

/// Instala o ícone `logo-icon.png` e o arquivo `.desktop` no diretório XDG do usuário
/// (`~/.local/share/icons/hicolor/...` e `~/.local/share/applications/`), sem exigir root.
#[cfg(target_os = "linux")]
pub fn ensure_linux_desktop_integration(icon_png_bytes: &[u8]) -> Result<()> {
    let data_home = std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/share")))
        .ok_or(LauncherError::DirectoriesUnavailable)?;

    let icon_dir = data_home.join("icons/hicolor/256x256/apps");
    let apps_dir = data_home.join("applications");

    std::fs::create_dir_all(&icon_dir).map_err(|source| LauncherError::Io {
        path: icon_dir.clone(),
        source,
    })?;
    std::fs::create_dir_all(&apps_dir).map_err(|source| LauncherError::Io {
        path: apps_dir.clone(),
        source,
    })?;

    let icon_path = icon_dir.join(format!("{APP_ID}.png"));
    if !icon_path.exists() {
        std::fs::write(&icon_path, icon_png_bytes).map_err(|source| LauncherError::Io {
            path: icon_path.clone(),
            source,
        })?;
    }

    let exec_path = std::env::current_exe()
        .ok()
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_else(|| "crab-launcher".to_string());

    let desktop_content = format!(
        "[Desktop Entry]\n\
         Type=Application\n\
         Name=Crab Launcher\n\
         Comment=A fast, minimalist Minecraft launcher\n\
         Exec=\"{exec_path}\"\n\
         Icon={APP_ID}\n\
         Terminal=false\n\
         Categories=Game;\n\
         StartupWMClass={APP_ID}\n"
    );

    let desktop_path = apps_dir.join(format!("{APP_ID}.desktop"));
    std::fs::write(&desktop_path, desktop_content).map_err(|source| LauncherError::Io {
        path: desktop_path,
        source,
    })?;

    Ok(())
}

#[cfg(not(target_os = "linux"))]
pub fn ensure_linux_desktop_integration(_icon_png_bytes: &[u8]) -> Result<()> {
    Ok(())
}
