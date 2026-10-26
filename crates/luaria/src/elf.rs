//! Leitura mínima de ELF64: segmentos, seções e tradução VA <-> offset de arquivo.
//!
//! No payload do Mocktail a primeira PT_LOAD tem p_offset == p_vaddr == 0, então
//! VA e offset coincidem — mas a tradução é feita por segmento mesmo assim, porque
//! a segunda PT_LOAD (rw-) tem deslocamento de arquivo próprio e uma versão futura pode mudar
//! o primeiro segmento.

use std::fs;
use std::io;
use std::path::Path;

pub const PF_X: u32 = 1;
pub const PF_W: u32 = 2;
pub const PF_R: u32 = 4;

pub const PT_LOAD: u32 = 1;

#[derive(Debug, Clone, Copy)]
pub struct Segment {
    pub p_type: u32,
    pub flags: u32,
    pub offset: u64,
    pub vaddr: u64,
    pub filesz: u64,
    pub memsz: u64,
}

impl Segment {
    pub fn executable(&self) -> bool {
        self.flags & PF_X != 0
    }
    pub fn writable(&self) -> bool {
        self.flags & PF_W != 0
    }
    pub fn readable(&self) -> bool {
        self.flags & PF_R != 0
    }
    /// Faixa de VA coberta por bytes presentes no arquivo.
    pub fn file_span(&self) -> (u64, u64) {
        (self.vaddr, self.vaddr + self.filesz)
    }
    pub fn end(&self) -> u64 {
        self.vaddr + self.memsz
    }
}

#[derive(Debug, Clone)]
pub struct Section {
    pub name: String,
    pub sh_type: u32,
    pub flags: u64,
    pub addr: u64,
    pub offset: u64,
    pub size: u64,
    pub link: u32,
    pub entsize: u64,
}

pub struct Elf {
    pub path: String,
    pub data: Vec<u8>,
    pub segments: Vec<Segment>,
    pub sections: Vec<Section>,
}

fn rd_u16(d: &[u8], off: usize) -> u16 {
    u16::from_le_bytes([d[off], d[off + 1]])
}
fn rd_u32(d: &[u8], off: usize) -> u32 {
    u32::from_le_bytes([d[off], d[off + 1], d[off + 2], d[off + 3]])
}
fn rd_u64(d: &[u8], off: usize) -> u64 {
    let mut b = [0u8; 8];
    b.copy_from_slice(&d[off..off + 8]);
    u64::from_le_bytes(b)
}

impl Elf {
    pub fn open<P: AsRef<Path>>(path: P) -> io::Result<Elf> {
        let data = fs::read(path.as_ref())?;
        let p = path.as_ref().to_string_lossy().to_string();
        Elf::parse(p, data)
    }

    pub fn parse(path: String, data: Vec<u8>) -> io::Result<Elf> {
        if data.len() < 64 || &data[..4] != b"\x7fELF" {
            return Err(io::Error::new(io::ErrorKind::InvalidData, "not an ELF file"));
        }
        if data[4] != 2 {
            return Err(io::Error::new(io::ErrorKind::InvalidData, "esperado ELF64"));
        }
        if data[5] != 1 {
            return Err(io::Error::new(io::ErrorKind::InvalidData, "esperado little-endian"));
        }

        let phoff = rd_u64(&data, 32) as usize;
        let shoff = rd_u64(&data, 40) as usize;
        let phentsize = rd_u16(&data, 54) as usize;
        let phnum = rd_u16(&data, 56) as usize;
        let shentsize = rd_u16(&data, 58) as usize;
        let shnum = rd_u16(&data, 60) as usize;
        let shstrndx = rd_u16(&data, 62) as usize;

        let mut segments = Vec::with_capacity(phnum);
        for i in 0..phnum {
            let o = phoff + i * phentsize;
            if o + 56 > data.len() {
                break;
            }
            segments.push(Segment {
                p_type: rd_u32(&data, o),
                flags: rd_u32(&data, o + 4),
                offset: rd_u64(&data, o + 8),
                vaddr: rd_u64(&data, o + 16),
                filesz: rd_u64(&data, o + 32),
                memsz: rd_u64(&data, o + 40),
            });
        }

        let mut sections = Vec::new();
        if shoff != 0 && shnum > 0 && shentsize >= 64 && shoff + shnum * shentsize <= data.len() {
            let strtab_off = if shstrndx < shnum {
                let o = shoff + shstrndx * shentsize;
                rd_u64(&data, o + 24) as usize
            } else {
                0
            };
            for i in 0..shnum {
                let o = shoff + i * shentsize;
                let name_off = rd_u32(&data, o) as usize;
                let name = if strtab_off > 0 {
                    read_cstr(&data, strtab_off + name_off).unwrap_or_default()
                } else {
                    String::new()
                };
                sections.push(Section {
                    name,
                    sh_type: rd_u32(&data, o + 4),
                    flags: rd_u64(&data, o + 8),
                    addr: rd_u64(&data, o + 16),
                    offset: rd_u64(&data, o + 24),
                    size: rd_u64(&data, o + 32),
                    link: rd_u32(&data, o + 40),
                    entsize: rd_u64(&data, o + 56),
                });
            }
        }

        Ok(Elf { path, data, segments, sections })
    }

    pub fn section(&self, name: &str) -> Option<&Section> {
        self.sections.iter().find(|s| s.name == name)
    }

    /// Faixas executáveis com bytes no arquivo (VA inicial, VA final).
    pub fn exec_ranges(&self) -> Vec<(u64, u64)> {
        self.segments
            .iter()
            .filter(|s| s.p_type == PT_LOAD && s.executable() && s.filesz > 0)
            .map(|s| s.file_span())
            .collect()
    }

    pub fn load_segments(&self) -> impl Iterator<Item = &Segment> {
        self.segments.iter().filter(|s| s.p_type == PT_LOAD)
    }

    pub fn va_to_off(&self, va: u64) -> Option<u64> {
        for s in self.load_segments() {
            if va >= s.vaddr && va < s.vaddr + s.filesz {
                return Some(va - s.vaddr + s.offset);
            }
        }
        None
    }

    pub fn off_to_va(&self, off: u64) -> Option<u64> {
        for s in self.load_segments() {
            if off >= s.offset && off < s.offset + s.filesz {
                return Some(off - s.offset + s.vaddr);
            }
        }
        None
    }

    pub fn bytes_at(&self, va: u64, len: usize) -> Option<&[u8]> {
        let off = self.va_to_off(va)? as usize;
        if off + len > self.data.len() {
            return None;
        }
        Some(&self.data[off..off + len])
    }

    pub fn u64_at(&self, va: u64) -> Option<u64> {
        self.bytes_at(va, 8).map(|b| u64::from_le_bytes(b.try_into().unwrap()))
    }

    pub fn u32_at(&self, va: u64) -> Option<u32> {
        self.bytes_at(va, 4).map(|b| u32::from_le_bytes(b.try_into().unwrap()))
    }

    pub fn in_exec(&self, va: u64) -> bool {
        self.exec_ranges().iter().any(|(a, b)| va >= *a && va < *b)
    }

    pub fn read_cstr_at(&self, va: u64, max: usize) -> Option<String> {
        let b = self.bytes_at(va, max.min(4096))?;
        let end = b.iter().position(|&c| c == 0).unwrap_or(b.len());
        Some(String::from_utf8_lossy(&b[..end]).to_string())
    }
}

pub fn read_cstr(data: &[u8], off: usize) -> Option<String> {
    if off >= data.len() {
        return None;
    }
    let end = data[off..].iter().position(|&c| c == 0)?;
    Some(String::from_utf8_lossy(&data[off..off + end]).to_string())
}
