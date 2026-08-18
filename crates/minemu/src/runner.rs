use std::{
    fs,
    path::{Path, PathBuf},
    thread,
    time::Duration,
};

use minemu_core::{Machine, PhysicalMemory};
use minemu_platform::{
    BOOT_ROM_BASE, BOOT_ROM_SIZE, InspectionRequest, MmuInspection, ObservableEvent,
};
use minemu_runtime::{
    LifecycleState, RuntimeConfig, RuntimeHandle, RuntimeInspection, RuntimeStatus, UartPort,
};
use serde::Deserialize;

use crate::{CliError, Result};

/// Options for bounded headless execution of a system image.
#[derive(Clone, Debug)]
pub struct RunOptions {
    pub image: PathBuf,
    pub boot_rom: PathBuf,
    pub block_media_path: Option<PathBuf>,
    pub max_ticks: u64,
    pub inputs: Vec<HeadlessInput>,
}

/// One UART byte sequence injected once virtual time reaches `at_tick`.
#[derive(Clone, Debug, Deserialize)]
pub struct HeadlessInput {
    pub at_tick: u64,
    pub uart: u8,
    pub data: String,
}

/// Declarative headless test file.
#[derive(Debug, Deserialize)]
pub struct HeadlessTest {
    pub image: PathBuf,
    pub boot_rom: PathBuf,
    #[serde(default = "default_max_ticks")]
    pub max_ticks: u64,
    #[serde(default)]
    pub inputs: Vec<HeadlessInput>,
    #[serde(default)]
    pub assert: HeadlessAssertion,
}

/// Supported headless assertions over final runtime state.
#[derive(Clone, Debug, Default, Deserialize)]
pub struct HeadlessAssertion {
    pub uart0_contains: Option<String>,
    pub uart1_contains: Option<String>,
    pub ticks_at_least: Option<u64>,
    pub lifecycle: Option<ExpectedLifecycle>,
    pub mmu_enabled: Option<bool>,
    pub fault_status: Option<u32>,
    pub trace_values: Option<Vec<u32>>,
}

/// Lifecycle name accepted by the headless test manifest.
#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ExpectedLifecycle {
    Running,
    Stopped,
    Failed,
}

/// Result of a bounded run suitable for CLI output or test assertions.
#[derive(Clone, Debug)]
pub struct RunResult {
    pub status: RuntimeStatus,
    pub uart0_output: Vec<u8>,
    pub uart1_output: Vec<u8>,
    pub mmu: Option<MmuInspection>,
    pub events: Vec<ObservableEvent>,
}

/// Loads a system image and runs it headlessly for a bounded virtual-time budget.
pub fn run_image(options: RunOptions) -> Result<RunResult> {
    let runtime = start_runtime(&options.image, &options.boot_rom, options.block_media_path)?;
    let mut inputs = options.inputs;
    inputs.sort_by_key(|input| input.at_tick);
    let mut next_input = 0;

    loop {
        let status = runtime.status();
        while next_input < inputs.len() && status.machine.ticks >= inputs[next_input].at_tick {
            let input = &inputs[next_input];
            runtime.send_uart(port(input.uart)?, input.data.as_bytes());
            next_input += 1;
        }
        if status.machine.ticks >= options.max_ticks
            || matches!(
                status.lifecycle,
                LifecycleState::Stopped | LifecycleState::Failed
            )
        {
            break;
        }
        thread::sleep(Duration::from_millis(1));
    }

    let status = runtime.status();
    let (uart0_output, uart1_output, mmu, events) = if status.lifecycle == LifecycleState::Running {
        runtime.pause().map_err(|_| CliError::RuntimeSetup)?;
        wait_for(&runtime, LifecycleState::Paused)?;
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
    Ok(RunResult {
        status: runtime.status(),
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
    let config = runtime_config(&boot_rom, &image, block_media_path)?;
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
    let root = path.parent().unwrap_or_else(|| Path::new("."));
    let result = run_image(RunOptions {
        image: resolve(root, &test.image),
        boot_rom: resolve(root, &test.boot_rom),
        block_media_path: None,
        max_ticks: test.max_ticks,
        inputs: test.inputs,
    })?;
    assert_result(&result, &test.assert)?;
    Ok(result)
}

fn runtime_config(
    boot_rom: &[u8],
    image: &minemu_image::SystemImage,
    block_media_path: Option<PathBuf>,
) -> Result<RuntimeConfig> {
    let memory =
        PhysicalMemory::with_roms(boot_rom, image.bytes()).map_err(|_| CliError::RuntimeSetup)?;
    let mut config = RuntimeConfig::new(Machine::new(memory, 4096), BOOT_ROM_BASE);
    config.block_media_path = block_media_path;
    Ok(config)
}

fn assert_result(result: &RunResult, assertion: &HeadlessAssertion) -> Result<()> {
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
        && result.status.machine.ticks < ticks
    {
        return Err(CliError::Assertion(
            "virtual time did not reach the required tick".into(),
        ));
    }
    if let Some(expected) = &assertion.lifecycle
        && !matches!(
            (expected, result.status.lifecycle),
            (ExpectedLifecycle::Running, LifecycleState::Running)
                | (ExpectedLifecycle::Stopped, LifecycleState::Stopped)
                | (ExpectedLifecycle::Failed, LifecycleState::Failed)
        )
    {
        return Err(CliError::Assertion(
            "unexpected final lifecycle state".into(),
        ));
    }
    if let Some(enabled) = assertion.mmu_enabled
        && result.mmu.map(|mmu| mmu.enabled) != Some(enabled)
    {
        return Err(CliError::Assertion("unexpected MMU enabled state".into()));
    }
    if let Some(status) = assertion.fault_status
        && result
            .mmu
            .and_then(|mmu| mmu.last_fault_status)
            .map(|status| status.raw())
            != Some(status)
    {
        return Err(CliError::Assertion("unexpected fault status".into()));
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

    use super::{HeadlessAssertion, RunResult, assert_result, runtime_config};
    use crate::CliError;
    use minemu_runtime::{
        LifecycleState, RuntimeHandle, RuntimeInspection, RuntimeInspectionRequest,
    };

    fn result(output: &[u8]) -> RunResult {
        RunResult {
            status: minemu_runtime::RuntimeStatus {
                lifecycle: LifecycleState::Stopped,
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
            },
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
    fn image_reset_returns_to_boot_rom_with_clear_ram() {
        let boot_rom = test_boot_rom();
        let config = runtime_config(&boot_rom, &boot_test_image(), None).unwrap();
        assert_eq!(config.entry, BOOT_ROM_BASE);
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
