#![forbid(unsafe_code)]
// The runner copies production raw_json.rs and raw_json/tests.rs byte-for-byte
// beside this root, retaining Rust's ordinary nested module lookup.
pub mod raw_json;
pub mod mailbox;
pub mod original_lines;
pub mod stderr_observation;
pub mod research_projection;
