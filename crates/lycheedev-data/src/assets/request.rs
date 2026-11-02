//! Admitted asset operations; argument parsing belongs to the CLI adapter.
use std::path::PathBuf;

#[derive(Clone, Debug)]
pub enum SearchMode {
    Query(String),
    Extension(String),
    Name(String),
    FileId(u32),
}
#[derive(Clone, Debug)]
pub struct SearchRequest {
    pub listfile: String,
    pub mode: SearchMode,
    pub limit: usize,
    pub maximum_bytes: usize,
}
impl SearchRequest {
    pub fn new(listfile: String, mode: SearchMode) -> Self {
        Self {
            listfile,
            mode,
            limit: 50,
            maximum_bytes: 256 << 20,
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Encoding {
    Raw,
    Png,
    Webp,
}
impl Encoding {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Raw => "raw",
            Self::Png => "png",
            Self::Webp => "webp",
        }
    }
}
#[derive(Clone, Debug)]
pub struct ExportRequest {
    pub file_id: u32,
    pub output: PathBuf,
    pub encoding: Encoding,
    pub mipmap: usize,
    pub channels: String,
    pub max_pixels: usize,
    pub maximum_bytes: usize,
    pub overwrite: bool,
}
impl ExportRequest {
    pub fn raw(file_id: u32, output: PathBuf) -> Self {
        Self {
            file_id,
            output,
            encoding: Encoding::Raw,
            mipmap: 0,
            channels: "rgba".into(),
            max_pixels: 16 << 20,
            maximum_bytes: 128 << 20,
            overwrite: false,
        }
    }
}
#[derive(Clone, Debug)]
pub enum DemuxInput {
    Path(PathBuf),
    Casc { file_id: u32 },
}
#[derive(Clone, Debug)]
pub struct DemuxRequest {
    pub input: DemuxInput,
    pub output: PathBuf,
    pub maximum_bytes: usize,
    pub max_frames: usize,
    pub allow_partial: bool,
    pub overwrite: bool,
}
impl DemuxRequest {
    pub fn new(input: DemuxInput, output: PathBuf) -> Self {
        Self {
            input,
            output,
            maximum_bytes: 128 << 20,
            max_frames: 100000,
            allow_partial: false,
            overwrite: false,
        }
    }
}
#[derive(Clone, Debug)]
pub enum AssetRequest {
    Inspect { file_id: u32 },
    Search(SearchRequest),
    Export(ExportRequest),
    Demux(DemuxRequest),
}
