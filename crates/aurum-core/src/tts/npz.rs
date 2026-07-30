//! Bounded NPZ / NPY loader for KittenTTS voice embeddings (float32 only).
//!
//! Hard limits prevent ZIP bombs, dimension overflow, and uncontrolled
//! allocation (JOE-1593).

use crate::error::{ProviderError, Result};
use std::collections::HashMap;
use std::io::Read;
use std::path::Path;
use zip::ZipArchive;

/// Global adapter/parser limits (documented; not magic numbers).
pub const MAX_NPZ_ARCHIVE_BYTES: u64 = 64 * 1024 * 1024;
pub const MAX_NPZ_ENTRIES: usize = 64;
pub const MAX_NPZ_ENTRY_EXPANDED_BYTES: usize = 32 * 1024 * 1024;
pub const MAX_NPZ_TOTAL_EXPANDED_BYTES: usize = 48 * 1024 * 1024;
pub const MAX_NPZ_FILENAME_LEN: usize = 256;
pub const MAX_MATRIX_DIM: usize = 16_384;
pub const MAX_MATRIX_ELEMENTS: usize = 8 * 1024 * 1024;

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
    let meta = std::fs::metadata(path).map_err(|e| ProviderError::ModelLoad {
        model: path.display().to_string(),
        reason: format!("stat voices npz: {e}"),
    })?;
    if meta.len() > MAX_NPZ_ARCHIVE_BYTES {
        return Err(ProviderError::ModelLoad {
            model: path.display().to_string(),
            reason: format!(
                "npz archive too large ({} > {MAX_NPZ_ARCHIVE_BYTES})",
                meta.len()
            ),
        }
        .into());
    }
    let file = std::fs::File::open(path).map_err(|e| ProviderError::ModelLoad {
        model: path.display().to_string(),
        reason: format!("open voices npz: {e}"),
    })?;
    let mut zip = ZipArchive::new(file).map_err(|e| ProviderError::ModelLoad {
        model: path.display().to_string(),
        reason: format!("invalid npz/zip: {e}"),
    })?;
    if zip.len() > MAX_NPZ_ENTRIES {
        return Err(ProviderError::ModelLoad {
            model: path.display().to_string(),
            reason: format!("too many npz entries ({})", zip.len()),
        }
        .into());
    }
    let mut out = HashMap::new();
    let mut total_expanded = 0usize;
    for i in 0..zip.len() {
        let mut entry = zip.by_index(i).map_err(|e| ProviderError::ModelLoad {
            model: path.display().to_string(),
            reason: format!("zip entry: {e}"),
        })?;
        let name = entry.name().to_string();
        if name.len() > MAX_NPZ_FILENAME_LEN {
            return Err(ProviderError::ModelLoad {
                model: path.display().to_string(),
                reason: "npz entry filename too long".into(),
            }
            .into());
        }
        if !name.ends_with(".npy") {
            continue;
        }
        let key = name
            .trim_end_matches(".npy")
            .rsplit('/')
            .next()
            .unwrap_or(&name)
            .to_string();
        if out.contains_key(&key) {
            return Err(ProviderError::ModelLoad {
                model: path.display().to_string(),
                reason: format!("duplicate voice key '{key}'"),
            }
            .into());
        }
        // Prefer declared uncompressed size when available.
        let declared = entry.size() as usize;
        if declared > MAX_NPZ_ENTRY_EXPANDED_BYTES {
            return Err(ProviderError::ModelLoad {
                model: path.display().to_string(),
                reason: format!("npz entry '{key}' expanded size exceeds limit"),
            }
            .into());
        }
        let mut buf = Vec::new();
        entry
            .by_ref()
            .take(MAX_NPZ_ENTRY_EXPANDED_BYTES as u64 + 1)
            .read_to_end(&mut buf)
            .map_err(|e| ProviderError::ModelLoad {
                model: path.display().to_string(),
                reason: format!("read npy: {e}"),
            })?;
        if buf.len() > MAX_NPZ_ENTRY_EXPANDED_BYTES {
            return Err(ProviderError::ModelLoad {
                model: path.display().to_string(),
                reason: format!("npz entry '{key}' exceeded expanded byte cap"),
            }
            .into());
        }
        total_expanded =
            total_expanded
                .checked_add(buf.len())
                .ok_or_else(|| ProviderError::ModelLoad {
                    model: path.display().to_string(),
                    reason: "expanded size overflow".into(),
                })?;
        if total_expanded > MAX_NPZ_TOTAL_EXPANDED_BYTES {
            return Err(ProviderError::ModelLoad {
                model: path.display().to_string(),
                reason: "total expanded npz bytes exceed limit".into(),
            }
            .into());
        }
        let (shape, data) = parse_npy(&buf)?;
        // Reject nonfinite embeddings.
        if data.iter().any(|v| !v.is_finite()) {
            return Err(ProviderError::ModelLoad {
                model: path.display().to_string(),
                reason: format!("voice '{key}' contains non-finite values"),
            }
            .into());
        }
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
        if nrows == 0 || ncols == 0 || nrows > MAX_MATRIX_DIM || ncols > MAX_MATRIX_DIM {
            return Err(ProviderError::ModelLoad {
                model: path.display().to_string(),
                reason: format!("invalid matrix dims {nrows}x{ncols} for {key}"),
            }
            .into());
        }
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

/// Public for fuzz targets / unit tests.
pub fn parse_npy(data: &[u8]) -> Result<(Vec<usize>, Vec<f32>)> {
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
    if header.contains("fortran_order': True") || header.contains("fortran_order\": True") {
        return Err(ProviderError::Other {
            message: "Fortran-order NPY is not supported".into(),
        }
        .into());
    }
    let shape = parse_shape(shape_str.trim())?;
    for d in &shape {
        if *d == 0 || *d > MAX_MATRIX_DIM {
            return Err(ProviderError::Other {
                message: format!("invalid NPY dimension {d}"),
            }
            .into());
        }
    }
    let n_elements = shape.iter().try_fold(1usize, |acc, &d| {
        acc.checked_mul(d).filter(|&n| n <= MAX_MATRIX_ELEMENTS)
    });
    let n_elements = n_elements.ok_or_else(|| ProviderError::Other {
        message: "NPY dimension product overflow or exceeds element cap".into(),
    })?;
    let need = n_elements
        .checked_mul(4)
        .ok_or_else(|| ProviderError::Other {
            message: "NPY byte length overflow".into(),
        })?;
    let data_bytes = &data[header_end..];
    if data_bytes.len() < need {
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
