mod app;

use std::path::PathBuf;
use tracing::error;
use tracing_subscriber::EnvFilter;

fn main() {
    let args: Vec<String> = std::env::args().collect();

    // Modo supervisor headless: usado quando "Close Launcher" está ativo.
    // Monitora o processo do Minecraft sem iniciar a interface Slint.
    if args.len() >= 3 && args[1] == "--supervise" {
        let spec_path = PathBuf::from(&args[2]);
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .expect("Failed to initialize Tokio runtime for supervisor");
        let res = runtime.block_on(
            crab_launcher::core::launch::supervisor::run_supervisor_from_file(&spec_path),
        );
        if res.is_err() {
            std::process::exit(1);
        }
        return;
    }

    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| EnvFilter::new("crab_launcher=info,warn")),
        )
        .init();

    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .expect("Failed to initialize Tokio runtime");

    if let Err(err) = app::run(runtime.handle().clone()) {
        error!("Application error: {err}");
        std::process::exit(1);
    }
}
