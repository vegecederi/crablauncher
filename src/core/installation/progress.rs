/// Etapas macro do pipeline de instalação de uma instância Vanilla.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InstallStage {
    ResolvingMetadata,
    CheckingLocalFiles,
    DownloadingClient,
    DownloadingLibraries,
    DownloadingAssets,
    ExtractingNatives,
    Verifying,
    Complete,
}

impl InstallStage {
    pub fn message(self) -> &'static str {
        match self {
            Self::ResolvingMetadata => "Resolving version metadata...",
            Self::CheckingLocalFiles => "Checking local files...",
            Self::DownloadingClient => "Downloading Minecraft client...",
            Self::DownloadingLibraries => "Downloading libraries...",
            Self::DownloadingAssets => "Downloading assets...",
            Self::ExtractingNatives => "Extracting native libraries...",
            Self::Verifying => "Verifying installation...",
            Self::Complete => "Installation complete.",
        }
    }
}

/// Snapshot de progresso emitido periodicamente para a interface.
#[derive(Debug, Clone)]
pub struct InstallProgress {
    pub instance_id: String,
    pub stage: InstallStage,
    pub message: String,
    pub completed_files: usize,
    pub total_files: usize,
    pub downloaded_bytes: u64,
    pub total_bytes: u64,
    pub bytes_per_sec: u64,
    pub indeterminate: bool,
}

impl InstallProgress {
    pub fn indeterminate(instance_id: impl Into<String>, stage: InstallStage) -> Self {
        Self {
            instance_id: instance_id.into(),
            stage,
            message: stage.message().to_string(),
            completed_files: 0,
            total_files: 0,
            downloaded_bytes: 0,
            total_bytes: 0,
            bytes_per_sec: 0,
            indeterminate: true,
        }
    }
}

/// Formata um volume de bytes em string legível (B, KB, MB, GB).
pub fn format_bytes(bytes: u64) -> String {
    const KB: f64 = 1024.0;
    const MB: f64 = KB * 1024.0;
    const GB: f64 = MB * 1024.0;

    let b = bytes as f64;
    if b >= GB {
        format!("{:.2} GB", b / GB)
    } else if b >= MB {
        format!("{:.1} MB", b / MB)
    } else if b >= KB {
        format!("{:.0} KB", b / KB)
    } else {
        format!("{bytes} B")
    }
}
