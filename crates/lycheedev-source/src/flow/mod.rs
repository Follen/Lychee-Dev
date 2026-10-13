//! Bounded static secret-value candidates from authenticated source and API rules.
//! A candidate is not runtime secrecy, safety or secure execution taint proof.
pub mod rules;
pub mod types;
mod syntax;
mod analyze;
pub use analyze::analyze;
pub use rules::from_facts;
pub use types::*;
#[cfg(feature="emmy-prototype")]
pub mod public;
