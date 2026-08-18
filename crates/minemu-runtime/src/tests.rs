use std::{num::NonZeroU64, thread, time::Duration};

use minemu_core::{Machine, PhysicalMemoryAccess};
use minemu_platform::{InspectionRequest, MemRegion, PhysicalAddress, PhysicalRange};

use crate::{
    LifecycleState, RuntimeConfig, RuntimeHandle, RuntimeInspection, RuntimeInspectionRequest,
    UartPort,
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

fn wait_for(runtime: &RuntimeHandle, state: LifecycleState) {
    for _ in 0..500 {
        if runtime.status().lifecycle == state {
            return;
        }
        thread::sleep(Duration::from_millis(2));
    }
    panic!("runtime did not reach {state:?}");
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
fn bounded_resume_executes_exact_instruction_count_and_pauses() {
    let runtime = running_runtime();
    wait_for(&runtime, LifecycleState::Running);
    runtime.pause().unwrap();
    wait_for(&runtime, LifecycleState::Paused);
    let initial_ticks = runtime.status().machine.ticks;

    runtime.resume_for(NonZeroU64::new(7).unwrap()).unwrap();
    for _ in 0..500 {
        let status = runtime.status();
        if status.lifecycle == LifecycleState::Paused && status.machine.ticks == initial_ticks + 7 {
            assert!(
                status
                    .last_stop
                    .as_deref()
                    .is_some_and(|stop| stop.contains("instruction limit reached"))
            );
            runtime.shutdown().unwrap();
            return;
        }
        thread::sleep(Duration::from_millis(2));
    }
    let status = runtime.status();
    panic!(
        "bounded runtime stopped at lifecycle {:?}, tick {} instead of {}",
        status.lifecycle,
        status.machine.ticks,
        initial_ticks + 7
    );
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
    for _ in 0..500 {
        if runtime.status().machine.ticks > 0 {
            break;
        }
        thread::sleep(Duration::from_millis(2));
    }
    assert!(runtime.status().machine.ticks > 0);
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
