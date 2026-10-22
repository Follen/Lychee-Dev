//! Bounded static secret-value candidates from authenticated source and API rules.
//! A candidate is not runtime secrecy, safety or secure execution taint proof.
pub mod rules;
pub mod types;
pub use rules::from_facts;
pub use types::*;
pub mod public;
