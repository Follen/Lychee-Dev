// Go-compatible gzip delivery boundaries over the public raw inflater.
// Uses the unmodified public miniz_oxide 0.9.1 block-boundary API.
#![forbid(unsafe_code)]
use miniz_oxide::inflate::{
    TINFLStatus,
    core::{
        BlockBoundaryState, DecompressorOxide, TINFL_LZ_DICT_SIZE, decompress, inflate_flags::*,
    },
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum End {
    StreamEnd,
    Corrupt,
    UnexpectedEof,
    InvalidState,
    InputError,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Progress {
    NeedInput,
    Continue,
    Ready,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Step {
    pub used: usize,
    pub progress: Progress,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Delivery {
    pub count: usize,
    pub end: Option<End>,
}
#[derive(Clone, Copy)]
struct Block {
    stored: bool,
    produced: bool,
}

// Mirrors only observable Go delivery barriers, not DEFLATE decoding.
// The 32KiB buffer is both the decoder's history and undelivered output.
// It must never be overwritten before the caller drains a published batch.
pub struct Decoder {
    decoder: DecompressorOxide,
    dictionary: [u8; TINFL_LZ_DICT_SIZE],
    read: usize,
    written: usize,
    full_history: bool,
    ready: bool,
    terminal: Option<End>,
    // At a full dictionary Go returns pending data before reading EOB/error.
    // A backend may already see that EOB/error, so retain it for the next read.
    deferred_terminal: bool,
    boundary: BlockBoundaryState,
    block: Option<Block>,
}
impl Default for Decoder {
    fn default() -> Self {
        Self {
            decoder: DecompressorOxide::new(),
            dictionary: [0; TINFL_LZ_DICT_SIZE],
            read: 0,
            written: 0,
            full_history: false,
            ready: false,
            terminal: None,
            deferred_terminal: false,
            boundary: BlockBoundaryState::default(),
            block: None,
        }
    }
}
impl Decoder {
    // Called only at the initial raw-stream boundary or after the decoder's
    // explicit BlockBoundary. It observes 3 header bits without consuming them;
    // it never scans payload bytes for a marker and never parses Huffman codes.
    fn start_block(&mut self, input: &[u8]) -> bool {
        if self.block.is_some() {
            return true;
        }
        let bits = self.boundary.num_bits;
        if bits > 7 {
            self.stop(End::InvalidState);
            return true;
        }
        let mut value = u16::from(self.boundary.bit_buf);
        if bits < 3 {
            let Some(byte) = input.first() else {
                return false;
            };
            value |= u16::from(*byte) << bits;
        }
        self.block = Some(Block {
            stored: value & 6 == 0,
            produced: false,
        });
        true
    }
    fn stop(&mut self, end: End) {
        self.terminal = Some(end);
        self.ready = true;
    }
    pub fn input_eof(&mut self) {
        // The input owner invokes this only after NeedInput and a confirmed
        // zero-length raw read, not merely because the supplied slice is empty.
        if self.ready || self.terminal.is_some() {
            self.stop(End::InvalidState);
        } else {
            self.stop(End::UnexpectedEof);
        }
    }
    pub fn input_error(&mut self) {
        // The outer gzip layer retains the actual typed I/O/control cause.
        // It drains this batch and attaches that cause to the last returned n.
        if self.ready || self.terminal.is_some() {
            self.stop(End::InvalidState);
        } else {
            self.stop(End::InputError);
        }
    }
    pub fn advance(&mut self, input: &[u8]) -> Step {
        if self.ready || self.terminal.is_some() {
            return Step {
                used: 0,
                progress: Progress::Ready,
            };
        }
        if !self.start_block(input) {
            return Step {
                used: 0,
                progress: Progress::NeedInput,
            };
        }
        if self.ready {
            return Step {
                used: 0,
                progress: Progress::Ready,
            };
        }
        let mut flags = TINFL_FLAG_HAS_MORE_INPUT | TINFL_FLAG_STOP_ON_BLOCK_BOUNDARY;
        // Before the first complete dictionary, reject a match distance into
        // bytes that were never output. Wrapping mode alone accepts such data.
        if !self.full_history {
            flags |= TINFL_FLAG_USING_NON_WRAPPING_OUTPUT_BUF;
        }
        let (status, used, count) = decompress(
            &mut self.decoder,
            input,
            &mut self.dictionary,
            self.written,
            flags,
        );
        if used > input.len() || count > TINFL_LZ_DICT_SIZE - self.written {
            self.stop(End::InvalidState);
            return Step {
                used: 0,
                progress: Progress::Ready,
            };
        }
        self.written += count;
        let Some(mut block) = self.block else {
            self.stop(End::InvalidState);
            return Step {
                used,
                progress: Progress::Ready,
            };
        };
        block.produced |= count != 0;
        self.block = Some(block);
        let mut empty_stored_flush = false;
        let end = match status {
            TINFLStatus::BlockBoundary => {
                match self.decoder.block_boundary_state() {
                    Some(boundary) => self.boundary = boundary,
                    None => {
                        self.stop(End::InvalidState);
                        return Step {
                            used,
                            progress: Progress::Ready,
                        };
                    }
                }
                empty_stored_flush = block.stored && !block.produced;
                self.block = None;
                None
            }
            TINFLStatus::Done => Some(End::StreamEnd),
            TINFLStatus::Failed | TINFLStatus::Adler32Mismatch => Some(End::Corrupt),
            TINFLStatus::FailedCannotMakeProgress => Some(End::UnexpectedEof),
            TINFLStatus::BadParam => Some(End::InvalidState),
            TINFLStatus::NeedsMoreInput | TINFLStatus::HasMoreOutput => None,
            _ => Some(End::InvalidState),
        };
        self.terminal = end;
        let full = self.written == TINFL_LZ_DICT_SIZE;
        self.ready = full || end.is_some() || (empty_stored_flush && self.read != self.written);
        self.deferred_terminal = full && end.is_some();
        let progress = if self.ready {
            Progress::Ready
        } else if status == TINFLStatus::NeedsMoreInput {
            Progress::NeedInput
        } else if status == TINFLStatus::HasMoreOutput {
            // The full fixed-size output buffer was handled above.
            self.stop(End::InvalidState);
            Progress::Ready
        } else {
            Progress::Continue
        };
        Step { used, progress }
    }
    pub fn read(&mut self, output: &mut [u8]) -> Option<Delivery> {
        if !self.ready {
            return self.terminal.map(|end| Delivery {
                count: 0,
                end: Some(end),
            });
        }
        let count = output.len().min(self.written - self.read);
        output[..count].copy_from_slice(&self.dictionary[self.read..self.read + count]);
        self.read += count;
        if self.read != self.written {
            return Some(Delivery { count, end: None });
        }
        self.ready = false;
        if self.written == TINFL_LZ_DICT_SIZE {
            self.written = 0;
            self.read = 0;
            self.full_history = true;
        }
        let end = if self.deferred_terminal {
            self.deferred_terminal = false;
            None
        } else {
            self.terminal
        };
        Some(Delivery { count, end })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    // Nonfinal stored abc, then empty nonfinal stored block. No tail follows.
    const SYNC: &[u8] = &[0, 3, 0, 252, 255, b'a', b'b', b'c', 0, 0, 0, 255, 255];
    #[test]
    fn explicit_empty_stored_boundary_publishes_without_tail() {
        for fragment in 1..=SYNC.len() {
            let mut d = Decoder::default();
            let mut offset = 0;
            let mut ready = false;
            while offset < SYNC.len() {
                let end = (offset + fragment).min(SYNC.len());
                let step = d.advance(&SYNC[offset..end]);
                offset += step.used;
                if step.progress == Progress::Ready {
                    ready = true;
                    break;
                }
            }
            assert!(ready, "fragment {fragment}");
            assert_eq!(offset, SYNC.len());
            let mut out = [0; 3];
            assert_eq!(
                d.read(&mut out),
                Some(Delivery {
                    count: 3,
                    end: None
                })
            );
            assert_eq!(&out, b"abc");
            assert_eq!(d.advance(&[]).progress, Progress::NeedInput);
        }
    }
    #[test]
    fn payload_marker_is_not_a_flush() {
        let raw = [0, 8, 0, 247, 255, b'a', b'b', b'c', 0, 0, 0, 255, 255];
        let mut d = Decoder::default();
        let step = d.advance(&raw);
        assert_eq!(step.progress, Progress::Continue);
        assert!(d.read(&mut [0; 16]).is_none());
        assert_eq!(d.advance(&[]).progress, Progress::NeedInput);
    }
    #[test]
    fn terminal_error_is_attached_only_to_last_delivered_slice() {
        let raw = [0, 3, 0, 252, 255, b'a', b'b', b'c', 7];
        let mut d = Decoder::default();
        let first = d.advance(&raw);
        assert_eq!(first.progress, Progress::Continue);
        let next = d.advance(&raw[first.used..]);
        assert_eq!(next.progress, Progress::Ready);
        let mut out = [0; 2];
        assert_eq!(
            d.read(&mut out),
            Some(Delivery {
                count: 2,
                end: None
            })
        );
        assert_eq!(&out, b"ab");
        assert_eq!(
            d.read(&mut out),
            Some(Delivery {
                count: 1,
                end: Some(End::Corrupt)
            })
        );
        assert_eq!(out[0], b'c');
    }
    #[test]
    fn truncation_is_queued_with_partial_output() {
        let raw = [0, 4, 0, 251, 255, b'a', b'b', b'c'];
        let mut d = Decoder::default();
        assert_eq!(d.advance(&raw).progress, Progress::NeedInput);
        assert!(d.read(&mut [0; 3]).is_none());
        d.input_eof();
        let mut out = [0; 3];
        assert_eq!(
            d.read(&mut out),
            Some(Delivery {
                count: 3,
                end: Some(End::UnexpectedEof)
            })
        );
    }
    #[test]
    fn invalid_distance_cannot_read_uninitialized_history() {
        let mut d = Decoder::default();
        assert_eq!(d.advance(&[3, 2, 0]).progress, Progress::Ready);
        assert_eq!(
            d.read(&mut [0; 8]),
            Some(Delivery {
                count: 0,
                end: Some(End::Corrupt)
            })
        );
    }
    #[test]
    fn full_window_defers_end_until_the_next_read() {
        let mut raw = vec![1, 0, 128, 255, 127];
        raw.resize(5 + TINFL_LZ_DICT_SIZE, b'a');
        let mut d = Decoder::default();
        assert_eq!(d.advance(&raw).progress, Progress::Ready);
        let mut out = [0; TINFL_LZ_DICT_SIZE];
        assert_eq!(
            d.read(&mut out),
            Some(Delivery {
                count: TINFL_LZ_DICT_SIZE,
                end: None
            })
        );
        assert_eq!(
            d.read(&mut out),
            Some(Delivery {
                count: 0,
                end: Some(End::StreamEnd)
            })
        );
    }
}
