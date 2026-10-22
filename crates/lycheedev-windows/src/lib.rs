//! Windows-only, read-only process capabilities and directed input. No game business.
pub mod broker;
pub mod cancellation;
pub mod data_lease;
pub mod detached;
pub mod filesystem;
pub mod guarded_spawn;
pub mod inventory;
pub mod job;
pub mod mailbox_input;
pub mod memory;
pub mod process;
pub mod random;
pub use filesystem::durable_replace;
pub use job::{BrokerJob, JobGuard};
pub use process::{Process, ProcessIdentity, WindowIdentity};
pub type Result<T> = std::result::Result<T, String>;
