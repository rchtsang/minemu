//! Unicorn A32 backend integration for `minemu`.

mod arm;
mod backend;

pub use backend::{BackendError, BackendStop, UnicornBackend};
