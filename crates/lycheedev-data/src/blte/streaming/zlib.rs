//! One authenticated encoded Z chunk, one bounded decode buffer. The owning
//! Reader keeps encoded storage admitted until this state has been dropped.
use super::*;
use crate::phase_profile::Phase;

pub(super) struct ZlibChunk<'a> {
    codec: flate2::bufread::ZlibDecoder<io::Cursor<Vec<u8>>>,
    buffer: Vec<u8>,
    offset: usize,
    available: usize,
    encoded: usize,
    expected: usize,
    produced: usize,
    emitted: usize,
    finished: bool,
    // Fields containing the actual allocations precede their refund guard.
    _state: RetainedReservation<'a>,
}

impl<'a> ZlibChunk<'a> {
    pub(super) fn new(encoded: Vec<u8>, expected: usize, budget: &'a Budget) -> Result<Self> {
        let state = budget.reserve_retained(128 << 10)?;
        let length = encoded.len() - 1;
        let mut cursor = io::Cursor::new(encoded);
        cursor.set_position(1);
        Ok(Self {
            codec: flate2::bufread::ZlibDecoder::new(cursor),
            buffer: vec![0; 65536],
            offset: 0,
            available: 0,
            encoded: length,
            expected,
            produced: 0,
            emitted: 0,
            finished: false,
            _state: state,
        })
    }

    fn refill(&mut self, decoder: &Decoder<'_>) -> Result<()> {
        debug_assert_eq!(self.offset, self.available);
        let budget = decoder.budget.expect("stream owns a budget");
        let _span = budget.phase(Phase::ZlibInflate);
        decoder.check()?;
        // Match the eager decoder's read request schedule, including its
        // one-byte excess-output probe. Caller buffer sizes never change it.
        let requested = self.buffer.len().min(self.expected - self.produced + 1);
        let n = self
            .codec
            .read(&mut self.buffer[..requested])
            .map_err(|_| fault("container.invalid_format", 4, "Invalid zlib payload"))?;
        decoder.check()?;
        if n == 0 {
            if self.codec.total_in() as usize != self.encoded {
                return malformed();
            }
            if self.produced != self.expected {
                return integrity();
            }
            self.finished = true;
        } else {
            if n > self.expected - self.produced {
                return exceeded();
            }
            budget.charge(Cost::work(1))?;
            self.produced += n;
        }
        self.offset = 0;
        self.available = n;
        Ok(())
    }

    pub(super) fn read(&mut self, out: &mut [u8], decoder: &Decoder<'_>) -> Result<usize> {
        let wanted = out.len().min(65536).min(self.expected - self.emitted);
        let mut copied = 0;
        while copied < wanted {
            if self.offset == self.available {
                self.refill(decoder)?;
            }
            let n = (wanted - copied).min(self.available - self.offset);
            out[copied..copied + n].copy_from_slice(&self.buffer[self.offset..self.offset + n]);
            self.offset += n;
            copied += n;
        }
        if self.emitted + copied == self.expected && !self.finished {
            // Exact decoded size alone is not codec EOF. Validate checksum,
            // framing and total_in before handing off the last bytes; source
            // EOF and the whole CKey still belong to Reader's terminal probe.
            self.refill(decoder)?;
        }
        self.emitted += copied;
        Ok(copied)
    }

    pub(super) fn finished(&self) -> bool {
        self.finished && self.emitted == self.expected
    }

    #[cfg(test)]
    pub(super) fn produced(&self) -> usize {
        self.produced
    }

    #[cfg(test)]
    pub(super) fn buffer_capacity(&self) -> usize {
        self.buffer.capacity() + self.codec.get_ref().get_ref().capacity()
    }
}
