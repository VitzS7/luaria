//! Leitura do cabeçalho do bytecode Luau.
//!
//! Layout (versão 3+): `u8 version`, `u8 types_version`, `varint string_count`,
//! depois cada string como `varint len` + bytes. A **primeira** string é o nome
//! do chunk. É o único jeito de saber qual módulo está sendo carregado sem
//! depender de símbolo.

#[derive(Debug, Clone)]
pub struct Cabecalho {
    pub versao: u8,
    pub tipos: u8,
    pub bytecode: bool,
    pub nome: Option<String>,
    pub n_strings: u64,
}

fn varint(d: &[u8], pos: &mut usize) -> Option<u64> {
    let mut valor = 0u64;
    let mut shift = 0;
    loop {
        let b = *d.get(*pos)?;
        *pos += 1;
        valor |= ((b & 0x7f) as u64) << shift;
        if b & 0x80 == 0 {
            return Some(valor);
        }
        shift += 7;
        if shift > 63 {
            return None;
        }
    }
}

pub fn ler(d: &[u8]) -> Cabecalho {
    if d.is_empty() {
        return Cabecalho { versao: 0, tipos: 0, bytecode: false, nome: None, n_strings: 0 };
    }
    // Fonte tem cara de texto; bytecode começa com versões pequenas.
    let bytecode = d[0] <= 16 && d.len() > 3;
    if !bytecode {
        let primeira = d.split(|&c| c == b'\n').next().unwrap_or(&[]);
        return Cabecalho {
            versao: 0,
            tipos: 0,
            bytecode: false,
            nome: Some(String::from_utf8_lossy(&primeira[..primeira.len().min(80)]).to_string()),
            n_strings: 0,
        };
    }
    let versao = d[0];
    let tipos = *d.get(1).unwrap_or(&0);
    let mut pos = 2;
    let n = varint(d, &mut pos).unwrap_or(0);
    let mut nome = None;
    let mut lidos = 0u64;
    for i in 0..n {
        let tam = match varint(d, &mut pos) {
            Some(t) => t as usize,
            None => break,
        };
        if tam > d.len().saturating_sub(pos) {
            break;
        }
        if i == 0 {
            nome = Some(String::from_utf8_lossy(&d[pos..pos + tam]).to_string());
        }
        pos += tam;
        lidos += 1;
    }
    Cabecalho { versao, tipos, bytecode, nome, n_strings: lidos.max(n) }
}
