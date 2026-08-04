use minemu_platform::{
    Peripheral, RngInspection,
    peripherals::rng::{DEFAULT_SEED, Register},
};

use crate::Result;

/// One state-changing RNG input.
pub enum RngUpdate {
    Write { register: Register, value: u32 },
}

/// Deterministic xorshift32 RNG state.
pub struct Rng {
    state: u32,
}

impl Rng {
    pub const fn new() -> Self {
        Self {
            state: DEFAULT_SEED,
        }
    }

    pub const fn seed(&self) -> u32 {
        self.state
    }

    fn set_seed(&mut self, seed: u32) {
        self.state = if seed == 0 { DEFAULT_SEED } else { seed };
    }

    fn next_u32(&mut self) -> u32 {
        self.state ^= self.state << 13;
        self.state ^= self.state >> 17;
        self.state ^= self.state << 5;
        self.state
    }

    pub const fn inspect(&self) -> RngInspection {
        RngInspection { state: self.state }
    }
}

impl Default for Rng {
    fn default() -> Self {
        Self::new()
    }
}

impl Peripheral for Rng {
    type Register = Register;
    type Update = RngUpdate;
    type Inspection = RngInspection;
    type Error = crate::CoreError;

    fn read(&mut self, register: Self::Register) -> Result<u32> {
        Ok(match register {
            Register::Seed | Register::State => self.seed(),
            Register::Data => self.next_u32(),
        })
    }

    fn update(&mut self, update: Self::Update) -> Result<()> {
        let RngUpdate::Write { register, value } = update;
        if matches!(register, Register::Seed) {
            self.set_seed(value);
        }
        Ok(())
    }

    fn inspect(&self) -> Self::Inspection {
        Rng::inspect(self)
    }
}

#[cfg(test)]
mod tests {
    use super::Rng;

    #[test]
    fn zero_seed_uses_the_abi_default() {
        let mut rng = Rng::new();
        rng.set_seed(0);
        assert_eq!(rng.next_u32(), 0x791c_7b62);
    }
}
