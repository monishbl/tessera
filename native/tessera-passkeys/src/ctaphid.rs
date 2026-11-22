//! CTAPHID (FIDO's framing over 64-byte HID reports, CTAP 2.1 §11.2):
//! a message is an initialization packet (channel id, command with bit 7
//! set, a 16-bit length, 57 bytes) and continuation packets (channel id,
//! sequence 0..127, 59 bytes). `Assembler` turns packets into messages
//! (one at a time, as the transport allows), `frames` a reply into packets.

pub const PACKET: usize = 64;
pub const BROADCAST: u32 = 0xffff_ffff;
const INIT_DATA: usize = PACKET - 7;
const CONT_DATA: usize = PACKET - 5;
/// 57 + 128 × 59: the most a message can carry
pub const MAX_MESSAGE: usize = INIT_DATA + 128 * CONT_DATA;
/// a message's packets must come within this of each other
pub const TIMEOUT_MS: u64 = 3000;

pub const PING: u8 = 0x81;
pub const MSG: u8 = 0x83;
pub const LOCK: u8 = 0x84;
pub const INIT: u8 = 0x86;
pub const WINK: u8 = 0x88;
pub const CBOR: u8 = 0x90;
pub const CANCEL: u8 = 0x91;
pub const KEEPALIVE: u8 = 0xbb;
pub const ERROR: u8 = 0xbf;

pub const ERR_INVALID_CMD: u8 = 0x01;
pub const ERR_INVALID_PAR: u8 = 0x02;
pub const ERR_INVALID_LEN: u8 = 0x03;
pub const ERR_INVALID_SEQ: u8 = 0x04;
pub const ERR_MSG_TIMEOUT: u8 = 0x05;
pub const ERR_CHANNEL_BUSY: u8 = 0x06;
pub const ERR_INVALID_CHANNEL: u8 = 0x0b;
pub const ERR_OTHER: u8 = 0x7f;

pub const STATUS_PROCESSING: u8 = 1;
pub const STATUS_UPNEEDED: u8 = 2;

/// INIT's capabilities: WINK and CBOR (MSG, CTAP1, is served too)
pub const CAPABILITIES: u8 = 0x01 | 0x04;

#[derive(Debug, PartialEq, Eq)]
pub enum Event {
    /// a whole message
    Message { cid: u32, cmd: u8, data: Vec<u8> },
    /// reply this error on this channel
    Error { cid: u32, code: u8 },
    /// nothing yet (part of a message, or a packet to ignore)
    Nothing,
}

struct Partial {
    cid: u32,
    cmd: u8,
    need: usize,
    data: Vec<u8>,
    seq: u8,
    last_ms: u64,
}

#[derive(Default)]
pub struct Assembler {
    partial: Option<Partial>,
}

pub fn cid_of(p: &[u8]) -> u32 {
    u32::from_be_bytes([p[0], p[1], p[2], p[3]])
}

impl Assembler {
    /// One packet as it came (64 bytes), at `now_ms`
    pub fn push(&mut self, p: &[u8], now_ms: u64) -> Event {
        if p.len() < PACKET {
            return Event::Nothing;
        }
        let cid = cid_of(p);
        if let Some(part) = &self.partial {
            if now_ms.saturating_sub(part.last_ms) > TIMEOUT_MS {
                let c = part.cid;
                self.partial = None;
                if p[4] & 0x80 == 0 && cid == c {
                    return Event::Error { cid, code: ERR_MSG_TIMEOUT };
                }
            }
        }
        if p[4] & 0x80 != 0 {
            let cmd = p[4];
            let len = u16::from_be_bytes([p[5], p[6]]) as usize;
            if cid == 0 || (cid == BROADCAST && cmd != INIT) {
                return Event::Error { cid, code: ERR_INVALID_CHANNEL };
            }
            if let Some(part) = &self.partial {
                if part.cid != cid && cmd != INIT {
                    return Event::Error { cid, code: ERR_CHANNEL_BUSY };
                }
                // (INIT resyncs; anything else on the same channel mid-message is out of order)
                let same = part.cid == cid;
                self.partial = None;
                if same && cmd != INIT {
                    return Event::Error { cid, code: ERR_INVALID_SEQ };
                }
            }
            if len > MAX_MESSAGE {
                return Event::Error { cid, code: ERR_INVALID_LEN };
            }
            let first = len.min(INIT_DATA);
            let data = p[7..7 + first].to_vec();
            if first == len {
                return Event::Message { cid, cmd, data };
            }
            self.partial = Some(Partial { cid, cmd, need: len, data, seq: 0, last_ms: now_ms });
            return Event::Nothing;
        }
        // A continuation packet
        let Some(part) = self.partial.as_mut() else { return Event::Nothing };
        if part.cid != cid {
            return Event::Error { cid, code: ERR_CHANNEL_BUSY };
        }
        if p[4] != part.seq {
            self.partial = None;
            return Event::Error { cid, code: ERR_INVALID_SEQ };
        }
        part.seq += 1;
        part.last_ms = now_ms;
        let n = (part.need - part.data.len()).min(CONT_DATA);
        part.data.extend_from_slice(&p[5..5 + n]);
        if part.data.len() == part.need {
            let part = self.partial.take().unwrap();
            return Event::Message { cid: part.cid, cmd: part.cmd, data: part.data };
        }
        Event::Nothing
    }

    /// A message is half in
    pub fn busy(&self) -> bool {
        self.partial.is_some()
    }
}

/// A reply as packets
pub fn frames(cid: u32, cmd: u8, data: &[u8]) -> Vec<[u8; PACKET]> {
    let data = &data[..data.len().min(MAX_MESSAGE)];
    let mut out = Vec::new();
    let mut p = [0u8; PACKET];
    p[..4].copy_from_slice(&cid.to_be_bytes());
    p[4] = cmd | 0x80;
    p[5..7].copy_from_slice(&(data.len() as u16).to_be_bytes());
    let first = data.len().min(INIT_DATA);
    p[7..7 + first].copy_from_slice(&data[..first]);
    out.push(p);
    let mut at = first;
    let mut seq = 0u8;
    while at < data.len() {
        let mut p = [0u8; PACKET];
        p[..4].copy_from_slice(&cid.to_be_bytes());
        p[4] = seq;
        let n = (data.len() - at).min(CONT_DATA);
        p[5..5 + n].copy_from_slice(&data[at..at + n]);
        out.push(p);
        at += n;
        seq += 1;
    }
    out
}

pub fn error(cid: u32, code: u8) -> [u8; PACKET] {
    frames(cid, ERROR, &[code])[0]
}

pub fn keepalive(cid: u32, status: u8) -> [u8; PACKET] {
    frames(cid, KEEPALIVE, &[status])[0]
}

/// INIT's reply: the nonce, the channel, protocol 2, version 1.0.0, capabilities
pub fn init_reply(nonce: &[u8], new_cid: u32) -> Vec<u8> {
    let mut r = nonce[..8.min(nonce.len())].to_vec();
    r.resize(8, 0);
    r.extend_from_slice(&new_cid.to_be_bytes());
    r.extend_from_slice(&[2, 1, 0, 0, CAPABILITIES]);
    r
}

#[cfg(test)]
mod tests {
    use super::*;

    fn packets_of(cid: u32, cmd: u8, data: &[u8]) -> Vec<[u8; PACKET]> {
        let mut f = frames(cid, cmd, data);
        // (frames sets bit 7 already; requests are framed the same way)
        for p in f.iter_mut().take(1) {
            p[4] = cmd | 0x80;
        }
        f
    }

    #[test]
    fn a_short_message_is_one_packet() {
        let f = frames(0x01020304, CBOR, &[0x04]);
        assert_eq!(f.len(), 1);
        assert_eq!(&f[0][..8], &[1, 2, 3, 4, 0x90, 0, 1, 0x04]);
        let mut a = Assembler::default();
        assert_eq!(a.push(&f[0], 0), Event::Message { cid: 0x01020304, cmd: CBOR, data: vec![4] });
    }

    #[test]
    fn long_messages_split_and_join() {
        let data: Vec<u8> = (0..1000u32).map(|i| i as u8).collect();
        let f = packets_of(7, CBOR, &data);
        assert_eq!(f.len(), 1 + (1000 - 57 + 58) / 59);
        assert_eq!(f[1][4], 0);
        assert_eq!(f[2][4], 1);
        let mut a = Assembler::default();
        for p in &f[..f.len() - 1] {
            assert_eq!(a.push(p, 10), Event::Nothing);
        }
        assert_eq!(a.push(f.last().unwrap(), 20), Event::Message { cid: 7, cmd: CBOR, data: data.clone() });
        // the biggest there can be
        let big = vec![0xabu8; MAX_MESSAGE];
        let f = packets_of(9, MSG, &big);
        assert_eq!(f.len(), 129);
        let mut got = Event::Nothing;
        for p in &f {
            got = a.push(p, 0);
        }
        assert_eq!(got, Event::Message { cid: 9, cmd: MSG, data: big });
    }

    #[test]
    fn out_of_order_and_busy_channels_are_errors() {
        let data = vec![1u8; 200];
        let f = packets_of(7, CBOR, &data);
        let mut a = Assembler::default();
        a.push(&f[0], 0);
        // another channel mid-message: busy
        let other = packets_of(8, CBOR, &[1]);
        assert_eq!(a.push(&other[0], 1), Event::Error { cid: 8, code: ERR_CHANNEL_BUSY });
        // a skipped sequence number
        assert_eq!(a.push(&f[2], 2), Event::Error { cid: 7, code: ERR_INVALID_SEQ });
        assert!(!a.busy());
        // a continuation with nothing begun: ignored
        assert_eq!(a.push(&f[1], 3), Event::Nothing);
        // broadcast is for INIT only; channel 0 never
        assert_eq!(a.push(&packets_of(BROADCAST, PING, &[1])[0], 4), Event::Error { cid: BROADCAST, code: ERR_INVALID_CHANNEL });
        assert_eq!(a.push(&packets_of(0, INIT, &[1; 8])[0], 4), Event::Error { cid: 0, code: ERR_INVALID_CHANNEL });
        // a length past the most
        let mut p = packets_of(7, CBOR, &[1])[0];
        p[5..7].copy_from_slice(&((MAX_MESSAGE + 1) as u16).to_be_bytes());
        assert_eq!(a.push(&p, 5), Event::Error { cid: 7, code: ERR_INVALID_LEN });
    }

    #[test]
    fn init_resyncs_and_slow_messages_time_out() {
        let f = packets_of(7, CBOR, &[2u8; 100]);
        let mut a = Assembler::default();
        a.push(&f[0], 0);
        let init = packets_of(BROADCAST, INIT, &[9; 8]);
        assert_eq!(a.push(&init[0], 1), Event::Message { cid: BROADCAST, cmd: INIT, data: vec![9; 8] });
        assert!(!a.busy());
        a.push(&f[0], 0);
        assert_eq!(a.push(&f[1], TIMEOUT_MS + 1), Event::Error { cid: 7, code: ERR_MSG_TIMEOUT });
    }

    #[test]
    fn init_replies_carry_the_nonce_and_channel() {
        let r = init_reply(&[1, 2, 3, 4, 5, 6, 7, 8], 0xdeadbeef);
        assert_eq!(r.len(), 17);
        assert_eq!(&r[..8], &[1, 2, 3, 4, 5, 6, 7, 8]);
        assert_eq!(&r[8..12], &[0xde, 0xad, 0xbe, 0xef]);
        assert_eq!(r[12], 2);
        assert_eq!(r[16], CAPABILITIES);
        assert_eq!(keepalive(5, STATUS_UPNEEDED)[..8], [0, 0, 0, 5, 0xbb, 0, 1, 2]);
        assert_eq!(error(5, ERR_CHANNEL_BUSY)[..8], [0, 0, 0, 5, 0xbf, 0, 1, 6]);
    }
}
