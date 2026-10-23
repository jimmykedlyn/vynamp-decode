//! RIFF / RF64 / BW64 container walking.

use crate::Layout;
use crate::fmt::{FMT_READ, Fmt, parse_fmt};
use agulha_core::{ByteReader, Error, LimitKind, Mode, Options, Result, Source};

const UNKNOWN: u32 = u32::MAX;

/// Walks the chunks of a RIFF-family file whose 4-byte magic was already
/// recognised as `magic` (`RIFF`, `RF64` or `BW64`). The source is positioned
/// at byte 0.
pub(crate) fn parse(src: &mut dyn Source, magic: [u8; 4], opts: &Options) -> Result<Layout> {
    let src_len = src.len();
    let mut r = ByteReader::new(src);
    r.skip(8)?;
    if &r.fourcc()? != b"WAVE" {
        return Err(Error::Invalid {
            what: "form type",
            offset: 8,
        });
    }
    let big = &magic != b"RIFF";
    let mut ds64_data: Option<u64> = None;
    let mut fmt: Option<Fmt> = None;
    loop {
        let start = r.position();
        if src_len.is_some_and(|l| start >= l) {
            return Err(Error::Invalid {
                what: "missing data chunk",
                offset: start,
            });
        }
        let id = r.fourcc()?;
        let size32 = r.u32_le()?;
        let body = r.position();
        let size = u64::from(size32);
        match &id {
            b"ds64" if big => {
                if size < 28 {
                    return Err(Error::Invalid {
                        what: "ds64 size",
                        offset: start,
                    });
                }
                let _riff_size = r.u64_le()?;
                ds64_data = Some(r.u64_le()?);
            }
            b"fmt " => {
                if size > u64::from(opts.limits.max_metadata_bytes) {
                    return Err(Error::Limit(LimitKind::Metadata));
                }
                let n = size.min(FMT_READ as u64) as usize;
                let mut buf = [0u8; FMT_READ];
                r.read_exact(&mut buf[..n])?;
                if fmt.is_none() {
                    fmt = Some(parse_fmt(&buf[..n], size, body, &opts.limits)?);
                }
            }
            b"data" => {
                let Some(fmt) = fmt else {
                    return Err(Error::Invalid {
                        what: "data before fmt",
                        offset: start,
                    });
                };
                let data_len = if size32 != UNKNOWN {
                    Some(size)
                } else if big {
                    Some(ds64_data.ok_or(Error::Invalid {
                        what: "missing ds64",
                        offset: start,
                    })?)
                } else if opts.mode == Mode::Strict {
                    return Err(Error::Invalid {
                        what: "unknown data size",
                        offset: start,
                    });
                } else {
                    None
                };
                return Ok(Layout {
                    fmt,
                    data_start: body,
                    data_len,
                });
            }
            _ => {}
        }
        // Next chunk: payload padded to an even size.
        let invalid_size = Error::Invalid {
            what: "chunk size",
            offset: start,
        };
        let next = body
            .checked_add(size)
            .and_then(|p| p.checked_add(size & 1))
            .ok_or(invalid_size.clone())?;
        let skip = next.checked_sub(r.position()).ok_or(invalid_size)?;
        r.skip(skip)?;
    }
}
