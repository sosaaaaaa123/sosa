//! Lettura e scrittura di file WAV, senza librerie esterne.
//! Serve solo agli esempi e ai test (il plugin riceve l'audio dal DAW).

use std::fs;
use std::io;
use std::path::Path;

/// Audio caricato: un vettore di campioni per ogni canale.
pub struct Audio {
    pub sample_rate: u32,
    pub channels: Vec<Vec<f32>>,
}

fn invalid(msg: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, msg.to_string())
}

/// Legge un WAV PCM a 16, 24 o 32 bit, oppure float a 32 bit.
pub fn read(path: impl AsRef<Path>) -> io::Result<Audio> {
    let bytes = fs::read(path)?;
    if bytes.len() < 12 || &bytes[0..4] != b"RIFF" || &bytes[8..12] != b"WAVE" {
        return Err(invalid("non e' un file WAV"));
    }

    let u16_at = |i: usize| u16::from_le_bytes([bytes[i], bytes[i + 1]]);
    let u32_at = |i: usize| u32::from_le_bytes([bytes[i], bytes[i + 1], bytes[i + 2], bytes[i + 3]]);

    let mut format = None; // (formato, canali, sample rate, bit)
    let mut data = None;
    let mut pos = 12;
    while pos + 8 <= bytes.len() {
        let id = &bytes[pos..pos + 4];
        let size = u32_at(pos + 4) as usize;
        let body = pos + 8;
        let end = (body + size).min(bytes.len());
        match id {
            b"fmt " if size >= 16 => {
                let mut tag = u16_at(body);
                if tag == 0xFFFE && size >= 26 {
                    tag = u16_at(body + 24); // WAVE_FORMAT_EXTENSIBLE
                }
                format = Some((tag, u16_at(body + 2), u32_at(body + 4), u16_at(body + 14)));
            }
            b"data" => data = Some(&bytes[body..end]),
            _ => {}
        }
        pos = body + size + (size & 1);
    }

    let (tag, channels, sample_rate, bits) = format.ok_or_else(|| invalid("manca il blocco fmt"))?;
    let data = data.ok_or_else(|| invalid("manca il blocco data"))?;
    let channels = channels.max(1) as usize;
    let width = bits as usize / 8;
    if width == 0 {
        return Err(invalid("profondita' in bit non valida"));
    }

    let decode = |s: &[u8]| -> Option<f32> {
        match (tag, bits) {
            (1, 16) => Some(i16::from_le_bytes([s[0], s[1]]) as f32 / 32768.0),
            (1, 24) => Some(((i32::from_le_bytes([0, s[0], s[1], s[2]])) >> 8) as f32 / 8_388_608.0),
            (1, 32) => Some(i32::from_le_bytes([s[0], s[1], s[2], s[3]]) as f32 / 2_147_483_648.0),
            (3, 32) => Some(f32::from_le_bytes([s[0], s[1], s[2], s[3]])),
            _ => None,
        }
    };
    if decode(&[0, 0, 0, 0]).is_none() {
        return Err(invalid("formato WAV non supportato (usa PCM 16/24/32 bit o float 32 bit)"));
    }

    let frames = data.len() / (width * channels);
    let mut out = vec![Vec::with_capacity(frames); channels];
    for f in 0..frames {
        for (ch, samples) in out.iter_mut().enumerate() {
            let i = (f * channels + ch) * width;
            samples.push(decode(&data[i..i + width]).unwrap_or(0.0));
        }
    }
    Ok(Audio { sample_rate, channels: out })
}

/// Scrive un WAV PCM a 16 bit (mono o stereo).
pub fn write(path: impl AsRef<Path>, sample_rate: u32, channels: &[Vec<f32>]) -> io::Result<()> {
    let num_channels = channels.len().max(1);
    let frames = channels.iter().map(|c| c.len()).min().unwrap_or(0);
    let data_len = frames * num_channels * 2;

    let mut bytes = Vec::with_capacity(44 + data_len);
    bytes.extend_from_slice(b"RIFF");
    bytes.extend_from_slice(&((36 + data_len) as u32).to_le_bytes());
    bytes.extend_from_slice(b"WAVEfmt ");
    bytes.extend_from_slice(&16u32.to_le_bytes());
    bytes.extend_from_slice(&1u16.to_le_bytes()); // PCM
    bytes.extend_from_slice(&(num_channels as u16).to_le_bytes());
    bytes.extend_from_slice(&sample_rate.to_le_bytes());
    bytes.extend_from_slice(&(sample_rate * num_channels as u32 * 2).to_le_bytes());
    bytes.extend_from_slice(&(num_channels as u16 * 2).to_le_bytes());
    bytes.extend_from_slice(&16u16.to_le_bytes());
    bytes.extend_from_slice(b"data");
    bytes.extend_from_slice(&(data_len as u32).to_le_bytes());
    for f in 0..frames {
        for channel in channels {
            let s = (channel[f].clamp(-1.0, 1.0) * 32767.0).round() as i16;
            bytes.extend_from_slice(&s.to_le_bytes());
        }
    }
    fs::write(path, bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip() {
        let dir = std::env::temp_dir().join("intona_wav_test.wav");
        let left: Vec<f32> = (0..100).map(|i| (i as f32 / 100.0) - 0.5).collect();
        let right: Vec<f32> = left.iter().map(|x| -x).collect();
        write(&dir, 44100, &[left.clone(), right]).unwrap();
        let audio = read(&dir).unwrap();
        assert_eq!(audio.sample_rate, 44100);
        assert_eq!(audio.channels.len(), 2);
        for (a, b) in audio.channels[0].iter().zip(&left) {
            assert!((a - b).abs() < 1e-4);
        }
        let _ = fs::remove_file(dir);
    }
}
