// SPDX-License-Identifier: BSD-3-Clause
// Framing adapted from The Go Authors' compress/gzip; see adjacent GO-LICENSE.
//! Go-compatible gzip delivery. DEFLATE uses the unmodified miniz_oxide public API; CRC uses flate2.
//! Based on Go1.27.1 compress/gzip/gunzip.go; no async select owns decoded output.
use super::*;
use flate2::Crc;
mod decoder;
use decoder::{Decoder, End, Progress};

#[derive(Clone, Copy)]
enum State {
    Header,
    Deflate,
    Footer,
    End,
}
struct Gzip<'a, R> {
    input: R,
    control: &'a OperationControl,
    budget: Option<&'a Budget>,
    deadline: Instant,
    state: State,
    decoder: Decoder,
    input_error: Option<ToolkitError>,
    crc: Crc,
    input_buffer: [u8; 8192],
    start: usize,
    end: usize,
    eof: bool,
}
impl<R: AsyncRead + Unpin> Gzip<'_, R> {
    async fn fill(&mut self) -> Result<()> {
        if self.start != self.end || self.eof {
            return Ok(());
        }
        let n = controlled(
            self.input.read(&mut self.input_buffer),
            self.control,
            self.budget,
            self.deadline,
        )
        .await?
        .map_err(|error| http(error.to_string()))?;
        // A read that signals cancellation and returns n still supplies buffered
        // gzip input. Save it before any stop; do not select away decoded output.
        self.start = 0;
        self.end = n;
        self.eof = n == 0;
        Ok(())
    }
    async fn exact(&mut self, output: &mut [u8], boundary: bool) -> Result<bool> {
        let mut used = 0;
        while used < output.len() {
            self.fill().await?;
            let amount = (output.len() - used).min(self.end - self.start);
            if amount == 0 {
                if boundary && used == 0 {
                    return Ok(false);
                }
                return Err(http("unexpected EOF"));
            }
            output[used..used + amount]
                .copy_from_slice(&self.input_buffer[self.start..self.start + amount]);
            self.start += amount;
            used += amount;
        }
        Ok(true)
    }
    async fn header(&mut self) -> Result<bool> {
        let mut fixed = [0; 10];
        if !self.exact(&mut fixed, true).await? {
            return Ok(false);
        }
        if fixed[..3] != [0x1f, 0x8b, 8] {
            return Err(http("gzip: invalid header"));
        }
        let flags = fixed[3]; // Go ignores reserved bits; CRC still includes them.
        let mut crc = Crc::new();
        crc.update(&fixed);
        let mut scratch = [0; 512];
        if flags & 4 != 0 {
            self.exact(&mut scratch[..2], false).await?;
            crc.update(&scratch[..2]);
            let mut left = u16::from_le_bytes([scratch[0], scratch[1]]) as usize;
            while left != 0 {
                let amount = left.min(scratch.len());
                self.exact(&mut scratch[..amount], false).await?;
                crc.update(&scratch[..amount]);
                left -= amount;
            }
        }
        for flag in [8, 16] {
            if flags & flag != 0 {
                let mut terminated = false;
                for byte in &mut scratch {
                    self.exact(std::slice::from_mut(byte), false).await?;
                    crc.update(std::slice::from_ref(byte));
                    if *byte == 0 {
                        terminated = true;
                        break;
                    }
                }
                if !terminated {
                    return Err(http("gzip: invalid header"));
                }
            }
        }
        if flags & 2 != 0 {
            self.exact(&mut scratch[..2], false).await?;
            if u16::from_le_bytes([scratch[0], scratch[1]]) != crc.sum() as u16 {
                return Err(http("gzip: invalid header"));
            }
        }
        self.decoder = Decoder::default();
        self.input_error = None;
        self.crc.reset();
        Ok(true)
    }
    // `produced` survives every error, including a later footer/header await.
    // Never wrap this future in controlled(): only raw input waits are cancelled.
    async fn next(&mut self, output: &mut [u8], produced: &mut usize) -> Result<()> {
        loop {
            match self.state {
                State::Header => {
                    if !self.header().await? {
                        self.state = State::End;
                        return Ok(());
                    }
                    self.state = State::Deflate;
                    if *produced != 0 {
                        return Ok(());
                    }
                }
                State::Deflate => {
                    if let Some(delivery) = self.decoder.read(output) {
                        *produced = delivery.count;
                        self.crc.update(&output[..*produced]);
                        match delivery.end {
                            None => return Ok(()),
                            Some(End::StreamEnd) => self.state = State::Footer,
                            Some(End::Corrupt) => return Err(http("deflate decompression error")),
                            Some(End::UnexpectedEof) => return Err(http("unexpected EOF")),
                            Some(End::InputError) => {
                                return Err(self.input_error.take().unwrap_or_else(|| {
                                    fault("records.internal", 8, "Missing gzip input error")
                                }));
                            }
                            Some(End::InvalidState) => {
                                return Err(fault("records.internal", 8, "Invalid decoder state"));
                            }
                        }
                        continue;
                    }
                    let step = self
                        .decoder
                        .advance(&self.input_buffer[self.start..self.end]);
                    self.start += step.used;
                    if step.progress == Progress::NeedInput {
                        if self.start != self.end {
                            return Err(fault(
                                "records.internal",
                                8,
                                "Decoder left input unconsumed",
                            ));
                        }
                        if self.eof {
                            self.decoder.input_eof();
                        } else if let Err(error) = self.fill().await {
                            self.input_error = Some(error);
                            self.decoder.input_error();
                        }
                    }
                }
                State::Footer => {
                    let mut footer = [0; 8];
                    self.exact(&mut footer, false).await?;
                    let crc = u32::from_le_bytes(footer[..4].try_into().expect("four bytes"));
                    let size = u32::from_le_bytes(footer[4..].try_into().expect("four bytes"));
                    if crc != self.crc.sum() || size != self.crc.amount() {
                        return Err(http("gzip: invalid checksum"));
                    }
                    self.state = State::Header;
                }
                State::End => return Ok(()),
            }
        }
    }
}
pub(super) async fn read_bounded(
    input: impl AsyncRead + Unpin,
    maximum: usize,
    control: &OperationControl,
    budget: Option<&Budget>,
    deadline: Instant,
) -> Result<Vec<u8>> {
    collect(
        input,
        ReadLimit::Complete(maximum),
        control,
        budget,
        deadline,
    )
    .await
}
pub(super) async fn collect(
    input: impl AsyncRead + Unpin,
    limit: ReadLimit,
    control: &OperationControl,
    budget: Option<&Budget>,
    deadline: Instant,
) -> Result<Vec<u8>> {
    collect_admitted(input, limit, control, budget, deadline, &mut |_| Ok(())).await
}
pub(super) async fn collect_admitted(
    input: impl AsyncRead + Unpin,
    limit: ReadLimit,
    control: &OperationControl,
    budget: Option<&Budget>,
    deadline: Instant,
    admit: &mut dyn FnMut(usize) -> Result<()>,
) -> Result<Vec<u8>> {
    let mut reader = Gzip {
        input,
        control,
        budget,
        deadline,
        state: State::Header,
        decoder: Decoder::default(),
        input_error: None,
        crc: Crc::new(),
        input_buffer: [0; 8192],
        start: 0,
        end: 0,
        eof: false,
    };
    let mut raw = Vec::new();
    let mut output = [0; 8192];
    loop {
        if limit.done(raw.len()) {
            return Ok(raw);
        }
        check(control, budget)?;
        let amount = limit.amount(raw.len(), budget)?;
        let mut produced = 0;
        let result = reader.next(&mut output[..amount], &mut produced).await;
        delivered(produced, result.err(), control, budget)?;
        if produced == 0 {
            return Ok(raw);
        }
        admit(produced)?;
        append(&mut raw, &output[..produced], limit.maximum())?;
    }
}

#[cfg(test)]
pub(super) mod tests;
