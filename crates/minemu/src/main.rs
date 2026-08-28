use std::{
    fs,
    path::{Path, PathBuf},
    process::ExitCode,
};

use clap::{Parser, Subcommand};
use minemu::{CliError, RunOptions, package_manifest, run_headless, run_image, run_tui};
use tracing_subscriber::EnvFilter;

#[derive(Parser)]
#[command(
    name = "minemu",
    about = "Package, boot, and test A32 teaching-platform images.",
    long_about = "Package independently linked A32 kernel and user ELFs into a system image, then boot it headlessly through the same deterministic runtime used by tests.",
    after_help = "Examples:\n  minemu image minimum-template/image/minimum.toml --output minimum-template/image/build/minimum.img\n  minemu run minimum-template/image/build/minimum.img --boot-rom minimum-template/bootloader/bootloader.bin\n  minemu test minimum-template/image/minimum-test.toml"
)]
struct Cli {
    /// Write structured diagnostics to a file instead of the terminal.
    #[arg(long, global = true, value_name = "PATH")]
    log_file: Option<PathBuf>,
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
        /// Raw 64-KiB platform firmware mapped at the reset vector.
        #[arg(long, value_name = "BOOT_ROM")]
        boot_rom: PathBuf,
        /// Optional host raw-disk file attached as write-back block media.
        #[arg(short = 'm', long)]
        block_media: Option<PathBuf>,
        /// Maximum completed virtual instruction ticks before stopping.
        #[arg(short = 't', long, default_value_t = 100_000)]
        ticks: u64,
        /// Run without the terminal UI and stop after `--ticks`.
        #[arg(long)]
        headless: bool,
    },
    /// Run scheduled UART input and assertions from a TOML test manifest.
    Test {
        /// TOML test manifest naming a system image and expected observations.
        #[arg(value_name = "MANIFEST")]
        manifest: PathBuf,
    },
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    match init_logging(cli.log_file.as_deref()).and_then(|()| execute(cli.command)) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("minemu: {error}");
            ExitCode::FAILURE
        }
    }
}

fn init_logging(path: Option<&Path>) -> minemu::Result<()> {
    let Some(path) = path else {
        return Ok(());
    };
    let file = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .map_err(|source| CliError::LogFile {
            path: path.into(),
            source,
        })?;
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("warn"));
    tracing_subscriber::fmt()
        .with_ansi(false)
        .with_env_filter(filter)
        .with_writer(file)
        .try_init()
        .map_err(|error| CliError::Tracing(error.to_string()))
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
            boot_rom,
            block_media,
            ticks,
            headless,
        } => {
            if !headless {
                return run_tui(image, boot_rom, block_media);
            }
            let result = run_image(RunOptions {
                image,
                boot_rom,
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
