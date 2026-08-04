/// Contract shared by backend-independent peripheral state machines.
///
/// `Update` contains every state-changing input except reads whose ABI itself
/// consumes state, such as a UART receive-data read.
pub trait Peripheral {
    type Register: Copy;
    type Update;
    type Inspection;
    type Error;

    fn read(&mut self, register: Self::Register) -> std::result::Result<u32, Self::Error>;
    fn update(&mut self, update: Self::Update) -> std::result::Result<(), Self::Error>;
    fn inspect(&self) -> Self::Inspection;
}
