//! ModUs Creator and main-client author operations keep their identities separate.
use lycheedev_core::{Result, ToolkitError, control::OperationControl};
use serde_json::{Value, json};
mod auth;
mod backup;
mod leveldb;
mod main_form;
mod main_write;
mod media;
mod package;
mod plaintext;
mod plugin;
mod project;
mod protocol;
mod read;
pub use auth::Client;
pub use backup::{BackupCloud, BackupLocal, BackupUpload};
pub use main_write::MainWrite;
pub use media::MediaUpload;
pub use plugin::PluginWrite;
pub use project::ProjectWrite;
pub use read::{Listing, Query};

fn malformed(message: &str) -> ToolkitError {
    ToolkitError::new("publish.modus_protocol", message, 4)
}
pub fn read(query: Query, control: &OperationControl) -> Result<Value> {
    query.validate()?;
    if matches!(query, Query::Builds) {
        return auth::builds(control);
    }
    let client = query.client();
    let session = if query.public() {
        None
    } else {
        Some(auth::Session::load(client, control)?)
    };
    let api = protocol::Api {
        http: crate::transport::Http::new(control)?,
        session,
    };
    read::execute(&api, query).map(|v| crate::output::public(v, &[]))
}

pub fn doctor(client: Client, control: &OperationControl) -> Result<Value> {
    let api = protocol::Api {
        http: crate::transport::Http::new(control)?,
        session: Some(auth::Session::load(client, control)?),
    };
    let account = api.account()?;
    Ok(
        json!({"platform":"modus","client":client.id(),"credential_read":true,"api_ready":true,"account_id":account,"remote_write":false}),
    )
}

#[cfg(test)]
mod tests;
