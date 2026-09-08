use std::{
    fs,
    num::NonZeroUsize,
    path::{Path, PathBuf},
    thread,
    time::Duration,
};

use minemu_core::{Machine, PhysicalMemory};
use minemu_platform::{
    BOOT_ROM_BASE, BOOT_ROM_SIZE, InspectionRequest, MemRegion, MmuInspection, ObservableEvent,
    PhysicalAddress, PhysicalRange,
};
use minemu_runtime::{
    LifecycleState, RuntimeConfig, RuntimeHandle, RuntimeInspection, RuntimeStatus,
    ScheduledUartInput, UartPort,
};
use serde::Deserialize;

use crate::{CliError, Result};

/// Options for bounded headless execution of a system image.
#[derive(Clone, Debug)]
pub struct RunOptions {
    pub image: PathBuf,
    pub boot_rom: PathBuf,
    pub block_media_path: Option<PathBuf>,
    pub instruction_batch: Option<NonZeroUsize>,
    pub max_ticks: u64,
    pub inputs: Vec<HeadlessInput>,
}

enum RuntimeStart<'a> {
    Paused,
    RunningUntil {
        deadline: u64,
        inputs: &'a [HeadlessInput],
    },
}

/// One UART byte sequence injected once virtual time reaches `at_tick`.
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HeadlessInput {
    pub at_tick: u64,
    pub uart: u8,
    pub data: String,
}

/// One byte pattern written to physical RAM before reset firmware executes.
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RamPrefill {
    pub address: u32,
    pub length: u32,
    pub value: u8,
}

/// Declarative headless test file.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HeadlessTest {
    pub image: PathBuf,
    pub boot_rom: PathBuf,
    pub block_media: Option<PathBuf>,
    pub instruction_batch: Option<NonZeroUsize>,
    #[serde(default = "default_max_ticks")]
    pub max_ticks: u64,
    #[serde(default)]
    pub inputs: Vec<HeadlessInput>,
    #[serde(default)]
    pub ram_prefill: Vec<RamPrefill>,
    pub assert: HeadlessAssertion,
}

/// Supported headless assertions over execution and shutdown snapshots.
#[derive(Clone, Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HeadlessAssertion {
    pub uart0_contains: Option<String>,
    pub uart1_contains: Option<String>,
    pub ticks_at_least: Option<u64>,
    pub execution_lifecycle: Option<ExpectedExecutionLifecycle>,
    pub shutdown_lifecycle: Option<ExpectedShutdownLifecycle>,
    pub mmu_enabled: Option<bool>,
    pub fault_status: Option<u32>,
    pub trace_values: Option<Vec<u32>>,
    #[serde(default)]
    pub block_media: Vec<BlockMediaAssertion>,
}

/// Expected bytes at one offset in the attached block media.
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BlockMediaAssertion {
    pub offset: u64,
    pub bytes: Vec<u8>,
}

/// Lifecycle accepted for the pre-shutdown execution snapshot.
#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ExpectedExecutionLifecycle {
    Paused,
    Stopped,
}

/// Final lifecycle accepted after the headless runner requests shutdown.
#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ExpectedShutdownLifecycle {
    Stopped,
}

/// Result of a bounded run suitable for CLI output or test assertions.
#[derive(Clone, Debug)]
pub struct RunResult {
    pub execution_status: RuntimeStatus,
    pub shutdown_status: RuntimeStatus,
    pub uart0_output: Vec<u8>,
    pub uart1_output: Vec<u8>,
    pub mmu: Option<MmuInspection>,
    pub events: Vec<ObservableEvent>,
}

/// Loads a system image and runs it headlessly for a bounded virtual-time budget.
pub fn run_image(options: RunOptions) -> Result<RunResult> {
    run_image_with_prefill(options, &[])
}

fn run_image_with_prefill(options: RunOptions, ram_prefill: &[RamPrefill]) -> Result<RunResult> {
    let runtime = start_runtime_with_prefill(
        &options.image,
        &options.boot_rom,
        options.block_media_path,
        options.instruction_batch,
        ram_prefill,
        RuntimeStart::RunningUntil {
            deadline: options.max_ticks,
            inputs: &options.inputs,
        },
    )?;

    loop {
        let status = runtime.status();
        if matches!(
            status.lifecycle,
            LifecycleState::Paused | LifecycleState::Stopped | LifecycleState::Failed
        ) {
            break;
        }
        thread::sleep(Duration::from_millis(1));
    }

    if runtime.status().lifecycle == LifecycleState::Running {
        runtime.pause().map_err(|_| CliError::RuntimeSetup)?;
        wait_for(&runtime, LifecycleState::Paused)?;
    }
    let execution_status = runtime.status();
    let (uart0_output, uart1_output, mmu, events) =
        if execution_status.lifecycle == LifecycleState::Paused {
            let RuntimeInspection::Peripherals(peripherals) = runtime
                .inspect(InspectionRequest::Peripherals)
                .map_err(|_| CliError::RuntimeSetup)?
            else {
                unreachable!("peripheral inspection has a fixed response type")
            };
            let RuntimeInspection::Mmu(mmu) = runtime
                .inspect(InspectionRequest::Mmu)
                .map_err(|_| CliError::RuntimeSetup)?
            else {
                unreachable!("MMU inspection has a fixed response type")
            };
            let RuntimeInspection::Events(events) = runtime
                .inspect(InspectionRequest::Events)
                .map_err(|_| CliError::RuntimeSetup)?
            else {
                unreachable!("event inspection has a fixed response type")
            };
            (
                peripherals.uart0.tx_history,
                peripherals.uart1.tx_history,
                Some(mmu),
                events,
            )
        } else {
            (Vec::new(), Vec::new(), None, Vec::new())
        };
    runtime.shutdown().map_err(|_| CliError::RuntimeSetup)?;
    let shutdown_status = runtime.status();
    Ok(RunResult {
        execution_status,
        shutdown_status,
        uart0_output,
        uart1_output,
        mmu,
        events,
    })
}

pub(crate) fn start_runtime(
    image_path: &Path,
    boot_rom_path: &Path,
    block_media_path: Option<PathBuf>,
    instruction_batch: Option<NonZeroUsize>,
) -> Result<RuntimeHandle> {
    start_runtime_with_prefill(
        image_path,
        boot_rom_path,
        block_media_path,
        instruction_batch,
        &[],
        RuntimeStart::Paused,
    )
}

fn start_runtime_with_prefill(
    image_path: &Path,
    boot_rom_path: &Path,
    block_media_path: Option<PathBuf>,
    instruction_batch: Option<NonZeroUsize>,
    ram_prefill: &[RamPrefill],
    start: RuntimeStart<'_>,
) -> Result<RuntimeHandle> {
    let image = minemu_image::SystemImage::parse(&read(image_path)?)?;
    let boot_rom = read(boot_rom_path)?;
    if boot_rom.len() != BOOT_ROM_SIZE as usize {
        return Err(CliError::InvalidBootRomSize {
            path: boot_rom_path.into(),
            expected: BOOT_ROM_SIZE as usize,
            actual: boot_rom.len(),
        });
    }
    let (execution_deadline, inputs, start_paused) = match start {
        RuntimeStart::Paused => (None, &[][..], true),
        RuntimeStart::RunningUntil { deadline, inputs } => (Some(deadline), inputs, false),
    };
    let mut config = runtime_config(
        &boot_rom,
        &image,
        block_media_path,
        instruction_batch,
        ram_prefill,
        execution_deadline,
        inputs,
    )?;
    config.start_paused = start_paused;
    RuntimeHandle::spawn(config).map_err(|_| CliError::RuntimeSetup)
}

/// Loads and executes a declarative headless test manifest.
pub fn run_headless(path: impl AsRef<Path>) -> Result<RunResult> {
    let path = path.as_ref();
    let source = fs::read_to_string(path).map_err(|source| CliError::Read {
        path: path.into(),
        source,
    })?;
    let test: HeadlessTest = toml::from_str(&source).map_err(|source| CliError::Manifest {
        path: path.into(),
        source,
    })?;
    if !test.assert.has_expectations() {
        return Err(CliError::Assertion(
            "headless tests require at least one assertion".into(),
        ));
    }
    let root = path.parent().unwrap_or_else(|| Path::new("."));
    let block_media_path = test.block_media.as_deref().map(|path| resolve(root, path));
    let result = run_image_with_prefill(
        RunOptions {
            image: resolve(root, &test.image),
            boot_rom: resolve(root, &test.boot_rom),
            block_media_path: block_media_path.clone(),
            instruction_batch: test.instruction_batch,
            max_ticks: test.max_ticks,
            inputs: test.inputs,
        },
        &test.ram_prefill,
    )?;
    assert_block_media(block_media_path.as_deref(), &test.assert.block_media)?;
    assert_result(&result, &test.assert)?;
    Ok(result)
}

fn runtime_config(
    boot_rom: &[u8],
    image: &minemu_image::SystemImage,
    block_media_path: Option<PathBuf>,
    instruction_batch: Option<NonZeroUsize>,
    ram_prefill: &[RamPrefill],
    execution_deadline: Option<u64>,
    inputs: &[HeadlessInput],
) -> Result<RuntimeConfig> {
    let memory =
        PhysicalMemory::with_roms(boot_rom, image.bytes()).map_err(|_| CliError::RuntimeSetup)?;
    let mut config = RuntimeConfig::new(Machine::new(memory, 4096), BOOT_ROM_BASE);
    config.block_media_path = block_media_path;
    if let Some(instruction_batch) = instruction_batch {
        config.instruction_batch = instruction_batch.get();
    }
    config.execution_deadline = execution_deadline;
    config.scheduled_uart = inputs
        .iter()
        .map(|input| {
            Ok(ScheduledUartInput {
                at_tick: input.at_tick,
                port: port(input.uart)?,
                bytes: input.data.as_bytes().to_vec(),
            })
        })
        .collect::<Result<_>>()?;
    for fill in ram_prefill {
        let address = PhysicalAddress::new(fill.address);
        let range = PhysicalRange::new(address, fill.length).map_err(|_| {
            CliError::Assertion(format!(
                "RAM prefill at {:#010x} must have a nonzero in-range length",
                fill.address
            ))
        })?;
        if !MemRegion::Ram.range().contains_range(range) {
            return Err(CliError::Assertion(format!(
                "RAM prefill at {:#010x} with length {} is outside physical RAM",
                fill.address, fill.length
            )));
        }
        config
            .initial_ram_writes
            .push((address, vec![fill.value; fill.length as usize]));
    }
    Ok(config)
}

fn assert_result(result: &RunResult, assertion: &HeadlessAssertion) -> Result<()> {
    if result.execution_status.lifecycle == LifecycleState::Failed
        || result.shutdown_status.lifecycle == LifecycleState::Failed
    {
        let detail = result
            .execution_status
            .last_error
            .as_deref()
            .or(result.shutdown_status.last_error.as_deref())
            .unwrap_or("unknown runtime failure");
        return Err(CliError::Assertion(format!(
            "emulator runtime failed: {detail}"
        )));
    }
    if let Some(expected) = &assertion.uart0_contains
        && !String::from_utf8_lossy(&result.uart0_output).contains(expected)
    {
        return Err(CliError::Assertion(
            "UART0 output did not contain expected text".into(),
        ));
    }
    if let Some(expected) = &assertion.uart1_contains
        && !String::from_utf8_lossy(&result.uart1_output).contains(expected)
    {
        return Err(CliError::Assertion(
            "UART1 output did not contain expected text".into(),
        ));
    }
    if let Some(ticks) = assertion.ticks_at_least
        && result.execution_status.machine.ticks < ticks
    {
        return Err(CliError::Assertion(
            "virtual time did not reach the required tick".into(),
        ));
    }
    if let Some(expected) = &assertion.execution_lifecycle
        && !matches!(
            (expected, result.execution_status.lifecycle),
            (ExpectedExecutionLifecycle::Paused, LifecycleState::Paused)
                | (ExpectedExecutionLifecycle::Stopped, LifecycleState::Stopped)
        )
    {
        return Err(CliError::Assertion(
            "unexpected execution lifecycle state".into(),
        ));
    }
    if let Some(expected) = &assertion.shutdown_lifecycle
        && !matches!(
            (expected, result.shutdown_status.lifecycle),
            (ExpectedShutdownLifecycle::Stopped, LifecycleState::Stopped)
        )
    {
        return Err(CliError::Assertion(
            "unexpected shutdown lifecycle state".into(),
        ));
    }
    if let Some(enabled) = assertion.mmu_enabled
        && result.mmu.map(|mmu| mmu.enabled) != Some(enabled)
    {
        return Err(CliError::Assertion("unexpected MMU enabled state".into()));
    }
    if let Some(expected) = assertion.fault_status {
        let actual = result
            .mmu
            .and_then(|mmu| mmu.last_fault_status)
            .map(|status| status.raw());
        if actual != Some(expected) {
            return Err(CliError::Assertion(format!(
                "unexpected fault status: expected {expected:#010x}, got {actual:?}"
            )));
        }
    }
    if let Some(values) = &assertion.trace_values {
        let actual = result
            .events
            .iter()
            .filter_map(|event| match event {
                ObservableEvent::Trace(event) => Some(event.value),
                ObservableEvent::Exception(_) => None,
            })
            .collect::<Vec<_>>();
        if &actual != values {
            return Err(CliError::Assertion(format!(
                "unexpected trace values: expected {values:?}, got {actual:?}"
            )));
        }
    }
    Ok(())
}

impl HeadlessAssertion {
    fn has_expectations(&self) -> bool {
        self.uart0_contains.is_some()
            || self.uart1_contains.is_some()
            || self.ticks_at_least.is_some()
            || self.execution_lifecycle.is_some()
            || self.shutdown_lifecycle.is_some()
            || self.mmu_enabled.is_some()
            || self.fault_status.is_some()
            || self.trace_values.is_some()
            || !self.block_media.is_empty()
    }
}

fn assert_block_media(path: Option<&Path>, assertions: &[BlockMediaAssertion]) -> Result<()> {
    if assertions.is_empty() {
        return Ok(());
    }
    let path = path.ok_or_else(|| {
        CliError::Assertion("block media assertions require an attached block_media path".into())
    })?;
    assert_block_media_contents(&read(path)?, assertions)
}

fn assert_block_media_contents(media: &[u8], assertions: &[BlockMediaAssertion]) -> Result<()> {
    for assertion in assertions {
        let start = usize::try_from(assertion.offset).map_err(|_| {
            CliError::Assertion(format!(
                "block media region at offset {} is out of range for {} bytes of media",
                assertion.offset,
                media.len()
            ))
        })?;
        let end = start.checked_add(assertion.bytes.len()).ok_or_else(|| {
            CliError::Assertion(format!(
                "block media region at offset {} is out of range for {} bytes of media",
                assertion.offset,
                media.len()
            ))
        })?;
        let actual = media.get(start..end).ok_or_else(|| {
            CliError::Assertion(format!(
                "block media region at offset {} with length {} is out of range for {} bytes of media",
                assertion.offset,
                assertion.bytes.len(),
                media.len()
            ))
        })?;
        if actual != assertion.bytes {
            return Err(CliError::Assertion(format!(
                "block media mismatch at offset {}: expected {:?}, got {:?}",
                assertion.offset, assertion.bytes, actual
            )));
        }
    }
    Ok(())
}

fn port(value: u8) -> Result<UartPort> {
    match value {
        0 => Ok(UartPort::Uart0),
        1 => Ok(UartPort::Uart1),
        _ => Err(CliError::Assertion("UART input must target 0 or 1".into())),
    }
}

fn wait_for(runtime: &RuntimeHandle, lifecycle: LifecycleState) -> Result<()> {
    for _ in 0..500 {
        if runtime.status().lifecycle == lifecycle {
            return Ok(());
        }
        thread::sleep(Duration::from_millis(2));
    }
    Err(CliError::RuntimeSetup)
}

fn resolve(root: &Path, path: &Path) -> PathBuf {
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        root.join(path)
    }
}

fn read(path: &Path) -> Result<Vec<u8>> {
    fs::read(path).map_err(|source| CliError::Read {
        path: path.into(),
        source,
    })
}

const fn default_max_ticks() -> u64 {
    100_000
}

#[cfg(test)]
mod tests {
    use minemu_core::MachineStatus;
    use minemu_image::SystemImage;
    use minemu_platform::{
        BOOT_INFO_PADDR, BOOT_ROM_BASE, BOOT_ROM_SIZE, BOOTSTRAP_ENTRY_PADDR, IMAGE_HEADER_SIZE,
        ImageHeader, KERNEL_SEGMENT_SIZE, KernelSegment, PhysicalAddress, PhysicalRange,
        VirtualAddress,
    };

    use super::{
        BlockMediaAssertion, HeadlessAssertion, HeadlessTest, RamPrefill, RunResult,
        assert_block_media_contents, assert_result, runtime_config,
    };
    use crate::CliError;
    use minemu_runtime::{
        LifecycleState, RuntimeHandle, RuntimeInspection, RuntimeInspectionRequest,
    };

    fn result(output: &[u8]) -> RunResult {
        let status = |lifecycle| minemu_runtime::RuntimeStatus {
            lifecycle,
            machine: MachineStatus {
                ticks: 7,
                mmu_enabled: false,
                pending_interrupts: 0,
                systick_status: 0,
                block_status: 0,
                uart0_status: 0,
                uart1_status: 0,
            },
            last_stop: None,
            last_error: None,
        };
        RunResult {
            execution_status: status(LifecycleState::Paused),
            shutdown_status: status(LifecycleState::Stopped),
            uart0_output: output.to_vec(),
            uart1_output: Vec::new(),
            mmu: None,
            events: Vec::new(),
        }
    }

    #[test]
    fn assertions_cover_console_and_virtual_time() {
        assert_result(
            &result(b"ready"),
            &HeadlessAssertion {
                uart0_contains: Some("ready".into()),
                ticks_at_least: Some(7),
                ..Default::default()
            },
        )
        .unwrap();
        assert!(matches!(
            assert_result(
                &result(b"ready"),
                &HeadlessAssertion {
                    uart0_contains: Some("missing".into()),
                    ..Default::default()
                },
            ),
            Err(CliError::Assertion(_))
        ));
    }

    #[test]
    fn assertions_distinguish_lifecycle_snapshots_and_fail_runtime_errors() {
        let assertion: HeadlessAssertion =
            toml::from_str("execution_lifecycle = 'paused'\nshutdown_lifecycle = 'stopped'\n")
                .unwrap();
        assert_result(&result(b""), &assertion).unwrap();

        let mut failed = result(b"");
        failed.execution_status.lifecycle = LifecycleState::Failed;
        failed.execution_status.last_error = Some("backend stopped".into());
        assert!(matches!(
            assert_result(&failed, &assertion),
            Err(CliError::Assertion(message)) if message.contains("backend stopped")
        ));
    }

    #[test]
    fn block_media_region_assertions_cover_success_out_of_range_and_mismatch() {
        let expected = [BlockMediaAssertion {
            offset: 2,
            bytes: vec![0x22, 0x33],
        }];
        assert_block_media_contents(&[0x00, 0x11, 0x22, 0x33], &expected).unwrap();

        let out_of_range = [BlockMediaAssertion {
            offset: 3,
            bytes: vec![0x33, 0x44],
        }];
        assert!(matches!(
            assert_block_media_contents(&[0x00, 0x11, 0x22, 0x33], &out_of_range),
            Err(CliError::Assertion(message)) if message.contains("out of range")
        ));

        let mismatch = [BlockMediaAssertion {
            offset: 2,
            bytes: vec![0xaa, 0xbb],
        }];
        assert!(matches!(
            assert_block_media_contents(&[0x00, 0x11, 0x22, 0x33], &mismatch),
            Err(CliError::Assertion(message)) if message.contains("mismatch")
        ));
    }

    #[test]
    fn headless_manifest_accepts_media_regions_and_positive_instruction_batch() {
        let test: HeadlessTest = toml::from_str(
            r#"
image = "system.img"
boot_rom = "boot.bin"
block_media = "working-disk.img"
instruction_batch = 1

[[ram_prefill]]
address = 0x40030000
length = 128
value = 165

[[assert.block_media]]
offset = 512
bytes = [0xde, 0xad, 0xbe, 0xef]
"#,
        )
        .unwrap();

        assert_eq!(
            test.block_media.unwrap(),
            std::path::PathBuf::from("working-disk.img")
        );
        assert_eq!(test.instruction_batch.unwrap().get(), 1);
        assert_eq!(test.ram_prefill.len(), 1);
        assert_eq!(test.ram_prefill[0].value, 165);
        assert_eq!(test.assert.block_media.len(), 1);
        assert_eq!(test.assert.block_media[0].offset, 512);
        assert!(test.assert.has_expectations());

        assert!(
            toml::from_str::<HeadlessTest>(
                r#"
image = "system.img"
boot_rom = "boot.bin"
instruction_batch = 0
"#,
            )
            .is_err()
        );
        let empty: HeadlessTest =
            toml::from_str("image = 'system.img'\nboot_rom = 'boot.bin'\n[assert]\n").unwrap();
        assert!(!empty.assert.has_expectations());
        assert!(
            toml::from_str::<HeadlessTest>(
                "image = 'system.img'\nboot_rom = 'boot.bin'\n[[inputs]]\nat_tick = 1\nuart = 0\ndata = 'x'\nwhen = 'late'\n[assert]\nticks_at_least = 1\n"
            )
            .is_err()
        );
        assert!(
            toml::from_str::<HeadlessTest>(
                r#"
image = "system.img"
boot_rom = "boot.bin"

[assert]
block_media_bytes = []
"#,
            )
            .is_err()
        );
    }

    #[test]
    fn runtime_config_applies_instruction_batch_override() {
        let config = runtime_config(
            &test_boot_rom(),
            &boot_test_image(),
            None,
            std::num::NonZeroUsize::new(7),
            &[RamPrefill {
                address: 0x4003_0000,
                length: 4,
                value: 0xa5,
            }],
            Some(99),
            &[],
        )
        .unwrap();

        assert_eq!(config.instruction_batch, 7);
        assert_eq!(config.initial_ram_writes.len(), 1);
        assert_eq!(config.initial_ram_writes[0].1, [0xa5; 4]);
        assert_eq!(config.execution_deadline, Some(99));
    }

    #[test]
    fn image_reset_returns_to_boot_rom_with_clear_ram() {
        let boot_rom = test_boot_rom();
        let config =
            runtime_config(&boot_rom, &boot_test_image(), None, None, &[], None, &[]).unwrap();
        assert_eq!(config.entry, BOOT_ROM_BASE);
        assert_eq!(config.instruction_batch, 1024);
        assert!(config.initial_ram_writes.is_empty());
        let runtime = RuntimeHandle::spawn(config).unwrap();
        runtime.pause().unwrap();
        super::wait_for(&runtime, LifecycleState::Paused).unwrap();
        runtime.reset().unwrap();

        let RuntimeInspection::Execution(execution) = runtime
            .request_inspection(RuntimeInspectionRequest::Execution {
                address: None,
                before: 0,
                after: 4,
            })
            .unwrap()
            .recv()
            .unwrap()
            .unwrap()
        else {
            panic!("execution request has a fixed response type");
        };
        assert_eq!(execution.registers[15], BOOT_ROM_BASE);

        let RuntimeInspection::LiveMemory(_, bytes) = runtime
            .request_inspection(RuntimeInspectionRequest::LiveMemory(
                PhysicalRange::new(PhysicalAddress::new(BOOT_ROM_BASE), 4).unwrap(),
            ))
            .unwrap()
            .recv()
            .unwrap()
            .unwrap()
        else {
            panic!("live-memory request has a fixed response type");
        };
        assert_eq!(bytes, [0xfe, 0xff, 0xff, 0xea]);

        let RuntimeInspection::LiveMemory(_, bytes) = runtime
            .request_inspection(RuntimeInspectionRequest::LiveMemory(
                PhysicalRange::new(PhysicalAddress::new(BOOTSTRAP_ENTRY_PADDR), 8).unwrap(),
            ))
            .unwrap()
            .recv()
            .unwrap()
            .unwrap()
        else {
            panic!("live-memory request has a fixed response type");
        };
        assert_eq!(bytes, [0; 8]);
        runtime.shutdown().unwrap();
    }

    fn test_boot_rom() -> Vec<u8> {
        let mut bytes = vec![0; BOOT_ROM_SIZE as usize];
        bytes[..4].copy_from_slice(&[0xfe, 0xff, 0xff, 0xea]); // b .
        bytes
    }

    fn boot_test_image() -> SystemImage {
        const BOOTSTRAP: [u8; 4] = [0xfe, 0xff, 0xff, 0xea]; // b .
        const HIGH_ENTRY: [u8; 4] = [0xfe, 0xff, 0xff, 0xea]; // b .
        let table_size = 2 * KERNEL_SEGMENT_SIZE;
        let data_offset = IMAGE_HEADER_SIZE + table_size;
        let image_size = data_offset + BOOTSTRAP.len() + HIGH_ENTRY.len();
        let header = ImageHeader {
            image_size: image_size as u32,
            kernel_segment_table_offset: IMAGE_HEADER_SIZE as u32,
            kernel_segment_count: 2,
            module_table_offset: data_offset as u32,
            module_count: 0,
            bootstrap_entry_paddr: PhysicalAddress::new(BOOTSTRAP_ENTRY_PADDR),
            kernel_entry_vaddr: VirtualAddress::new(0xc000_9000),
            boot_info_paddr: PhysicalAddress::new(BOOT_INFO_PADDR),
        };
        let segments = [
            KernelSegment {
                data_offset: data_offset as u32,
                physical_address: PhysicalAddress::new(BOOTSTRAP_ENTRY_PADDR),
                virtual_address: VirtualAddress::new(BOOTSTRAP_ENTRY_PADDR),
                file_size: BOOTSTRAP.len() as u32,
                memory_size: 8,
                flags: 0x5,
            },
            KernelSegment {
                data_offset: (data_offset + BOOTSTRAP.len()) as u32,
                physical_address: PhysicalAddress::new(0x4000_9000),
                virtual_address: VirtualAddress::new(0xc000_9000),
                file_size: HIGH_ENTRY.len() as u32,
                memory_size: HIGH_ENTRY.len() as u32,
                flags: 0x5,
            },
        ];
        let mut bytes = vec![0; image_size];
        bytes[..IMAGE_HEADER_SIZE].copy_from_slice(&header.encode().unwrap());
        for (index, segment) in segments.into_iter().enumerate() {
            let offset = IMAGE_HEADER_SIZE + index * KERNEL_SEGMENT_SIZE;
            bytes[offset..offset + KERNEL_SEGMENT_SIZE].copy_from_slice(&segment.encode().unwrap());
        }
        bytes[data_offset..data_offset + BOOTSTRAP.len()].copy_from_slice(&BOOTSTRAP);
        bytes[data_offset + BOOTSTRAP.len()..].copy_from_slice(&HIGH_ENTRY);
        SystemImage::parse(&bytes).unwrap()
    }
}
