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
    version,
    about = "Package, boot, and test A32 teaching-platform images.",
    long_about = "Package independently linked A32 kernel and user ELFs into a system image, boot it in an interactive terminal UI, or run it headlessly through the deterministic test runtime.",
    after_help = "Examples:\n  minemu image minimum-template/image/minimum.toml --output minimum-template/image/build/minimum.img\n  minemu run minimum-template/image/build/minimum.img --boot-rom minimum-template/bootloader/bootloader.bin\n  minemu test minimum-tests/image/minimum-test.toml"
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
    /// Boot a system image in the terminal UI, or run it headlessly.
    Run {
        /// Versioned system-ROM image produced by `minemu image`.
        #[arg(value_name = "IMAGE")]
        image: PathBuf,
        /// Raw 64-KiB platform firmware mapped at the reset vector.
        #[arg(long, value_name = "BOOT_ROM")]
        boot_rom: PathBuf,
        /// Legacy unit-0 host raw-disk attachment.
        #[arg(short = 'm', long, conflicts_with = "block0_media")]
        block_media: Option<PathBuf>,
        /// Optional host raw-disk file explicitly attached as block unit 0.
        #[arg(long)]
        block0_media: Option<PathBuf>,
        /// Optional host raw-disk file attached as block unit 1.
        #[arg(long)]
        block1_media: Option<PathBuf>,
        /// Headless virtual-time deadline (default: 100000).
        #[arg(short = 't', long, requires = "headless")]
        ticks: Option<u64>,
        /// Run without the terminal UI and stop at the tick deadline.
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
            block0_media,
            block1_media,
            ticks,
            headless,
        } => {
            let block0_media = block0_media.or(block_media);
            if !headless {
                return run_tui(image, boot_rom, block0_media, block1_media);
            }
            let result = run_image(RunOptions {
                image,
                boot_rom,
                block_media_path: block0_media,
                block1_media_path: block1_media,
                instruction_batch: None,
                max_ticks: ticks.unwrap_or(100_000),
                inputs: Vec::new(),
            })?;
            print!("{}", String::from_utf8_lossy(&result.uart0_output));
            eprint!("{}", String::from_utf8_lossy(&result.uart1_output));
            if result.execution_status.lifecycle == minemu_runtime::LifecycleState::Failed
                || result.shutdown_status.lifecycle == minemu_runtime::LifecycleState::Failed
            {
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

#[cfg(test)]
mod tests {
    use clap::{CommandFactory, Parser};

    use super::{Cli, Command};

    #[test]
    fn generated_help_describes_tui_first_run_mode() {
        let help = Cli::command().render_long_help().to_string();
        assert!(help.contains("boot it in an interactive terminal UI"));

        let run = Cli::command()
            .find_subcommand("run")
            .expect("run subcommand")
            .clone()
            .render_long_help()
            .to_string();
        assert!(run.contains("Boot a system image in the terminal UI"));
        assert!(run.contains("Headless virtual-time deadline"));
    }

    #[test]
    fn ticks_requires_explicit_headless_mode() {
        assert!(
            Cli::try_parse_from([
                "minemu",
                "run",
                "image.bin",
                "--boot-rom",
                "boot.bin",
                "--ticks",
                "1",
            ])
            .is_err()
        );
        assert!(
            Cli::try_parse_from([
                "minemu",
                "run",
                "image.bin",
                "--boot-rom",
                "boot.bin",
                "--headless",
                "--ticks",
                "1",
            ])
            .is_ok()
        );
    }

    #[test]
    fn block_media_flags_preserve_unit_zero_alias_and_allow_unit_one() {
        let cli = Cli::try_parse_from([
            "minemu",
            "run",
            "image.bin",
            "--boot-rom",
            "boot.bin",
            "--block-media",
            "disk.img",
            "--block1-media",
            "swap.img",
        ])
        .unwrap();
        let Command::Run {
            block_media,
            block0_media,
            block1_media,
            ..
        } = cli.command
        else {
            panic!("expected run command");
        };
        assert_eq!(block_media.unwrap(), std::path::PathBuf::from("disk.img"));
        assert!(block0_media.is_none());
        assert_eq!(block1_media.unwrap(), std::path::PathBuf::from("swap.img"));

        assert!(
            Cli::try_parse_from([
                "minemu",
                "run",
                "image.bin",
                "--boot-rom",
                "boot.bin",
                "--block-media",
                "disk.img",
                "--block0-media",
                "other.img",
            ])
            .is_err()
        );
    }
}
