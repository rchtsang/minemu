use bitflags::bitflags;

/// Direction of an MMU or MMIO access.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Access {
    Fetch,
    Read,
    Write,
}

bitflags! {
    /// Access rights shared by page mappings and MMIO registers.
    #[derive(Clone, Copy, Debug, Eq, PartialEq)]
    pub struct Permissions: u8 {
        const READ = 1 << 4;
        const WRITE = 1 << 1;
        const USER = 1 << 2;
        const EXECUTE = 1 << 3;
    }
}

impl Permissions {
    /// Returns whether this permission set admits an attempted access.
    pub const fn permits(self, access: Access, user_mode: bool) -> bool {
        if user_mode && !self.contains(Self::USER) {
            return false;
        }
        match access {
            Access::Fetch => self.contains(Self::EXECUTE),
            Access::Read => self.contains(Self::READ),
            Access::Write => self.contains(Self::WRITE),
        }
    }
}
