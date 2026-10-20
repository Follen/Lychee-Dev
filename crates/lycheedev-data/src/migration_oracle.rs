//! Frozen-interface migration oracle, never called by production dispatch.
//! Available only to unit tests or the explicit migration qualification build.
use crate::{agent_session, assets, db2, fault, generation, hotfix, local_query, navigate};
use lycheedev_core::{Invocation, Outcome, Result, ToolkitError};

pub fn dispatch(inv: &Invocation) -> Result<Outcome> {
    if inv.command == "data agent schema" {
        inv.control.check()?;
        return Ok(Outcome::new(agent_session::schema()));
    }
    if inv.command == "data agent serve" {
        return Err(fault(
            "data.session.supervisor_required",
            3,
            "Data Agent serve requires the CLI-owned supervised process entry",
        ));
    }
    if inv.command == "data cache prepare" {
        return generation::dispatch(inv);
    }
    if inv.command == "data hotfix"
        && inv.flag_bool("--scan")
        && inv.flag("--source") != Some("dbcache")
    {
        return Err(ToolkitError::arguments("--scan requires --source dbcache"));
    }
    if matches!(
        inv.command.as_str(),
        "asset search" | "asset export" | "asset demux"
    ) {
        return assets::dispatch(inv);
    }
    if matches!(
        inv.command.as_str(),
        "data spell info"
            | "data spell auras"
            | "data spell summons"
            | "data item get"
            | "data item models"
            | "data item geosets"
            | "data item textures"
            | "data decor list"
            | "data decor get"
            | "data creature display"
            | "data creature model"
            | "data encounter get"
    ) {
        return navigate::dispatch(inv);
    }
    if matches!(
        inv.command.as_str(),
        "data db2" | "data db2 schema" | "data db2 search" | "data db2 foreign-key"
    ) {
        return db2::dispatch(inv);
    }
    if inv.command == "data sql" {
        return local_query::sql(inv);
    }
    if inv.command == "asset inspect" {
        return local_query::asset(inv);
    }
    if inv.command == "data hotfix" {
        return match inv.required("--source")? {
            "dbcache" => hotfix::dispatch(inv),
            "wago" | "raidbots" => hotfix::remote::dispatch(inv),
            _ => Err(ToolkitError::arguments(
                "--source must be wago, dbcache, or raidbots",
            )),
        };
    }
    Err(fault(
        "command.unsupported",
        3,
        format!(
            "Rust data command has not completed parity: {}",
            inv.command
        ),
    ))
}

