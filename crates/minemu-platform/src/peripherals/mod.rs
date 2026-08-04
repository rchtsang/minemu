pub mod block;
pub mod interrupt;
pub mod rng;
pub mod systick;
pub mod trace;
pub mod uart;

pub(crate) fn invalid_value(register: &'static str, value: u32) -> crate::Result<()> {
    Err(crate::PlatformError::InvalidMmioValue { register, value })
}
