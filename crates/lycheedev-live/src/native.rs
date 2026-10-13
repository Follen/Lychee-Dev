use crate::{
    host::{Services, Target},
    lifecycle::{Session, Transport},
    managed::ManagedInstallation,
};
use lycheedev_core::{Invocation, Result, ToolkitError};
use std::path::Path;
/// Bundled root methods are admitted only after actual static finalization. No numeric RVA fallback.
pub struct WindowsServices<M> {
    managed: M,
    roots: Vec<crate::binding::RootRule>,
    control: lycheedev_core::control::OperationControl,
}
impl<M: ManagedInstallation + Clone + 'static> WindowsServices<M> {
    pub fn new(managed: M) -> Self {
        Self {
            managed,
            roots: Vec::new(),
            control: Default::default(),
        }
    }
    /// Only opaque methods produced by actual static finalization can populate this catalog.
    pub fn with_roots(managed: M, roots: Vec<crate::binding::RootRule>) -> Self {
        Self {
            managed,
            roots,
            control: Default::default(),
        }
    }
    pub fn with_control(managed: M, control: lycheedev_core::control::OperationControl) -> Self {
        Self {
            managed,
            roots: Vec::new(),
            control,
        }
    }
}
impl<M: ManagedInstallation + Clone + 'static> Services for WindowsServices<M> {
    fn managed(&self) -> &dyn ManagedInstallation {
        &self.managed
    }
    fn instances(&self, inv: &Invocation) -> Result<crate::passive::Report> {
        crate::passive::discover(inv, &crate::passive::WindowsSource)
    }
    fn targets(&self, inv: &Invocation) -> Result<Vec<Target>> {
        let pinned = match lycheedev_core::selection::resolve_pinned(inv) {
            Ok(pin) => Some(pin),
            Err(e)
                if e.code == "selection.project_missing"
                    && inv.flag("--project").is_none()
                    && inv.flag("--snapshot").is_none() =>
            {
                None
            }
            Err(e) => return Err(e),
        };
        let pid = inv
            .flag("--pid")
            .map(|s| {
                s.parse::<u32>()
                    .map_err(|_| ToolkitError::arguments("--pid requires an integer"))
            })
            .transpose()?;
        let install = inv
            .flag("--installation")
            .map(std::fs::canonicalize)
            .transpose()?;
        let mut out = Vec::new();
        for window in lycheedev_windows::inventory::windows()
            .map_err(|e| ToolkitError::new("desktop.inventory", e, 3))?
        {
            let name = Path::new(&window.executable)
                .file_name()
                .and_then(|v| v.to_str())
                .unwrap_or("")
                .to_ascii_lowercase();
            if ![
                "wow.exe",
                "wowclassic.exe",
                "wowclassicb.exe",
                "wowclassic64.exe",
            ]
            .contains(&name.as_str())
                || !window.visible
                || pid.is_some_and(|v| v != window.identity.process.pid)
            {
                continue;
            }
            let directory = Path::new(&window.executable)
                .parent()
                .ok_or_else(|| ToolkitError::invalid("game executable parent missing"))?;
            if install
                .as_ref()
                .is_some_and(|v| std::fs::canonicalize(directory).ok().as_ref() != Some(v))
            {
                continue;
            }
            let process = lycheedev_windows::Process::open(window.identity.process.pid)
                .map_err(|e| ToolkitError::new("desktop.process", e, 3))?;
            let module = process
                .main_module()
                .map_err(|e| ToolkitError::new("memory.module", e, 3))?;
            let mut target = Target {
                window: window.identity,
                installation: directory.join("Interface/AddOns/Lychee Dev"),
                actor: String::new(),
                build: String::new(),
                module_sha256: module.hash().into(),
                runtime: String::new(),
            };
            if let Some(rule) = self
                .roots
                .iter()
                .find(|r| r.accepts_hash(&target.module_sha256))
            {
                let memory = crate::binding::BoundMemory::open(target.window.process.pid, rule)
                    .map_err(|e| ToolkitError::new("live.reader_unavailable", e, 3))?;
                if let Some(record) = memory
                    .lookup(&crate::mailbox::Selector::Identity)
                    .map_err(|e| ToolkitError::new("live.identity_unavailable", e, 3))?
                {
                    let info: serde_json::Value = serde_json::from_slice(&record.payload)?;
                    if info["schema"] != "lycheedev.slot.identity.v3"
                        || (info["inventory"] != 500 && info["activationOnly"] != true)
                    {
                        return Err(ToolkitError::new(
                            "live.runtime_inventory",
                            "matching v4 runtime and real 500-slot inventory are required",
                            3,
                        ));
                    }
                    target.actor = info["guid"]
                        .as_str()
                        .ok_or_else(|| ToolkitError::invalid("actor identity missing"))?
                        .into();
                    target.build = info["build"]
                        .as_str()
                        .ok_or_else(|| ToolkitError::invalid("build identity missing"))?
                        .into();
                    target.runtime = record.runtime;
                    let local = lycheedev_core::client::inspect(directory)?;
                    if info["product"] != local.product || target.build != local.full_build {
                        return Err(ToolkitError::new(
                            "live.local_identity_changed",
                            "observed runtime product/full build differs from bounded local client metadata",
                            3,
                        ));
                    }
                    if inv.flag("--character").is_some_and(|name| {
                        info["character"]
                            .as_str()
                            .is_none_or(|v| v.to_lowercase() != name.to_lowercase())
                    }) || inv.flag("--realm").is_some_and(|realm| {
                        info["realm"]
                            .as_str()
                            .is_none_or(|v| v.to_lowercase() != realm.to_lowercase())
                    }) {
                        continue;
                    }
                    if let Some(pin) = &pinned
                        && (pin.data.as_ref().is_some_and(|p| {
                            p.product != local.product || p.full_build != target.build
                        }) || pin.source.as_ref().is_some_and(|p| {
                            p.product != local.product
                                || lycheedev_core::selection::source_interface(p)
                                    != Some(local.interface)
                        }))
                    {
                        continue;
                    }
                }
            }
            out.push(target);
        }
        Ok(out)
    }
    fn open(&self, target: &Target, _: Option<&Session>) -> Result<Box<dyn Transport>> {
        target
            .window
            .verify()
            .map_err(|e| ToolkitError::new("desktop.identity_changed", e, 3))?;
        // This precedes every publication or input, including first activation.
        let rule=self.roots.iter().find(|r|r.accepts_hash(&target.module_sha256)).ok_or_else(||ToolkitError::new("live.locator_not_finalized","no statically finalized root method and exact-hash Lua layout are bundled for the selected binary",3).with_details(serde_json::json!({"moduleSha256":target.module_sha256,"required":["finalizedRootMethod","typedReaderBinding","exactHashLuaLayout"],"inputSent":false})))?;
        let channel = crate::windows_channel::WindowsChannel::open(
            target.clone(),
            self.managed.clone(),
            rule,
            self.control.clone(),
        )
        .map_err(|e| ToolkitError::new("live.native_channel", e, 3))?;
        Ok(Box::new(
            crate::transport::NativeTransport::new(channel, 0)
                .map_err(|e| ToolkitError::new("live.route", e, 4))?,
        ))
    }
}
