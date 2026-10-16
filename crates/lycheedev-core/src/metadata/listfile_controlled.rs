//! One raw row on an already held Metadata owner. Listfile refresh must sample
//! generation after fetch/parse without opening a second connection or decoding
//! the old payload. Original bytes and bounded size remain unchanged.
use super::*;
use crate::control::OperationControl;
use std::time::Duration;
impl Metadata {
    pub fn read_document_on_owner_controlled(
        &self,
        key: &str,
        control: &OperationControl,
    ) -> Result<RawDocument> {
        self.read_document_on_owner_controlled_observed(key, control, &mut || {})
    }
    fn read_document_on_owner_controlled_observed(
        &self,
        key: &str,
        control: &OperationControl,
        on_busy: &mut impl FnMut(),
    ) -> Result<RawDocument> {
        control.check()?;
        self.connection.busy_timeout(Duration::ZERO)?;
        let result = (|| {
            let row: Option<(i64, i64, Option<Vec<u8>>)> = checked_sql(
                control,
                control.deadline(),
                on_busy,
                || {
                    self.connection.query_row(
                    "SELECT generation,length(CAST(value AS BLOB)),CASE WHEN length(CAST(value AS BLOB))<=1048576 THEN CAST(value AS BLOB) ELSE NULL END FROM documents WHERE key=?1",
                    [key], |row| Ok((row.get(0)?,row.get(1)?,row.get(2)?))).optional()
                },
            )?;
            control.check()?;
            match row {
                Some((generation, length, Some(bytes)))
                    if (0..=1048576).contains(&length) && bytes.len() as i64 == length =>
                {
                    Ok(RawDocument { generation, bytes })
                }
                Some(_) => Err(ToolkitError::invalid("metadata document exceeds 1 MiB")),
                None => Err(ToolkitError::new(
                    "vault.record_missing",
                    format!("document {key} does not exist"),
                    3,
                )),
            }
        })();
        let result = historical_write::finish_historical(
            result,
            self.connection
                .busy_timeout(Duration::from_secs(5))
                .map_err(Into::into),
        );
        crate::selection::selection_finish(result, control.check())
    }
}
#[cfg(test)]
mod tests;
