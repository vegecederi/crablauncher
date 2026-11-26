use crate::error::{LauncherError, Result};
use flate2::read::GzDecoder;
use flate2::write::GzEncoder;
use flate2::Compression;
use std::io::{Cursor, Read, Write};
use std::path::Path;

/// Valor NBT completo preservando todos os tipos e campos desconhecidos em round-trip.
#[derive(Debug, Clone, PartialEq)]
pub enum NbtTag {
    Byte(i8),
    Short(i16),
    Int(i32),
    Long(i64),
    Float(f32),
    Double(f64),
    ByteArray(Vec<i8>),
    String(String),
    List(u8, Vec<NbtTag>),
    Compound(Vec<(String, NbtTag)>),
    IntArray(Vec<i32>),
    LongArray(Vec<i64>),
}

impl NbtTag {
    pub fn type_id(&self) -> u8 {
        match self {
            Self::Byte(_) => 1,
            Self::Short(_) => 2,
            Self::Int(_) => 3,
            Self::Long(_) => 4,
            Self::Float(_) => 5,
            Self::Double(_) => 6,
            Self::ByteArray(_) => 7,
            Self::String(_) => 8,
            Self::List(_, _) => 9,
            Self::Compound(_) => 10,
            Self::IntArray(_) => 11,
            Self::LongArray(_) => 12,
        }
    }

    pub fn as_compound(&self) -> Option<&[(String, NbtTag)]> {
        match self {
            Self::Compound(entries) => Some(entries),
            _ => None,
        }
    }

    pub fn as_compound_mut(&mut self) -> Option<&mut Vec<(String, NbtTag)>> {
        match self {
            Self::Compound(entries) => Some(entries),
            _ => None,
        }
    }

    pub fn get(&self, key: &str) -> Option<&NbtTag> {
        self.as_compound()?
            .iter()
            .find(|(k, _)| k == key)
            .map(|(_, v)| v)
    }

    pub fn set_compound_field(&mut self, key: &str, value: NbtTag) {
        if let Some(entries) = self.as_compound_mut() {
            if let Some((_, existing)) = entries.iter_mut().find(|(k, _)| k == key) {
                *existing = value;
            } else {
                entries.push((key.to_string(), value));
            }
        }
    }

    pub fn as_str(&self) -> Option<&str> {
        match self {
            Self::String(s) => Some(s),
            _ => None,
        }
    }

    pub fn as_i32(&self) -> Option<i32> {
        match self {
            Self::Int(v) => Some(*v),
            _ => None,
        }
    }

    pub fn as_i64(&self) -> Option<i64> {
        match self {
            Self::Long(v) => Some(*v),
            _ => None,
        }
    }
}

/// Lê um arquivo NBT (suporta tanto GZip quanto não-comprimido automaticamente).
pub fn read_nbt_file(path: &Path) -> Result<(String, NbtTag)> {
    let raw = std::fs::read(path).map_err(|source| LauncherError::Io {
        path: path.to_path_buf(),
        source,
    })?;
    let bytes = if raw.len() >= 2 && raw[0] == 0x1f && raw[1] == 0x8b {
        let mut decoder = GzDecoder::new(Cursor::new(&raw));
        let mut out = Vec::new();
        decoder
            .read_to_end(&mut out)
            .map_err(|source| LauncherError::Io {
                path: path.to_path_buf(),
                source,
            })?;
        out
    } else {
        raw
    };
    let mut cursor = Cursor::new(bytes.as_slice());
    parse_root(&mut cursor)
        .map_err(|msg| LauncherError::InvalidMetadata(format!("NBT error: {msg}")))
}

/// Escreve um arquivo NBT (comprimido em GZip se `gzip == true`, ou bruto como `servers.dat`).
pub fn write_nbt_file(path: &Path, root_name: &str, root: &NbtTag, gzip: bool) -> Result<()> {
    let mut buf = Vec::new();
    encode_root(&mut buf, root_name, root)
        .map_err(|msg| LauncherError::InvalidMetadata(format!("NBT encode error: {msg}")))?;

    let final_bytes = if gzip {
        let mut encoder = GzEncoder::new(Vec::new(), Compression::default());
        encoder
            .write_all(&buf)
            .and_then(|_| encoder.finish())
            .map_err(|source| LauncherError::Io {
                path: path.to_path_buf(),
                source,
            })?
    } else {
        buf
    };

    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|source| LauncherError::Io {
            path: parent.to_path_buf(),
            source,
        })?;
    }
    let tmp = path.with_extension("dat.tmp");
    std::fs::write(&tmp, &final_bytes).map_err(|source| LauncherError::Io {
        path: tmp.clone(),
        source,
    })?;
    std::fs::rename(&tmp, path).map_err(|source| LauncherError::Io {
        path: path.to_path_buf(),
        source,
    })
}

fn read_u8(r: &mut impl Read) -> std::result::Result<u8, String> {
    let mut b = [0u8; 1];
    r.read_exact(&mut b).map_err(|e| e.to_string())?;
    Ok(b[0])
}
fn read_i16(r: &mut impl Read) -> std::result::Result<i16, String> {
    let mut b = [0u8; 2];
    r.read_exact(&mut b).map_err(|e| e.to_string())?;
    Ok(i16::from_be_bytes(b))
}
fn read_i32(r: &mut impl Read) -> std::result::Result<i32, String> {
    let mut b = [0u8; 4];
    r.read_exact(&mut b).map_err(|e| e.to_string())?;
    Ok(i32::from_be_bytes(b))
}
fn read_i64(r: &mut impl Read) -> std::result::Result<i64, String> {
    let mut b = [0u8; 8];
    r.read_exact(&mut b).map_err(|e| e.to_string())?;
    Ok(i64::from_be_bytes(b))
}
fn read_string(r: &mut impl Read) -> std::result::Result<String, String> {
    let len = read_i16(r)? as u16 as usize;
    let mut buf = vec![0u8; len];
    r.read_exact(&mut buf).map_err(|e| e.to_string())?;
    Ok(String::from_utf8_lossy(&buf).into_owned())
}

fn parse_root(r: &mut impl Read) -> std::result::Result<(String, NbtTag), String> {
    let tag_id = read_u8(r)?;
    if tag_id != 10 {
        return Err(format!("expected root compound (10), got {tag_id}"));
    }
    let name = read_string(r)?;
    let val = parse_payload(r, 10)?;
    Ok((name, val))
}

fn parse_payload(r: &mut impl Read, tag_id: u8) -> std::result::Result<NbtTag, String> {
    match tag_id {
        1 => Ok(NbtTag::Byte(read_u8(r)? as i8)),
        2 => Ok(NbtTag::Short(read_i16(r)?)),
        3 => Ok(NbtTag::Int(read_i32(r)?)),
        4 => Ok(NbtTag::Long(read_i64(r)?)),
        5 => {
            let mut b = [0u8; 4];
            r.read_exact(&mut b).map_err(|e| e.to_string())?;
            Ok(NbtTag::Float(f32::from_be_bytes(b)))
        }
        6 => {
            let mut b = [0u8; 8];
            r.read_exact(&mut b).map_err(|e| e.to_string())?;
            Ok(NbtTag::Double(f64::from_be_bytes(b)))
        }
        7 => {
            let len = read_i32(r)?.max(0) as usize;
            let mut buf = vec![0u8; len];
            r.read_exact(&mut buf).map_err(|e| e.to_string())?;
            Ok(NbtTag::ByteArray(
                buf.into_iter().map(|b| b as i8).collect(),
            ))
        }
        8 => Ok(NbtTag::String(read_string(r)?)),
        9 => {
            let elem_id = read_u8(r)?;
            let len = read_i32(r)?.max(0) as usize;
            let mut items = Vec::with_capacity(len.min(4096));
            for _ in 0..len {
                items.push(parse_payload(r, elem_id)?);
            }
            Ok(NbtTag::List(elem_id, items))
        }
        10 => {
            let mut entries = Vec::new();
            loop {
                let id = read_u8(r)?;
                if id == 0 {
                    break;
                }
                let k = read_string(r)?;
                let v = parse_payload(r, id)?;
                entries.push((k, v));
            }
            Ok(NbtTag::Compound(entries))
        }
        11 => {
            let len = read_i32(r)?.max(0) as usize;
            let mut items = Vec::with_capacity(len.min(4096));
            for _ in 0..len {
                items.push(read_i32(r)?);
            }
            Ok(NbtTag::IntArray(items))
        }
        12 => {
            let len = read_i32(r)?.max(0) as usize;
            let mut items = Vec::with_capacity(len.min(4096));
            for _ in 0..len {
                items.push(read_i64(r)?);
            }
            Ok(NbtTag::LongArray(items))
        }
        other => Err(format!("unknown NBT tag id {other}")),
    }
}

fn write_string(w: &mut impl Write, s: &str) -> std::result::Result<(), String> {
    let bytes = s.as_bytes();
    let len = u16::try_from(bytes.len()).map_err(|_| "NBT string too long".to_string())?;
    w.write_all(&len.to_be_bytes()).map_err(|e| e.to_string())?;
    w.write_all(bytes).map_err(|e| e.to_string())
}

fn encode_root(w: &mut impl Write, name: &str, tag: &NbtTag) -> std::result::Result<(), String> {
    w.write_all(&[tag.type_id()]).map_err(|e| e.to_string())?;
    write_string(w, name)?;
    encode_payload(w, tag)
}

fn encode_payload(w: &mut impl Write, tag: &NbtTag) -> std::result::Result<(), String> {
    match tag {
        NbtTag::Byte(v) => w.write_all(&[*v as u8]).map_err(|e| e.to_string()),
        NbtTag::Short(v) => w.write_all(&v.to_be_bytes()).map_err(|e| e.to_string()),
        NbtTag::Int(v) => w.write_all(&v.to_be_bytes()).map_err(|e| e.to_string()),
        NbtTag::Long(v) => w.write_all(&v.to_be_bytes()).map_err(|e| e.to_string()),
        NbtTag::Float(v) => w.write_all(&v.to_be_bytes()).map_err(|e| e.to_string()),
        NbtTag::Double(v) => w.write_all(&v.to_be_bytes()).map_err(|e| e.to_string()),
        NbtTag::ByteArray(v) => {
            w.write_all(&(v.len() as i32).to_be_bytes())
                .map_err(|e| e.to_string())?;
            let bytes: Vec<u8> = v.iter().map(|b| *b as u8).collect();
            w.write_all(&bytes).map_err(|e| e.to_string())
        }
        NbtTag::String(s) => write_string(w, s),
        NbtTag::List(elem_id, items) => {
            w.write_all(&[*elem_id]).map_err(|e| e.to_string())?;
            w.write_all(&(items.len() as i32).to_be_bytes())
                .map_err(|e| e.to_string())?;
            for item in items {
                encode_payload(w, item)?;
            }
            Ok(())
        }
        NbtTag::Compound(entries) => {
            for (k, v) in entries {
                w.write_all(&[v.type_id()]).map_err(|e| e.to_string())?;
                write_string(w, k)?;
                encode_payload(w, v)?;
            }
            w.write_all(&[0u8]).map_err(|e| e.to_string())
        }
        NbtTag::IntArray(items) => {
            w.write_all(&(items.len() as i32).to_be_bytes())
                .map_err(|e| e.to_string())?;
            for item in items {
                w.write_all(&item.to_be_bytes())
                    .map_err(|e| e.to_string())?;
            }
            Ok(())
        }
        NbtTag::LongArray(items) => {
            w.write_all(&(items.len() as i32).to_be_bytes())
                .map_err(|e| e.to_string())?;
            for item in items {
                w.write_all(&item.to_be_bytes())
                    .map_err(|e| e.to_string())?;
            }
            Ok(())
        }
    }
}
