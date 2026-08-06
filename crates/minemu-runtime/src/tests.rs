use std::{thread, time::Duration};

use minemu_core::{Machine, PhysicalMemoryAccess};
use minemu_platform::{InspectionRequest, MemRegion, PhysicalAddress, PhysicalRange};

use crate::{LifecycleState, RuntimeConfig, RuntimeHandle, UartPort};

fn running_runtime() -> RuntimeHandle {
    let mut machine = Machine::default();
    let entry = MemRegion::Ram.base().get();
    machine
        .memory
        .write_range(PhysicalAddress::new(entry), &[0xfe, 0xff, 0xff, 0xea])
        .unwrap(); // b .
    RuntimeHandle::spawn(RuntimeConfig::new(machine, entry)).unwrap()
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
fn dropped_inspection_receivers_do_not_stop_execution() {
    let runtime = running_runtime();
    wait_for(&runtime, LifecycleState::Running);
    let receiver = runtime
        .request_inspection(InspectionRequest::Memory(
            PhysicalRange::new(MemRegion::Ram.base(), 4096).unwrap(),
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
