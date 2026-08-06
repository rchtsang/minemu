use std::{fs, path::PathBuf, process::ExitCode};

use clap::{Parser, Subcommand};
use minemu::{CliError, RunOptions, package_manifest, run_headless, run_image};

#[derive(Parser)]
#[command(
    name = "minemu",
    about = "A32 teaching-platform image and headless runner"
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Package a system ROM image from a TOML manifest.
    Image {
        manifest: PathBuf,
        #[arg(short, long)]
        output: PathBuf,
    },
    /// Boot and run a system image headlessly for a bounded virtual-time budget.
    Run {
        image: PathBuf,
        #[arg(short = 'm', long)]
        block_media: Option<PathBuf>,
        #[arg(short = 't', long, default_value_t = 100_000)]
        ticks: u64,
    },
    /// Run a TOML headless test manifest with scheduled input and assertions.
    Test { manifest: PathBuf },
}

fn main() -> ExitCode {
    match execute(Cli::parse().command) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("minemu: {error}");
            ExitCode::FAILURE
        }
    }
}

fn execute(command: Command) -> minemu::Result<()> {
    match command {
        Command::Image { manifest, output } => {
            let image = package_manifest(manifest)?;
            fs::write(&output, image.bytes()).map_err(|source| CliError::Write {
                path: output,
                source,
            })
        }
        Command::Run {
            image,
            block_media,
            ticks,
        } => {
            let result = run_image(RunOptions {
                image,
                block_media_path: block_media,
                max_ticks: ticks,
                inputs: Vec::new(),
            })?;
            print!("{}", String::from_utf8_lossy(&result.uart0_output));
            eprint!("{}", String::from_utf8_lossy(&result.uart1_output));
            if result.status.lifecycle == minemu_runtime::LifecycleState::Failed {
                return Err(CliError::Assertion(
                    "emulator terminated with a runtime failure".into(),
                ));
            }
            Ok(())
        }
        Command::Test { manifest } => {
            run_headless(manifest)?;
            Ok(())
        }
    }
}
