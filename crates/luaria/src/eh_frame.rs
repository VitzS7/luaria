//! Tabela de funções do `.eh_frame_hdr`.
//!
//! É a lista oficial (start + FDE) que o próprio loader usa para desenrolar a
//! pilha. Com ela, "onde começa e onde termina a função" deixa de ser heurística
//! de prólogo/`int3` e passa a ser dado do binário.

use crate::elf::Elf;

#[derive(Debug, Clone, Copy)]
pub struct Funcao {
    pub ini: u64,
    pub fim: u64,
}

const DW_EH_PE_OMIT: u8 = 0xff;
const DW_EH_PE_ULEB128: u8 = 0x01;
const DW_EH_PE_UDATA2: u8 = 0x02;
const DW_EH_PE_UDATA4: u8 = 0x03;
const DW_EH_PE_UDATA8: u8 = 0x04;
const DW_EH_PE_SLEB128: u8 = 0x09;
const DW_EH_PE_SDATA2: u8 = 0x0a;
const DW_EH_PE_SDATA4: u8 = 0x0b;
const DW_EH_PE_SDATA8: u8 = 0x0c;
const DW_EH_PE_PCREL: u8 = 0x10;
const DW_EH_PE_DATAREL: u8 = 0x30;
const DW_EH_PE_ABSPTR: u8 = 0x00;

struct Leitor<'a> {
    d: &'a [u8],
    pos: usize,
    base: u64,
    ini: u64,
}

impl<'a> Leitor<'a> {
    fn u8(&mut self) -> Option<u8> {
        let b = *self.d.get(self.pos)?;
        self.pos += 1;
        Some(b)
    }
    fn u16(&mut self) -> Option<u16> {
        let s = self.d.get(self.pos..self.pos + 2)?;
        self.pos += 2;
        Some(u16::from_le_bytes(s.try_into().ok()?))
    }
    fn u32(&mut self) -> Option<u32> {
        let s = self.d.get(self.pos..self.pos + 4)?;
        self.pos += 4;
        Some(u32::from_le_bytes(s.try_into().ok()?))
    }
    fn u64(&mut self) -> Option<u64> {
        let s = self.d.get(self.pos..self.pos + 8)?;
        self.pos += 8;
        Some(u64::from_le_bytes(s.try_into().ok()?))
    }
    fn uleb(&mut self) -> Option<u64> {
        let mut v = 0u64;
        let mut shift = 0;
        loop {
            let b = self.u8()?;
            v |= ((b & 0x7f) as u64) << shift;
            if b & 0x80 == 0 {
                return Some(v);
            }
            shift += 7;
            if shift > 63 {
                return None;
            }
        }
    }
    fn sleb(&mut self) -> Option<i64> {
        let mut v = 0i64;
        let mut shift = 0;
        loop {
            let b = self.u8()?;
            v |= ((b & 0x7f) as i64) << shift;
            shift += 7;
            if b & 0x80 == 0 {
                if shift < 64 && b & 0x40 != 0 {
                    v |= -1i64 << shift;
                }
                return Some(v);
            }
            if shift > 63 {
                return None;
            }
        }
    }

    fn valor(&mut self, enc: u8) -> Option<u64> {
        if enc == DW_EH_PE_OMIT {
            return None;
        }
        let pos_antes = self.ini + self.pos as u64;
        let base = match enc & 0x70 {
            DW_EH_PE_PCREL => pos_antes,
            DW_EH_PE_DATAREL => self.base,
            _ => 0,
        };
        let v = match enc & 0x0f {
            DW_EH_PE_ABSPTR => self.u64()?,
            DW_EH_PE_ULEB128 => self.uleb()?,
            DW_EH_PE_UDATA2 => self.u16()? as u64,
            DW_EH_PE_UDATA4 => self.u32()? as u64,
            DW_EH_PE_UDATA8 => self.u64()?,
            DW_EH_PE_SLEB128 => self.sleb()? as u64,
            DW_EH_PE_SDATA2 => self.u16()? as i16 as i64 as u64,
            DW_EH_PE_SDATA4 => self.u32()? as i32 as i64 as u64,
            DW_EH_PE_SDATA8 => self.u64()?,
            _ => return None,
        };
        Some(v.wrapping_add(base))
    }
}

/// Lê `.eh_frame_hdr` e devolve as funções ordenadas por endereço.
pub fn funcoes(elf: &Elf) -> Vec<Funcao> {
    let hdr = match elf.section(".eh_frame_hdr") {
        Some(s) => s,
        None => return Vec::new(),
    };
    let bytes = match elf.data.get(hdr.offset as usize..(hdr.offset + hdr.size) as usize) {
        Some(b) => b,
        None => return Vec::new(),
    };
    if bytes.len() < 4 || bytes[0] != 1 {
        return Vec::new();
    }
    let enc_ptr = bytes[1];
    let enc_count = bytes[2];
    let enc_table = bytes[3];

    let mut l = Leitor { d: bytes, pos: 4, base: hdr.addr, ini: hdr.addr };
    let _eh_frame_ptr = match l.valor(enc_ptr) {
        Some(v) => v,
        None => return Vec::new(),
    };
    let count = match l.valor(enc_count) {
        Some(c) => c,
        None => return Vec::new(),
    };
    let mut inicios = Vec::with_capacity(count as usize);
    for _ in 0..count {
        let ini = match l.valor(enc_table & 0x0f | (enc_table & 0x70)) {
            Some(v) => v,
            None => break,
        };
        // segundo campo é o endereço do FDE; só precisamos pular
        if l.valor(enc_table & 0x0f | (enc_table & 0x70)).is_none() {
            break;
        }
        inicios.push(ini);
    }
    inicios.sort_unstable();
    inicios.dedup();

    let mut out = Vec::with_capacity(inicios.len());
    for (i, ini) in inicios.iter().enumerate() {
        let fim = inicios.get(i + 1).copied().unwrap_or(ini + 0x1000);
        out.push(Funcao { ini: *ini, fim });
    }
    out
}

/// Índice com busca binária: função que contém `addr`.
pub struct Indice {
    pub funcoes: Vec<Funcao>,
}

impl Indice {
    pub fn novo(elf: &Elf) -> Indice {
        Indice { funcoes: funcoes(elf) }
    }

    pub fn contendo(&self, addr: u64) -> Option<Funcao> {
        let i = self.funcoes.partition_point(|f| f.ini <= addr);
        if i == 0 {
            return None;
        }
        let f = self.funcoes[i - 1];
        if addr < f.fim {
            Some(f)
        } else {
            None
        }
    }

    pub fn inicio(&self, addr: u64) -> Option<u64> {
        self.contendo(addr).map(|f| f.ini)
    }

    pub fn fim(&self, addr: u64) -> Option<u64> {
        self.contendo(addr).map(|f| f.fim)
    }
}
