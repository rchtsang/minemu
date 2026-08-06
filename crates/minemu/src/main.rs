use std::{fs, path::PathBuf, process::ExitCode};

use clap::{Parser, Subcommand};
use minemu::{CliError, RunOptions, package_manifest, run_headless, run_image};

#[derive(Parser)]
#[command(
    name = "minemu",
    about = "Package, boot, and test A32 teaching-platform images.",
    long_about = "Package independently linked A32 kernel and user ELFs into a system image, then boot it headlessly through the same deterministic runtime used by tests.",
    after_help = "Examples:\n  minemu image minimum-template/system/minimum.toml --output build/minimum.img\n  minemu run build/minimum.img --ticks 100000\n  minemu test minimum-template/system/minimum-test.toml"
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Package kernel and user ELFs into a versioned system-ROM image.
    Image {
        /// TOML manifest naming the kernel ELF and optional user modules.
        #[arg(value_name = "MANIFEST")]
        manifest: PathBuf,
        /// Destination path for the versioned system-ROM image.
        #[arg(short, long, value_name = "IMAGE")]
        output: PathBuf,
    },
    /// Boot a system image headlessly for a bounded virtual-time budget.
    Run {
        /// Versioned system-ROM image produced by `minemu image`.
        #[arg(value_name = "IMAGE")]
        image: PathBuf,
        /// Optional host raw-disk file attached as write-back block media.
        #[arg(short = 'm', long)]
        block_media: Option<PathBuf>,
        /// Maximum completed virtual instruction ticks before stopping.
        #[arg(short = 't', long, default_value_t = 100_000)]
        ticks: u64,
    },
    /// Run scheduled UART input and assertions from a TOML test manifest.
    Test {
        /// TOML test manifest naming a system image and expected observations.
        #[arg(value_name = "MANIFEST")]
        manifest: PathBuf,
    },
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
