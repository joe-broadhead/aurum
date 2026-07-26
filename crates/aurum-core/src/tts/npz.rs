//! Minimal NPZ / NPY loader for KittenTTS voice embeddings (float32 only).

use crate::error::{ProviderError, Result};
use std::collections::HashMap;
use std::io::Read;
use std::path::Path;
use zip::ZipArchive;

/// One voice style matrix (rows × cols), row-major f32.
#[derive(Debug, Clone)]
pub struct VoiceMatrix {
    pub nrows: usize,
    pub ncols: usize,
    pub data: Vec<f32>,
}

impl VoiceMatrix {
    /// Row at `text_len`, clamped to valid range.
    pub fn style_row(&self, text_len: usize) -> &[f32] {
        let i = text_len.min(self.nrows.saturating_sub(1));
        &self.data[i * self.ncols..(i + 1) * self.ncols]
    }
}

/// Load all float32 arrays from a voices NPZ archive.
pub fn load_voices_npz(path: &Path) -> Result<HashMap<String, VoiceMatrix>> {
    let file = std::fs::File::open(path).map_err(|e| ProviderError::ModelLoad {
        model: path.display().to_string(),
        reason: format!("open voices npz: {e}"),
    })?;
    let mut zip = ZipArchive::new(file).map_err(|e| ProviderError::ModelLoad {
        model: path.display().to_string(),
        reason: format!("invalid npz/zip: {e}"),
    })?;
    let mut out = HashMap::new();
    for i in 0..zip.len() {
        let mut entry = zip.by_index(i).map_err(|e| ProviderError::ModelLoad {
            model: path.display().to_string(),
            reason: format!("zip entry: {e}"),
        })?;
        let name = entry.name().to_string();
        if !name.ends_with(".npy") {
            continue;
        }
        let key = name
            .trim_end_matches(".npy")
            .rsplit('/')
            .next()
            .unwrap_or(&name)
            .to_string();
        let mut buf = Vec::new();
        entry
            .read_to_end(&mut buf)
            .map_err(|e| ProviderError::ModelLoad {
                model: path.display().to_string(),
                reason: format!("read npy: {e}"),
            })?;
        let (shape, data) = parse_npy(&buf)?;
        let (nrows, ncols) = match shape.as_slice() {
            [r, c] => (*r, *c),
            [r] => (*r, 1),
            other => {
                return Err(ProviderError::ModelLoad {
                    model: path.display().to_string(),
                    reason: format!("unexpected voice shape {other:?} for {key}"),
                }
                .into());
            }
        };
        out.insert(key, VoiceMatrix { nrows, ncols, data });
    }
    if out.is_empty() {
        return Err(ProviderError::ModelLoad {
            model: path.display().to_string(),
            reason: "voices npz contained no float32 arrays".into(),
        }
        .into());
    }
    Ok(out)
}

fn parse_npy(data: &[u8]) -> Result<(Vec<usize>, Vec<f32>)> {
    if data.len() < 10 || &data[..6] != b"\x93NUMPY" {
        return Err(ProviderError::Other {
            message: "not a valid NPY file (bad magic)".into(),
        }
        .into());
    }
    let major = data[6];
    let (header_len, header_start) = match major {
        1 => (u16::from_le_bytes([data[8], data[9]]) as usize, 10usize),
        2 => {
            if data.len() < 12 {
                return Err(ProviderError::Other {
                    message: "NPY v2 file too short".into(),
                }
                .into());
            }
            (
                u32::from_le_bytes([data[8], data[9], data[10], data[11]]) as usize,
                12usize,
            )
        }
        _ => {
            return Err(ProviderError::Other {
                message: format!("unsupported NPY version {major}"),
            }
            .into());
        }
    };
    let header_end = header_start + header_len;
    if data.len() < header_end {
        return Err(ProviderError::Other {
            message: "NPY truncated in header".into(),
        }
        .into());
    }
    let header =
        std::str::from_utf8(&data[header_start..header_end]).map_err(|e| ProviderError::Other {
            message: format!("NPY header utf-8: {e}"),
        })?;
    let dtype = extract_header_field(header, "descr")
        .unwrap_or("")
        .trim()
        .trim_matches('\'')
        .trim_matches('"')
        .to_string();
    if !matches!(dtype.as_str(), "<f4" | "=f4" | "|f4" | ">f4") {
        return Err(ProviderError::Other {
            message: format!("unsupported dtype '{dtype}' — only float32"),
        }
        .into());
    }
    let big_endian = dtype.starts_with('>');
    let shape_str = extract_header_field(header, "shape").ok_or_else(|| ProviderError::Other {
        message: "NPY header missing shape".into(),
    })?;
    let shape = parse_shape(shape_str.trim())?;
    let n_elements: usize = shape.iter().product();
    let data_bytes = &data[header_end..];
    if data_bytes.len() < n_elements * 4 {
        return Err(ProviderError::Other {
            message: "NPY data section too short".into(),
        }
        .into());
    }
    let values: Vec<f32> = data_bytes[..n_elements * 4]
        .chunks_exact(4)
        .map(|b| {
            let arr = [b[0], b[1], b[2], b[3]];
            if big_endian {
                f32::from_be_bytes(arr)
            } else {
                f32::from_le_bytes(arr)
            }
        })
        .collect();
    Ok((shape, values))
}

fn extract_header_field<'a>(header: &'a str, key: &str) -> Option<&'a str> {
    let pat = format!("'{key}':");
    let idx = header.find(&pat)?;
    let rest = header[idx + pat.len()..].trim_start();
    if rest.starts_with('(') {
        let end = rest.find(')')? + 1;
        Some(&rest[..end])
    } else if let Some(inner) = rest.strip_prefix('\'') {
        let close = inner.find('\'')?;
        // include both quotes: '\'' + content + '\''
        Some(&rest[..close + 2])
    } else {
        let end = rest.find(',').unwrap_or(rest.len());
        Some(rest[..end].trim())
    }
}

fn parse_shape(s: &str) -> Result<Vec<usize>> {
    let s = s.trim().trim_start_matches('(').trim_end_matches(')');
    if s.trim().is_empty() {
        return Ok(vec![]);
    }
    s.split(',')
        .map(|p| p.trim())
        .filter(|p| !p.is_empty())
        .map(|p| {
            p.parse::<usize>().map_err(|e| {
                ProviderError::Other {
                    message: format!("bad shape component '{p}': {e}"),
                }
                .into()
            })
        })
        .collect()
}
