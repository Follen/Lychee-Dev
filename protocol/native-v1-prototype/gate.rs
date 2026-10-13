//! Isolated candidate gate; only integration tests import it, never product code.
use sha2::{Digest, Sha256};
use std::time::{Duration, Instant};

pub const MAX_STATE: usize = 8192;
pub const MAX_READS: u8 = 32;
pub const DELTA: Duration = Duration::from_millis(750);

pub fn hmac(key: &[u8], bytes: &[u8]) -> String {
    let short;
    let key = if key.len() > 64 {
        short = Sha256::digest(key);
        short.as_slice()
    } else {
        key
    };
    let mut inner = [0x36u8; 64];
    let mut outer = [0x5cu8; 64];
    for (index, byte) in key.iter().enumerate() {
        inner[index] ^= byte;
        outer[index] ^= byte;
    }
    let mut first = Sha256::new();
    first.update(inner);
    first.update(bytes);
    let mut second = Sha256::new();
    second.update(outer);
    second.update(first.finalize());
    hex::encode(second.finalize())
}

fn hex_field(value: &str, length: usize) -> bool {
    value.len() == length
        && value.bytes().all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

#[derive(Debug)]
pub struct State<'a> {
    pub binding: &'a str,
    pub runtime: &'a str,
    pub session: &'a str,
    pub number: u32,
    pub nonce: &'a str,
    pub digest: &'a str,
    pub revision: u32,
    pub game_clock: u32,
    pub facts: u8,
}

pub fn decode<'a>(wire: &'a str, key: &[u8]) -> Result<State<'a>, &'static str> {
    if wire.len() > MAX_STATE || !wire.is_ascii() {
        return Err("state_size_or_encoding");
    }
    let mut fields = wire.split('|');
    let mut f = [""; 11];
    for field in &mut f {
        *field = fields.next().ok_or("state_fields")?;
    }
    if fields.next().is_some() || f[0] != "LDP1" {
        return Err("state_fields");
    }
    for (index, length) in [(1,32),(2,32),(3,32),(4,8),(5,64),(6,64),(7,8),(8,8),(9,2),(10,64)] {
        if !hex_field(f[index], length) { return Err("state_field_encoding"); }
    }
    let raw = wire.rsplit_once('|').ok_or("state_fields")?.0;
    let expected = hmac(key, &[b"lycheedev.p0.state\0".as_slice(), raw.as_bytes()].concat());
    let mismatch = expected.bytes().zip(f[10].bytes()).fold(0u8, |acc, (a,b)| acc | (a ^ b));
    if mismatch != 0 { return Err("state_mac"); }
    let facts = u8::from_str_radix(f[9],16).map_err(|_| "state_facts")?;
    if facts > 15 { return Err("state_facts"); }
    Ok(State {
        binding:f[1],runtime:f[2],session:f[3],
        number:u32::from_str_radix(f[4],16).map_err(|_| "state_number")?,
        nonce:f[5],digest:f[6],
        revision:u32::from_str_radix(f[7],16).map_err(|_| "state_revision")?,
        game_clock:u32::from_str_radix(f[8],16).map_err(|_| "state_clock")?,facts,
    })
}

/// Deliberately reproduces v0's broken predicate, using real byte authentication.
pub fn v0_admits(a: &str, b: &str, elapsed: Duration, key: &[u8]) -> bool {
    let (Ok(a),Ok(b))=(decode(a,key),decode(b,key)) else { return false; };
    a.binding == b.binding && a.runtime == b.runtime && a.session == b.session
        && b.revision > a.revision && b.game_clock > a.game_clock
        && elapsed <= DELTA && b.facts == 0
}

#[derive(Clone)]
pub struct Attempt {
    pub binding: String,
    pub runtime: String,
    pub session: String,
    pub number: u32,
    pub nonce: String,
    pub digest: String,
    // Created BEFORE generating the nonce, never reconstructed from first receipt.
    pub started: Instant,
    reads: u8,
    consumed: bool,
}

impl Attempt {
    pub fn new(binding:String,runtime:String,session:String,number:u32,nonce:String,digest:String,started:Instant) -> Result<Self,&'static str> {
        if !hex_field(&binding,32) || !hex_field(&runtime,32) || !hex_field(&session,32)
            || number==0 || !hex_field(&nonce,64) || !hex_field(&digest,64) {
            return Err("attempt_fields");
        }
        Ok(Self { binding,runtime,session,number,nonce,digest,started,reads:0,consumed:false })
    }

    /// Returns a one-use input qualification, NOT proof of OS delivery or execution.
    /// No transport calls exist in this prototype.
    pub fn qualify(&mut self, wire:&str, key:&[u8], observed_at:Instant) -> Result<(),&'static str> {
        if self.consumed { return Err("attempt_consumed"); }
        if self.reads >= MAX_READS { return Err("observation_budget"); }
        self.reads += 1;
        let elapsed=observed_at.checked_duration_since(self.started).ok_or("clock_rollback")?;
        if elapsed > DELTA { return Err("observation_expired"); }
        let state=decode(wire,key)?;
        if state.binding!=self.binding || state.runtime!=self.runtime || state.session!=self.session {
            return Err("wrong_binding");
        }
        if state.number!=self.number || state.nonce!=self.nonce || state.digest!=self.digest {
            return Err("wrong_attempt");
        }
        if state.facts != 0 { return Err("input_blocked"); }
        self.consumed=true;
        Ok(())
    }
}
