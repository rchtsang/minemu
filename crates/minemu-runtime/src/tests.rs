use std::{num::NonZeroU64, thread, time::Duration};

use minemu_core::{Machine, PhysicalMemoryAccess};
use minemu_platform::{InspectionRequest, MemRegion, PhysicalAddress, PhysicalRange};

use crate::{
    LifecycleState, RuntimeConfig, RuntimeHandle, RuntimeInspection, RuntimeInspectionRequest,
    RuntimeStatus, ScheduledUartInput, UartPort,
};

fn running_runtime() -> RuntimeHandle {
    let mut machine = Machine::default();
    let entry = MemRegion::Ram.base().get();
    let instruction = [0xfe, 0xff, 0xff, 0xea]; // b .
    machine
        .memory
        .write_range(PhysicalAddress::new(entry), &instruction)
        .unwrap();
    RuntimeHandle::spawn(
        RuntimeConfig::new(machine, entry)
            .with_initial_ram_write(PhysicalAddress::new(entry), instruction.to_vec()),
    )
    .unwrap()
}

/// Polls the published status until `done` holds, panicking after about one second.
fn wait_until(
    runtime: &RuntimeHandle,
    what: &str,
    done: impl Fn(&RuntimeStatus) -> bool,
) -> RuntimeStatus {
    for _ in 0..500 {
        let status = runtime.status();
        if done(&status) {
            return status;
        }
        thread::sleep(Duration::from_millis(2));
    }
    let status = runtime.status();
    panic!(
        "runtime did not reach {what}; lifecycle {:?}, tick {}",
        status.lifecycle, status.machine.ticks
    );
}

fn wait_for(runtime: &RuntimeHandle, state: LifecycleState) {
    wait_until(runtime, &format!("{state:?}"), |status| {
        status.lifecycle == state
    });
}

fn wait_for_paused_tick(runtime: &RuntimeHandle, tick: u64) -> RuntimeStatus {
    wait_until(runtime, &format!("a pause at tick {tick}"), |status| {
        status.lifecycle == LifecycleState::Paused && status.machine.ticks == tick
    })
}

#[test]
fn lifecycle_commands_run_on_the_emulator_thread() {
    let runtime = running_runtime();
    wait_for(&runtime, LifecycleState::Running);
    runtime.pause().unwrap();
    wait_for(&runtime, LifecycleState::Paused);
    runtime.reset().unwrap();
    runtime.resume().unwrap();
    wait_for(&runtime, LifecycleState::Running);
    runtime.shutdown().unwrap();
    assert_eq!(runtime.status().lifecycle, LifecycleState::Stopped);
}

#[test]
fn configured_runtime_starts_paused_before_execution() {
    let mut machine = Machine::default();
    let entry = MemRegion::Ram.base().get();
    machine
        .memory
        .write_range(PhysicalAddress::new(entry), &[0xfe, 0xff, 0xff, 0xea])
        .unwrap();
    let mut config = RuntimeConfig::new(machine, entry);
    config.start_paused = true;
    let runtime = RuntimeHandle::spawn(config).unwrap();

    wait_for(&runtime, LifecycleState::Paused);
    assert_eq!(runtime.status().machine.ticks, 0);
    runtime.shutdown().unwrap();
}

#[test]
fn bounded_resume_executes_exact_instruction_count_and_pauses() {
    let runtime = running_runtime();
    wait_for(&runtime, LifecycleState::Running);
    runtime.pause().unwrap();
    wait_for(&runtime, LifecycleState::Paused);
    let initial_ticks = runtime.status().machine.ticks;

    runtime.resume_for(NonZeroU64::new(7).unwrap()).unwrap();
    let status = wait_for_paused_tick(&runtime, initial_ticks + 7);
    assert!(
        status
            .last_stop
            .as_deref()
            .is_some_and(|stop| stop.contains("instruction limit reached"))
    );
    runtime.shutdown().unwrap();
}

#[test]
fn breakpoints_pause_at_the_instruction_and_resume_past_it_once() {
    let mut machine = Machine::default();
    let entry = MemRegion::Ram.base().get();
    let instructions = [
        0x71, 0x00, 0x20, 0xe1, // bkpt #1
        0x72, 0x00, 0x20, 0xe1, // bkpt #2
        0x07, 0x00, 0xa0, 0xe3, // mov r0, #7
        0xfe, 0xff, 0xff, 0xea, // b .
    ];
    machine
        .memory
        .write_range(PhysicalAddress::new(entry), &instructions)
        .unwrap();
    let runtime = RuntimeHandle::spawn(
        RuntimeConfig::new(machine, entry)
            .with_initial_ram_write(PhysicalAddress::new(entry), instructions.to_vec()),
    )
    .unwrap();

    wait_for_paused_tick(&runtime, 1);
    let first_stop = format!("breakpoint #0x0001 at 0x{entry:08x}");
    assert_eq!(
        runtime.status().last_stop.as_deref(),
        Some(first_stop.as_str())
    );
    let RuntimeInspection::Execution(first) = runtime
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
        panic!("execution inspection has a fixed response type");
    };
    assert_eq!(first.registers[15], entry);
    assert_eq!(first.instruction_bytes, instructions[..4]);

    runtime.resume_for(NonZeroU64::new(1).unwrap()).unwrap();
    wait_for_paused_tick(&runtime, 2);
    let second_stop = format!("breakpoint #0x0002 at 0x{:08x}", entry + 4);
    assert_eq!(
        runtime.status().last_stop.as_deref(),
        Some(second_stop.as_str())
    );

    runtime.resume_for(NonZeroU64::new(1).unwrap()).unwrap();
    wait_for_paused_tick(&runtime, 3);
    assert!(
        runtime
            .status()
            .last_stop
            .as_deref()
            .is_some_and(|stop| stop.contains("instruction limit reached"))
    );
    let RuntimeInspection::Execution(after_resume) = runtime
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
        panic!("execution inspection has a fixed response type");
    };
    assert_eq!(after_resume.registers[0], 7);
    assert_eq!(after_resume.registers[15], entry + 12);
    runtime.shutdown().unwrap();
}

#[test]
fn reset_clears_a_pending_breakpoint_resume() {
    let mut machine = Machine::default();
    let entry = MemRegion::Ram.base().get();
    let instructions = [
        0x70, 0x00, 0x20, 0xe1, // bkpt #0
        0xfe, 0xff, 0xff, 0xea, // b .
    ];
    machine
        .memory
        .write_range(PhysicalAddress::new(entry), &instructions)
        .unwrap();
    let runtime = RuntimeHandle::spawn(
        RuntimeConfig::new(machine, entry)
            .with_initial_ram_write(PhysicalAddress::new(entry), instructions.to_vec()),
    )
    .unwrap();

    wait_for_paused_tick(&runtime, 1);
    runtime.reset().unwrap();
    wait_until(&runtime, "a cleared pause at tick 0", |status| {
        status.lifecycle == LifecycleState::Paused
            && status.machine.ticks == 0
            && status.last_stop.is_none()
    });

    runtime.resume().unwrap();
    wait_for_paused_tick(&runtime, 1);
    assert!(
        runtime
            .status()
            .last_stop
            .as_deref()
            .is_some_and(|stop| stop.starts_with("breakpoint #0x0000"))
    );
    runtime.shutdown().unwrap();
}

#[test]
fn scheduled_run_stops_and_delivers_uart_at_emulator_thread_boundaries() {
    let mut machine = Machine::default();
    let entry = MemRegion::Ram.base().get();
    let instruction = [0xfe, 0xff, 0xff, 0xea]; // b .
    machine
        .memory
        .write_range(PhysicalAddress::new(entry), &instruction)
        .unwrap();
    let mut config = RuntimeConfig::new(machine, entry)
        .with_initial_ram_write(PhysicalAddress::new(entry), instruction.to_vec());
    config.instruction_batch = 1024;
    config.execution_deadline = Some(7);
    config.scheduled_uart.push(ScheduledUartInput {
        at_tick: 3,
        port: UartPort::Uart1,
        bytes: b"scheduled".to_vec(),
    });
    let runtime = RuntimeHandle::spawn(config).unwrap();

    wait_for(&runtime, LifecycleState::Paused);
    let status = runtime.status();
    assert_eq!(status.machine.ticks, 7);
    assert_eq!(
        status.last_stop.as_deref(),
        Some("execution deadline reached")
    );
    let RuntimeInspection::Peripherals(peripherals) =
        runtime.inspect(InspectionRequest::Peripherals).unwrap()
    else {
        panic!("peripheral inspection has a fixed response type");
    };
    assert_eq!(peripherals.uart1.rx_queued, b"scheduled".len());
    runtime.shutdown().unwrap();
}

#[test]
fn execution_deadline_fires_once_and_rearms_on_reset() {
    let mut machine = Machine::default();
    let entry = MemRegion::Ram.base().get();
    let instruction = [0xfe, 0xff, 0xff, 0xea]; // b .
    machine
        .memory
        .write_range(PhysicalAddress::new(entry), &instruction)
        .unwrap();
    let mut config = RuntimeConfig::new(machine, entry)
        .with_initial_ram_write(PhysicalAddress::new(entry), instruction.to_vec());
    config.execution_deadline = Some(5);
    let runtime = RuntimeHandle::spawn(config).unwrap();
    wait_for_paused_tick(&runtime, 5);

    // Resuming after the deadline runs past it instead of re-pausing immediately.
    runtime.resume_for(NonZeroU64::new(3).unwrap()).unwrap();
    wait_for_paused_tick(&runtime, 8);

    // Reset restarts virtual time at zero, so the deadline applies again.
    runtime.reset().unwrap();
    runtime.resume().unwrap();
    let status = wait_for_paused_tick(&runtime, 5);
    assert_eq!(
        status.last_stop.as_deref(),
        Some("execution deadline reached")
    );
    runtime.shutdown().unwrap();
}

#[test]
fn scheduled_boundaries_split_multi_tick_exception_entry() {
    let mut machine = Machine::default();
    let entry = MemRegion::Ram.base().get();
    let instruction = [0, 0, 0, 0xef]; // svc #0
    machine
        .memory
        .write_range(PhysicalAddress::new(entry), &instruction)
        .unwrap();
    let mut config = RuntimeConfig::new(machine, entry)
        .with_initial_ram_write(PhysicalAddress::new(entry), instruction.to_vec());
    config.execution_deadline = Some(2);
    config.scheduled_uart.push(ScheduledUartInput {
        at_tick: 1,
        port: UartPort::Uart0,
        bytes: b"x".to_vec(),
    });
    let runtime = RuntimeHandle::spawn(config).unwrap();

    wait_for(&runtime, LifecycleState::Paused);
    assert_eq!(runtime.status().machine.ticks, 2);
    let RuntimeInspection::Peripherals(peripherals) =
        runtime.inspect(InspectionRequest::Peripherals).unwrap()
    else {
        panic!("peripheral inspection has a fixed response type");
    };
    assert_eq!(peripherals.uart0.rx_queued, 1);
    runtime.shutdown().unwrap();
}

#[test]
fn dropped_inspection_receivers_do_not_stop_execution() {
    let runtime = running_runtime();
    wait_for(&runtime, LifecycleState::Running);
    let receiver = runtime
        .request_inspection(RuntimeInspectionRequest::Machine(
            InspectionRequest::Memory(PhysicalRange::new(MemRegion::Ram.base(), 4096).unwrap()),
        ))
        .unwrap();
    drop(receiver);
    runtime.send_uart(UartPort::Uart0, b"coalesced input");
    wait_until(&runtime, "a nonzero tick", |status| {
        status.machine.ticks > 0
    });
    runtime.shutdown().unwrap();
}

#[test]
fn backend_snapshots_share_the_inspection_request_path() {
    let runtime = running_runtime();
    wait_for(&runtime, LifecycleState::Running);
    runtime.pause().unwrap();
    wait_for(&runtime, LifecycleState::Paused);

    let range = PhysicalRange::new(MemRegion::Ram.base(), 4).unwrap();
    let memory = runtime
        .request_inspection(RuntimeInspectionRequest::LiveMemory(range))
        .unwrap()
        .recv()
        .unwrap()
        .unwrap();
    assert!(
        matches!(memory, RuntimeInspection::LiveMemory(_, bytes) if bytes == [0xfe, 0xff, 0xff, 0xea])
    );

    let execution = runtime
        .request_inspection(RuntimeInspectionRequest::Execution {
            address: None,
            before: 0,
            after: 4,
        })
        .unwrap()
        .recv()
        .unwrap()
        .unwrap();
    assert!(
        matches!(execution, RuntimeInspection::Execution(snapshot) if snapshot.instruction_bytes == [0xfe, 0xff, 0xff, 0xea])
    );
    runtime.shutdown().unwrap();
}

#[test]
fn memory_search_requests_return_matches_and_no_match() {
    let runtime = running_runtime();
    wait_for(&runtime, LifecycleState::Running);
    runtime.pause().unwrap();
    wait_for(&runtime, LifecycleState::Paused);

    let match_response = runtime
        .request_inspection(RuntimeInspectionRequest::SearchMemory {
            pattern: vec![0xfe, 0xff, 0xff, 0xea],
        })
        .unwrap()
        .recv()
        .unwrap()
        .unwrap();
    assert_eq!(
        match_response,
        RuntimeInspection::SearchMemory(Some(MemRegion::Ram.base()))
    );

    let no_match_response = runtime
        .request_inspection(RuntimeInspectionRequest::SearchMemory {
            pattern: vec![0xde, 0xad, 0xbe, 0xef, 0xca, 0xfe],
        })
        .unwrap()
        .recv()
        .unwrap()
        .unwrap();
    assert_eq!(no_match_response, RuntimeInspection::SearchMemory(None));
    runtime.shutdown().unwrap();
}
