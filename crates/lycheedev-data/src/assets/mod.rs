//! Asset formats and publication share authenticated providers and Core evidence.
//! No codec owns installation selection, downloading, or the game process.
pub mod avi;
pub mod demux;
pub mod export;
mod image;
pub mod input;
pub mod listfile;
pub mod output;
pub mod search;
pub mod source;
pub mod texture;

pub mod request;
pub fn dispatch(ctx: &crate::request::DataContext, request: &request::AssetRequest) -> lycheedev_core::Result<lycheedev_core::Outcome> {
    match request {
        request::AssetRequest::Inspect { file_id } => crate::query_source::asset(ctx, *file_id),
        request::AssetRequest::Search(request) => search::run(ctx, request),
        request::AssetRequest::Export(request) => export::run(ctx, request),
        request::AssetRequest::Demux(request) => demux::run(ctx, request),
    }
}

fn fault(code: &str, exit: i32) -> lycheedev_core::ToolkitError {
    lycheedev_core::ToolkitError::new(code, code, exit)
}

pub(crate) mod pair_output;
