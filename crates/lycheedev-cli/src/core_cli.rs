//! Freeze CLI context and selectors before entering shared Core operations.
use crate::Invocation;
use lycheedev_core::{
    Result,
    request::RequestContext,
    selection::{self, PinSelectionRequest, PinnedSet},
};
use std::path::PathBuf;

pub(crate) fn context(inv: &Invocation) -> RequestContext {
    RequestContext {
        home: inv.home.clone(),
        cwd: inv.cwd.clone(),
        control: inv.control.clone(),
    }
}

pub(crate) fn pin_request(inv: &Invocation) -> PinSelectionRequest<'_> {
    PinSelectionRequest {
        home: &inv.home,
        cwd: &inv.cwd,
        snapshot: inv.flag("--snapshot"),
        project: inv.flag("--project"),
    }
}

pub(crate) fn resolve_pinned(inv: &Invocation) -> Result<PinnedSet> {
    selection::resolve_pin_request(pin_request(inv), &inv.control)
}

pub(crate) fn resolve_optional_project(inv: &Invocation) -> Result<Option<(PathBuf, PinnedSet)>> {
    selection::resolve_optional_project(pin_request(inv), &inv.control)
}
