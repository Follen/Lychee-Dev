//! Fixed Go ListDocuments keyset pages. Consume each raw document before
//! advancing; never retain 1000 one-MiB bodies just to look up one named target.
use super::*;
use crate::control::OperationControl;
use std::{cell::RefCell, time::Duration};
impl Metadata {
    pub(crate) fn require_historical_schema_with_check(
        &self,
        control: &OperationControl,
        mut check: impl FnMut() -> Result<()>,
    ) -> Result<()> {
        metadata_check(control, &mut check)?;
        self.connection.busy_timeout(Duration::ZERO)?;
        let result =
            checked_sql_with_check(control, control.deadline(), &mut check, &mut || {}, || {
                self.connection
                    .pragma_query_value(None, "user_version", |row| row.get::<_, i32>(0))
            })
            .and_then(|version| {
                if version == 1 {
                    Ok(())
                } else {
                    Err(ToolkitError::new(
                        "vault.unsupported_workspace",
                        "vault.unsupported_workspace",
                        4,
                    ))
                }
            });
        let result = historical_write::finish_historical(
            result,
            self.connection
                .busy_timeout(Duration::from_secs(5))
                .map_err(ToolkitError::from),
        );
        let result = crate::selection::selection_finish(result, control.check());
        crate::selection::selection_finish(result, check())
    }
    pub fn visit_raw_page_controlled_with_check(
        &self,
        prefix: &str,
        after: &str,
        limit: usize,
        control: &OperationControl,
        mut check: impl FnMut() -> Result<()>,
        mut visit: impl FnMut(&str, i64, &[u8]) -> Result<()>,
    ) -> Result<(usize, String)> {
        visit_raw_page(
            &self.connection,
            prefix,
            after,
            limit,
            control,
            Duration::from_secs(5),
            &mut check,
            &mut visit,
        )
    }
}
pub(super) fn visit_raw_page(
    connection: &Connection,
    prefix: &str,
    after: &str,
    limit: usize,
    control: &OperationControl,
    restore_timeout: Duration,
    mut check: impl FnMut() -> Result<()>,
    mut visit: impl FnMut(&str, i64, &[u8]) -> Result<()>,
) -> Result<(usize, String)> {
    metadata_check(control, &mut check)?;
    if !(1..=1000).contains(&limit) {
        return Err(ToolkitError::invalid("vault: invalid document page"));
    }
    connection.busy_timeout(Duration::ZERO)?;
    let result = (|| {
        let mut statement = checked_sql_with_check(
            control,
            control.deadline(),
            &mut check,
            &mut || {},
            || {
                connection.prepare("SELECT key,generation,length(CAST(value AS BLOB)),CASE WHEN length(CAST(value AS BLOB))<=1048576 THEN CAST(value AS BLOB) ELSE NULL END FROM documents WHERE substr(key,1,length(?1))=?1 AND key>?2 ORDER BY key LIMIT ?3")
            },
        )?;
        // The same FnMut is invoked serially, never while a control mutex
        // or another RefCell borrow is held. It reaches SQLite retry waits
        // and every actual row, without exporting a raw cancellation token.
        let supplemental = RefCell::new(&mut check);
        let result = checked_sql_with_check(
            control,
            control.deadline(),
            &mut || (*supplemental.borrow_mut())(),
            &mut || {},
            || {
                let mut rows = statement.query(params![prefix, after, limit as i64])?;
                let mut count = 0;
                let mut last = after.to_owned();
                loop {
                    if let Err(error) = metadata_check(control, &mut *supplemental.borrow_mut()) {
                        return Ok(Err(error));
                    }
                    let row = match rows.next() {
                        Ok(Some(row)) => row,
                        Ok(None) => break,
                        // rusqlite resets Rows on error. Before first delivery,
                        // the entire page can be retried. Never call next() on
                        // that reset cursor and mistake it for a successful EOF,
                        // and never replay a visitor which already consumed rows.
                        Err(error) if count == 0 => return Err(error),
                        Err(error) => return Ok(Err(error.into())),
                    };
                    let key: String = row.get(0)?;
                    let generation: i64 = row.get(1)?;
                    let length: i64 = row.get(2)?;
                    let bytes: Option<Vec<u8>> = row.get(3)?;
                    let Some(bytes) = bytes.filter(|bytes| {
                        (0..=1_048_576).contains(&length) && bytes.len() as i64 == length
                    }) else {
                        return Ok(Err(ToolkitError::invalid(
                            "metadata document exceeds 1 MiB",
                        )));
                    };
                    if let Err(error) = metadata_check(control, &mut *supplemental.borrow_mut())
                        .and_then(|_| visit(&key, generation, &bytes))
                    {
                        return Ok(Err(error));
                    }
                    count += 1;
                    last = key;
                }
                Ok(Ok((count, last)))
            },
        )
        .and_then(|result| result);
        historical_write::finish_historical(
            result,
            statement.finalize().map_err(ToolkitError::from),
        )
    })();
    let result = historical_write::finish_historical(
        result,
        connection
            .busy_timeout(restore_timeout)
            .map_err(ToolkitError::from),
    );
    let result = crate::selection::selection_finish(result, control.check());
    crate::selection::selection_finish(result, check())
}
