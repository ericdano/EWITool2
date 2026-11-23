//! Minimal 16-bit PCM WAV writing.

use std::io::Write;
use std::path::Path;

/// Write interleaved samples (-1..1) as 16-bit PCM.
pub fn write(path: &Path, samples: &[f32], channels: u16, sample_rate: u32) -> std::io::Result<()> {
    let data_len = (samples.len() * 2) as u32;
    let block = channels * 2;
    let mut f = std::io::BufWriter::new(std::fs::File::create(path)?);
    f.write_all(b"RIFF")?;
    f.write_all(&(36 + data_len).to_le_bytes())?;
    f.write_all(b"WAVEfmt ")?;
    f.write_all(&16u32.to_le_bytes())?;
    f.write_all(&1u16.to_le_bytes())?;
    f.write_all(&channels.to_le_bytes())?;
    f.write_all(&sample_rate.to_le_bytes())?;
    f.write_all(&(sample_rate * u32::from(block)).to_le_bytes())?;
    f.write_all(&block.to_le_bytes())?;
    f.write_all(&16u16.to_le_bytes())?;
    f.write_all(b"data")?;
    f.write_all(&data_len.to_le_bytes())?;
    for s in samples {
        f.write_all(&((s.clamp(-1.0, 1.0) * 32767.0) as i16).to_le_bytes())?;
    }
    f.flush()
}

/// Read a 16-bit PCM WAV written by [`write`]: (interleaved samples,
/// channels, sample rate).
pub fn read(path: &Path) -> std::io::Result<(Vec<f32>, u16, u32)> {
    let bytes = std::fs::read(path)?;
    let bad = || std::io::Error::new(std::io::ErrorKind::InvalidData, "not a 16-bit PCM WAV");
    if bytes.len() < 44 || &bytes[0..4] != b"RIFF" || &bytes[8..12] != b"WAVE" {
        return Err(bad());
    }
    let u16_at = |i: usize| u16::from_le_bytes([bytes[i], bytes[i + 1]]);
    let u32_at =
        |i: usize| u32::from_le_bytes([bytes[i], bytes[i + 1], bytes[i + 2], bytes[i + 3]]);
    let (mut pos, mut fmt, mut data) = (12, None, None);
    while pos + 8 <= bytes.len() {
        let id = &bytes[pos..pos + 4];
        let len = u32_at(pos + 4) as usize;
        let body = pos + 8;
        match id {
            b"fmt " => {
                fmt = Some((
                    u16_at(body),
                    u16_at(body + 2),
                    u32_at(body + 4),
                    u16_at(body + 14),
                ))
            }
            b"data" => data = Some(&bytes[body..(body + len).min(bytes.len())]),
            _ => {}
        }
        pos = body + len + (len & 1);
    }
    let (format, channels, rate, bits) = fmt.ok_or_else(bad)?;
    if format != 1 || bits != 16 {
        return Err(bad());
    }
    let samples = data
        .ok_or_else(bad)?
        .as_chunks::<2>()
        .0
        .iter()
        .map(|c| f32::from(i16::from_le_bytes(*c)) / 32768.0)
        .collect();
    Ok((samples, channels, rate))
}
