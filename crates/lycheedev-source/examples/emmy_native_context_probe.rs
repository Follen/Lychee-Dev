// Isolated native-typing experiment. This example is never a product command.
#[allow(dead_code)]
mod experiment {
    include!("support/wow_binding.rs");
    include!("support/native_context.rs");
    include!("support/object_context.rs");
    include!("support/object_audit.rs");
    include!("support/invocation_lifecycle.rs");
    include!("support/invocation_returns.rs");
}
fn main() { experiment::native_main(); }
