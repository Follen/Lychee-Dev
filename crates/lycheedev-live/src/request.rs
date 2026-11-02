//! Inputs for passive discovery and the local probe registry.
use lycheedev_core::control::OperationControl;
use std::path::PathBuf;

#[derive(Clone, Debug)]
pub struct LiveContext {
    pub home: PathBuf,
    pub control: OperationControl,
}

#[derive(Clone, Debug, Default)]
pub struct InstancesRequest {
    pub installation: Option<PathBuf>,
}

#[derive(Clone, Debug)]
pub enum ProbeRequest {
    Put { name: String, file: PathBuf },
    List { limit: usize, include_removed: bool },
    Show { selector: String },
    Remove { name: String },
}

#[derive(Clone, Debug)]
pub enum LiveRequest {
    Instances(InstancesRequest),
    Probe(ProbeRequest),
}
