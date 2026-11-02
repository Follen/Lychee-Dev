//! Fixed application inputs shared by Core operations.
use crate::control::OperationControl;
use std::path::PathBuf;

#[derive(Clone, Debug)]
pub struct RequestContext {
    pub home: PathBuf,
    pub cwd: PathBuf,
    pub control: OperationControl,
}
