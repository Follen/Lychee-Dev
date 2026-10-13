//! Asset formats and publication share authenticated providers and Core evidence.
//! No codec owns installation selection, downloading, or the game process.
pub mod avi;
pub mod demux;
pub mod export;
pub mod input;
mod image;
pub mod listfile;
pub mod output;
pub mod search;
pub mod source;
pub mod texture;

pub fn dispatch(
    inv: &lycheedev_core::Invocation,
) -> lycheedev_core::Result<lycheedev_core::Outcome> {
    match inv.command.as_str() {
        "asset search" => search::run(inv),
        "asset export" => export::run(inv),
        "asset demux" => demux::run(inv),
        _ => Err(fault("command.unsupported", 3)),
    }
}

fn fault(code: &str, exit: i32) -> lycheedev_core::ToolkitError {
    lycheedev_core::ToolkitError::new(code, code, exit)
}
