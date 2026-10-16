//! Open-once streaming input. Current-row ownership stays in the cursor until
//! the query resumes after all children, then advances exactly once.
use super::*;
use execution::{Control, Cursor, Row, SourceRef};
enum Input<'a> {
    Source(SourceRef<'a>, usize),
    Cursor(Cursor<'a>),
    Empty,
}
pub(super) struct Scan<'c, 'a, 'w: 'a> {
    input: Input<'a>,
    control: &'c Control<'a, 'w>,
    measurement: Option<usize>,
    equality: bool,
    done: bool,
}
impl<'c, 'a, 'w: 'a> Scan<'c, 'a, 'w> {
    pub fn open(
        source: SourceRef<'a>,
        selected: Option<&[usize]>,
        control: &'c Control<'a, 'w>,
    ) -> Result<Self> {
        let (measurement, provider) = control.work(|w| {
            w.consume(0)?;
            let m = w.scan_index(&source);
            w.scan_call(m, false);
            Ok((m, provider::find(&source, w)))
        })?;
        let input = if let Some(provider) = provider {
            Input::Cursor(control.share(std::cell::RefCell::new(provider.cursor(selected)?))?)
        } else {
            Input::Source(source, 0)
        };
        Ok(Self {
            input,
            control,
            measurement,
            equality: false,
            done: false,
        })
    }
    pub fn equal(
        source: &Source,
        provider: &'a dyn RowProvider,
        column: &str,
        value: &Scalar,
        control: &'c Control<'a, 'w>,
    ) -> Result<Option<Self>> {
        let measurement = control.work(|w| Ok(w.scan_index(source)))?;
        let input = match provider.equal_cursor(column, value) {
            Ok(EqualityCursor::Declined) => return Ok(None),
            Ok(EqualityCursor::Empty) => Input::Empty,
            Ok(EqualityCursor::Rows(cursor)) => {
                match control.share(std::cell::RefCell::new(cursor)) {
                    Ok(cursor) => Input::Cursor(cursor),
                    Err(e) => {
                        control.work(|w| {
                            w.scan_call(measurement, true);
                            Ok(())
                        })?;
                        return Err(e);
                    }
                }
            }
            Err(error) => {
                control.work(|w| {
                    w.scan_call(measurement, true);
                    Ok(())
                })?;
                return Err(error);
            }
        };
        Ok(Some(Self {
            input,
            control,
            measurement,
            equality: true,
            done: false,
        }))
    }
    pub fn next(&mut self) -> Result<Option<Row<'a>>> {
        if self.done {
            return Ok(None);
        }
        let row = match &mut self.input {
            Input::Source(source, next) => {
                if *next < source.rows.len() {
                    let row = Row::Source(source.clone(), *next);
                    *next += 1;
                    Some(row)
                } else {
                    None
                }
            }
            Input::Cursor(cursor) => {
                if cursor.borrow_mut().advance()? {
                    Some(Row::Cursor(cursor.clone()))
                } else {
                    None
                }
            }
            Input::Empty => None,
        };
        if row.is_some() {
            self.control.work(|w| {
                w.scan_row(self.measurement);
                Ok(())
            })?;
        } else {
            self.finish();
            if !self.equality {
                self.control.work(|w| w.consume(0))?;
            }
        }
        Ok(row)
    }
    fn finish(&mut self) {
        if !self.done {
            if self.equality {
                let _ = self.control.work(|w| {
                    w.scan_call(self.measurement, true);
                    Ok(())
                });
            }
            self.done = true;
        }
    }
}
impl Drop for Scan<'_, '_, '_> {
    fn drop(&mut self) {
        self.finish();
    }
}
