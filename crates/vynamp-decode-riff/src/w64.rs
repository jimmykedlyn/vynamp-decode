//! Sony Wave64 container walking.

use crate::Layout;
use crate::fmt::{FMT_READ, Fmt, parse_fmt};
use vynamp_decode_core::{ByteReader, Error, LimitKind, Mode, Options, Result, Source};

const fn guid(d1: u32, d2: u16, d3: u16, d4: [u8; 8]) -> [u8; 16] {
    let a = d1.to_le_bytes();
    let b = d2.to_le_bytes();
    let c = d3.to_le_bytes();
    [
        a[0], a[1], a[2], a[3], b[0], b[1], c[0], c[1], d4[0], d4[1], d4[2], d4[3], d4[4], d4[5],
        d4[6], d4[7],
    ]
}

const TAIL: [u8; 8] = [0x8C, 0xD1, 0x00, 0xC0, 0x4F, 0x8E, 0xDB, 0x8A];
pub(crate) const GUID_RIFF: [u8; 16] = guid(
    0x6666_6972,
    0x912E,
    0x11CF,
    [0xA5, 0xD6, 0x28, 0xDB, 0x04, 0xC1, 0x00, 0x00],
);
pub(crate) const GUID_WAVE: [u8; 16] = guid(0x6576_6177, 0xACF3, 0x11D3, TAIL);
const GUID_FMT: [u8; 16] = guid(0x2074_6D66, 0xACF3, 0x11D3, TAIL);
const GUID_DATA: [u8; 16] = guid(0x6174_6164, 0xACF3, 0x11D3, TAIL);

/// Chunk header size (GUID + u64 size), included in the stored chunk size.
const HEADER: u64 = 24;

fn read_guid(r: &mut ByteReader<'_>) -> Result<[u8; 16]> {
    let mut g = [0u8; 16];
    r.read_exact(&mut g)?;
    Ok(g)
}

/// Walks a Wave64 file; the source is positioned at byte 0.
pub(crate) fn parse(src: &mut dyn Source, opts: &Options) -> Result<Layout> {
    let src_len = src.len();
    let mut r = ByteReader::new(src);
    if read_guid(&mut r)? != GUID_RIFF {
        return Err(Error::Invalid {
            what: "riff guid",
            offset: 0,
        });
    }
    let file_size = r.u64_le()?;
    if read_guid(&mut r)? != GUID_WAVE {
        return Err(Error::Invalid {
            what: "form type",
            offset: 24,
        });
    }
    let mut fmt: Option<Fmt> = None;
    loop {
        opts.cancel.check()?;
        let start = r.position();
        if src_len.is_some_and(|l| start >= l) {
            return Err(Error::Invalid {
                what: "missing data chunk",
                offset: start,
            });
        }
        let id = read_guid(&mut r)?;
        let size = r.u64_le()?;
        let invalid_size = Error::Invalid {
            what: "chunk size",
            offset: start,
        };
        let payload = size.checked_sub(HEADER).ok_or(invalid_size.clone())?;
        let body = r.position();
        if id == GUID_FMT {
            if payload > u64::from(opts.limits.max_metadata_bytes) {
                return Err(Error::Limit(LimitKind::Metadata));
            }
            let n = payload.min(FMT_READ as u64) as usize;
            let mut buf = [0u8; FMT_READ];
            r.read_exact(&mut buf[..n])?;
            if fmt.is_none() {
                fmt = Some(parse_fmt(&buf[..n], payload, body, &opts.limits)?);
            }
        } else if id == GUID_DATA {
            let Some(fmt) = fmt else {
                return Err(Error::Invalid {
                    what: "data before fmt",
                    offset: start,
                });
            };
            // An empty payload followed by more bytes is an unpatched streamed file.
            let more = payload == 0
                && match src_len {
                    Some(l) => body < l,
                    None => r.u8().is_ok(),
                };
            let data_len = if more {
                if opts.mode == Mode::Strict {
                    return Err(Error::Invalid {
                        what: "unknown data size",
                        offset: start,
                    });
                }
                None
            } else {
                Some(payload)
            };
            let container_end = Some(file_size).filter(|&e| e > body && e != u64::MAX);
            return Ok(Layout {
                fmt,
                data_start: body,
                data_len,
                container_end,
            });
        }
        let next = start
            .checked_add(size)
            .and_then(|p| p.checked_next_multiple_of(8))
            .ok_or(invalid_size.clone())?;
        let skip = next.checked_sub(r.position()).ok_or(invalid_size)?;
        r.skip(skip)?;
    }
}
