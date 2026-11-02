//! Select one borrowed-GUI or native task session in the existing worker.
//! The enclosing process owner enforces native-call deadlines. Connections,
//! author signer and identity are retained across RPCs without another login.
use super::{
    CredentialKind, DdTransport, Installation, SavedCredential, cc::CcSession, desktop, device,
    request_app_id,
};
use lycheedev_core::{Result, ToolkitError, control::OperationControl};
use serde_json::{Value, json};
use std::path::Path;

pub struct NativeSession {
    cc: Option<CcSession>,
    author: Option<DdTransport>,
    identity: Value,
    kind: CredentialKind,
}
impl NativeSession {
    pub(super) fn open(
        home: &Path,
        installation: &Installation,
        control: &OperationControl,
    ) -> Result<Self> {
        control.check()?;
        let credential = SavedCredential::open(control)?;
        let kind = credential.kind;
        let device = device::load(home, control)?;
        let appid = request_app_id(&installation.program_dir, control)?;
        // Metadata/signature/SDK preparation may take time. Recheck at the
        // actual native-login boundary, preserving the chosen mode on failure.
        desktop::require_absent(control)?;
        let cc = CcSession::start(credential, appid, device, control)?;
        Self::attach(cc, kind, |cc| {
            // Match startup's JWT dependency, but retain no cached JWT or token
            // in public state. Channel work asks for a fresh JWT on this link.
            cc.jwt_for(control)?;
            DdTransport::open(
                &installation.program_dir.join("libnep.dll"),
                cc.author_cookie().to_owned(),
                String::new(),
                control,
            )
        })
    }
    pub(super) fn attach(
        cc: CcSession,
        kind: CredentialKind,
        author: impl FnOnce(&mut CcSession) -> Result<DdTransport>,
    ) -> Result<Self> {
        let mut session = Self {
            cc: Some(cc),
            author: None,
            identity: Value::Null,
            kind,
        };
        let opened: Result<()> = (|| {
            let cc = session.cc.as_mut().expect("starting session owns CC");
            cc.check_active()?;
            session.author = Some(author(cc)?);
            session.identity = session.author.as_mut().unwrap().login()?;
            session.cc.as_ref().unwrap().check_active()?;
            Ok(())
        })();
        if let Err(error) = opened {
            return match session.finish() {
                Ok(()) => Err(error),
                Err(cleanup) => Err(error.with_details(json!({"cleanup_error":cleanup.message}))),
            };
        }
        Ok(session)
    }
    pub fn identity(&self) -> Value {
        json!({"session_source":"native-login", "credential_kind":match self.kind { CredentialKind::Email=>"urs", CredentialKind::Mobile=>"mobile" },
            "native_login_count":1,"cc":self.cc.as_ref().map(CcSession::identity),"author":self.identity})
    }
    pub(super) fn begin_operation(&mut self, control: &OperationControl) -> Result<()> {
        control.check()?;
        self.check_active()?;
        self.author
            .as_mut()
            .expect("ready session owns author")
            .begin_operation(control)
    }
    fn check_active(&self) -> Result<()> {
        self.cc
            .as_ref()
            .ok_or_else(|| {
                ToolkitError::new("publish.dd_session_closed", "DD task session is closed", 3)
            })?
            .check_active()
    }
    pub fn get(&mut self, path: &str, query: &[(&str, String)]) -> Result<Value> {
        self.check_active()?;
        self.author
            .as_mut()
            .expect("ready session owns author")
            .get(path, query)
    }
    /// The business operation records its durable intent before calling this.
    /// A rejected or uncertain result never initiates another login or POST.
    pub fn post(
        &mut self,
        path: &str,
        query: &[(&str, String)],
        body: &Value,
        mutation: bool,
    ) -> Result<Value> {
        self.check_active()
            .map_err(|error| super::submission(error, mutation, "not_sent"))?;
        self.author
            .as_mut()
            .expect("ready session owns author")
            .post(path, query, body, mutation)
    }
    pub fn close(mut self) -> Result<()> {
        self.finish()
    }
    fn finish(&mut self) -> Result<()> {
        // Try both releases even if the first fails. Only this native task's
        // signer and CC link are owned here; no desktop logout/process is used.
        let author = self.author.take().map_or(Ok(()), DdTransport::close);
        let cc = self.cc.take().map_or(Ok(()), CcSession::close);
        match (author, cc) {
            (Ok(()), Ok(())) => Ok(()),
            (Err(error), Ok(())) | (Ok(()), Err(error)) => Err(error),
            (Err(error), Err(cleanup)) => {
                Err(error.with_details(json!({"cleanup_error":cleanup.message})))
            }
        }
    }
}
impl Drop for NativeSession {
    fn drop(&mut self) {
        let _ = self.finish();
    }
}

pub(super) enum TaskSession {
    Native(NativeSession),
    Borrowed(BorrowedSession),
}
pub(super) fn route<T>(
    selection: Result<Option<desktop::Desktop>>,
    native: impl FnOnce() -> Result<T>,
    borrowed: impl FnOnce(desktop::Desktop) -> Result<T>,
) -> Result<T> {
    match selection? {
        None => native(),
        Some(desktop) => borrowed(desktop),
    }
}
impl TaskSession {
    pub(super) fn open(home: &Path, control: &OperationControl) -> Result<Self> {
        route(
            desktop::select(control),
            || {
                let installation = Installation::discover(control)?;
                NativeSession::open(home, &installation, control).map(Self::Native)
            },
            |desktop| {
                let cookie = desktop.cookie(control)?;
                let author = DdTransport::open(
                    &desktop.installation.program_dir.join("libnep.dll"),
                    cookie.into_value(),
                    String::new(),
                    control,
                )?;
                BorrowedSession::attach(author, desktop, home.into()).map(Self::Borrowed)
            },
        )
    }
    pub(super) fn identity(&self) -> Value {
        match self {
            Self::Native(s) => s.identity(),
            Self::Borrowed(s) => s.identity(),
        }
    }
    pub(super) fn begin_operation(&mut self, control: &OperationControl) -> Result<()> {
        match self {
            Self::Native(s) => s.begin_operation(control),
            Self::Borrowed(s) => s.author.as_mut().unwrap().begin_operation(control),
        }
    }
    pub(super) fn get(&mut self, path: &str, query: &[(&str, String)]) -> Result<Value> {
        match self {
            Self::Native(s) => s.get(path, query),
            Self::Borrowed(s) => s.author.as_mut().unwrap().get(path, query),
        }
    }
    pub(super) fn post(
        &mut self,
        path: &str,
        query: &[(&str, String)],
        body: &Value,
        mutation: bool,
    ) -> Result<Value> {
        match self {
            Self::Native(s) => s.post(path, query, body, mutation),
            Self::Borrowed(s) => s.author.as_mut().unwrap().post(path, query, body, mutation),
        }
    }
    pub(super) fn upload(
        &mut self,
        home: &Path,
        request: &super::upload::Request,
        control: &OperationControl,
    ) -> Result<Value> {
        let result = (|| {
            control.check()?;
            let (author, author_id) = self.author_transport()?;
            super::upload::send(author, home, author_id, request, control)
        })();
        result.map_err(|error| {
            if error
                .details
                .get("submission")
                .and_then(Value::as_str)
                .is_some()
            {
                error
            } else {
                super::submission(error, true, "not_sent")
            }
        })
    }
    fn author_transport(&mut self) -> Result<(&mut DdTransport, &str)> {
        let (author, identity) = match self {
            Self::Native(s) => {
                s.check_active()?;
                (&mut s.author, &s.identity)
            }
            Self::Borrowed(s) => (&mut s.author, &s.identity),
        };
        let id = identity
            .get("author_id")
            .and_then(Value::as_str)
            .ok_or_else(|| ToolkitError::invalid("DD selected author identity is missing"))?;
        Ok((author.as_mut().unwrap(), id))
    }
    pub(super) fn backup_upload(
        &mut self,
        home: &Path,
        request: &super::backup::upload::UploadRequest,
        control: &OperationControl,
    ) -> Result<Value> {
        control.check()?;
        let (author, id) = self.author_transport()?;
        super::backup::upload::send(author, home, id, request, control)
    }
    pub(super) fn channels(&mut self, control: &OperationControl) -> Result<Value> {
        control.check()?;
        match self {
            Self::Native(s) => {
                s.check_active()?;
                super::channel::native(s.cc.as_mut().unwrap(), control)
            }
            Self::Borrowed(s) => super::channel::desktop(&s.home, &s.desktop, control),
        }
    }
    pub(super) fn close(self) -> Result<()> {
        match self {
            Self::Native(s) => s.close(),
            Self::Borrowed(s) => s.close(),
        }
    }
}
pub(super) struct BorrowedSession {
    author: Option<DdTransport>,
    identity: Value,
    desktop: desktop::Desktop,
    home: std::path::PathBuf,
}
impl BorrowedSession {
    pub(super) fn attach(
        mut author: DdTransport,
        desktop: desktop::Desktop,
        home: std::path::PathBuf,
    ) -> Result<Self> {
        match author.login() {
            Ok(identity) => Ok(Self {
                author: Some(author),
                identity,
                desktop,
                home,
            }),
            Err(error) => {
                lycheedev_core::metadata::owned_write::finish_owned(Err(error), author.close())
            }
        }
    }
    fn identity(&self) -> Value {
        json!({"session_source":"desktop-cookie","credential_kind":null,"native_login_count":0,
            "logout_requested":false,"gui":self.desktop.gui,"browser":self.desktop.network,"author":self.identity})
    }
    fn finish(&mut self) -> Result<()> {
        self.author.take().map_or(Ok(()), DdTransport::close)
    }
    fn close(mut self) -> Result<()> {
        self.finish()
    }
}
impl Drop for BorrowedSession {
    fn drop(&mut self) {
        let _ = self.finish();
    }
}
