//! Busca de bytes, índice de xrefs e desmontagem com iced-x86.
//!
//! Tudo que envolve endereço vem daqui: string -> xref -> início de função.
//! Nada é fixo no código; um payload novo só custa uma nova resolução.

use std::collections::{BTreeMap, HashSet};

use iced_x86::{Decoder, DecoderOptions, FlowControl, Instruction, OpKind};

use crate::elf::Elf;

/// Todas as ocorrências de `needle` em `haystack`, como offsets no arquivo.
pub fn find_all(haystack: &[u8], needle: &[u8]) -> Vec<usize> {
    if needle.is_empty() {
        return Vec::new();
    }
    memchr::memmem::find_iter(haystack, needle).collect()
}

/// Ocorrências de uma string C (seguida de NUL) em todo o arquivo.
pub fn find_cstr(elf: &Elf, s: &str) -> Vec<u64> {
    let mut needle = s.as_bytes().to_vec();
    needle.push(0);
    find_all(&elf.data, &needle)
        .into_iter()
        .filter_map(|off| elf.off_to_va(off as u64))
        .collect()
}

/// Ocorrências de uma string C que aparecem **uma única vez** — âncora mais confiável.
pub fn find_unique_cstr(elf: &Elf, s: &str) -> Option<u64> {
    let hits = find_cstr(elf, s);
    if hits.len() == 1 {
        hits.first().copied()
    } else {
        None
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RefKind {
    /// Operando de memória RIP-relativo (global, tabela, string)
    RipRelMemory,
    /// Chamada ou salto relativo de 32 bits
    NearBranch,
    /// Constante de 64 bits embutida (ponteiro)
    Imm64,
    /// Constante de 32 bits embutida
    Imm32,
}

#[derive(Debug, Clone, Copy)]
pub struct Xref {
    pub from: u64,
    pub target: u64,
    pub kind: RefKind,
}

/// Decodifica todas as faixas executáveis uma vez e guarda só as referências
/// que caem no conjunto de alvos pedido.
pub fn xrefs_to(elf: &Elf, targets: &HashSet<u64>) -> Vec<Xref> {
    let mut out = Vec::new();
    for (ini, fim) in elf.exec_ranges() {
        let off = match elf.va_to_off(ini) {
            Some(o) => o as usize,
            None => continue,
        };
        let len = (fim - ini) as usize;
        if off + len > elf.data.len() {
            continue;
        }
        let code = &elf.data[off..off + len];
        let mut dec = Decoder::with_ip(64, code, ini, DecoderOptions::NONE);
        while dec.can_decode() {
            let instr = dec.decode();
            let ip = instr.ip();
            // vale para qualquer operando de memória RIP-relativo, então não dá para casar por op0_kind
            let alvo = if instr.is_ip_rel_memory_operand() {
                Some((instr.ip_rel_memory_address(), RefKind::RipRelMemory))
            } else if matches!(
                instr.flow_control(),
                FlowControl::Call | FlowControl::UnconditionalBranch
            ) && instr.op0_kind() == OpKind::NearBranch64
            {
                Some((instr.near_branch64(), RefKind::NearBranch))
            } else if instr.op0_kind() == OpKind::Immediate64 && instr.op1_kind() == OpKind::Register {
                Some((instr.immediate64(), RefKind::Imm64))
            } else {
                None
            };
            if let Some((t, kind)) = alvo {
                if targets.contains(&t) {
                    out.push(Xref { from: ip, target: t, kind });
                }
            }
        }
    }
    out
}

/// Acha o início da função que contém `addr`.
///
/// Ordem: prólogo canônico mais próximo (é o que a VM usa neste build) e, se não
/// houver, a fronteira de padding `0xCC`. O passo de padding sozinho erra quando
/// duas funções são coladas sem padding — que é justamente o caso de
/// `luaG_typeerror` neste payload.
pub fn find_fn_start(elf: &Elf, addr: u64) -> Option<u64> {
    let limite_prologo = addr.saturating_sub(0x2000).max(1);
    let mut va = addr;
    while va > limite_prologo {
        va -= 1;
        if looks_like_prologue(elf, va) {
            return Some(va);
        }
    }
    let limite = addr.saturating_sub(0x8000).max(1);
    let mut va = addr;
    while va > limite {
        va -= 1;
        let b = elf.bytes_at(va, 2)?;
        if b[0] == 0xCC && b[1] != 0xCC {
            let start = va + 1;
            if looks_like_prologue(elf, start) {
                return Some(start);
            }
        }
    }
    None
}

fn looks_like_prologue(elf: &Elf, va: u64) -> bool {
    let b = match elf.bytes_at(va, 8) {
        Some(b) => b,
        None => return false,
    };
    if b[0] == 0x55 && b[1] == 0x48 && b[2] == 0x89 && b[3] == 0xE5 {
        return true;
    }
    if b[0] == 0x48 && b[1] == 0x89 && b[2] == 0x5C && b[3] == 0x24 {
        return true;
    }
    if b[0] == 0x48 && b[1] == 0x83 && b[2] == 0xEC {
        return true;
    }
    // pushes de registradores só contam como prólogo quando encabeçam a função; no meio do corpo aparecem demais
    false
}

/// Decodifica uma janela de bytes a partir de `start` e para no primeiro `ret`
/// seguido de padding.
pub fn decode_function(elf: &Elf, start: u64, max: usize) -> Vec<Instruction> {
    let mut out = Vec::new();
    let bytes = match elf.bytes_at(start, max) {
        Some(b) => b,
        None => return out,
    };
    let mut dec = Decoder::with_ip(64, bytes, start, DecoderOptions::NONE);
    while dec.can_decode() {
        let pos = dec.position();
        let instr = dec.decode();
        let is_ret = instr.flow_control() == FlowControl::Return;
        out.push(instr);
        if is_ret {
            // ret seguido de padding = fim da função
            let next = pos + instr.len();
            if next + 1 < bytes.len() && bytes[next] == 0xCC {
                break;
            }
        }
        if out.len() > 20_000 {
            break;
        }
    }
    out
}

/// Chamadas diretas dentro de uma função: (endereço da instrução, destino).
pub fn direct_calls(elf: &Elf, start: u64, max: usize) -> Vec<(u64, u64)> {
    decode_function(elf, start, max)
        .into_iter()
        .filter(|i| i.flow_control() == FlowControl::Call && i.op0_kind() == OpKind::NearBranch64)
        .map(|i| (i.ip(), i.near_branch64()))
        .collect()
}

/// Comprimento exato do prólogo a copiar num trampolim: instruções inteiras até
/// cobrir pelo menos `minimo` bytes.
pub fn prologue_len(elf: &Elf, start: u64, minimo: usize) -> Option<usize> {
    let bytes = elf.bytes_at(start, 64)?;
    let mut dec = Decoder::with_ip(64, bytes, start, DecoderOptions::NONE);
    let mut total = 0usize;
    while total < minimo {
        if !dec.can_decode() {
            return None;
        }
        let instr = dec.decode();
        if instr.len() == 0 {
            return None;
        }
        total += instr.len();
        if total > 32 {
            return None;
        }
    }
    Some(total)
}

/// Mapa "nome de função da lib base -> ponteiro de código" lendo as tabelas
/// `luaL_Reg` (par { const char*, lua_CFunction }) no segmento de dados.
pub fn resolve_lua_reg_tables(elf: &Elf, nomes: &[&str]) -> BTreeMap<String, u64> {
    let mut achados = BTreeMap::new();
    // Onde as tabelas podem morar: qualquer segmento não-executável com bytes no arquivo.
    let mut faixas: Vec<(u64, u64)> = Vec::new();
    for s in elf.load_segments() {
        if s.filesz > 0 && s.readable() && !s.executable() {
            faixas.push((s.vaddr, s.vaddr + s.filesz));
        }
    }
    // Tabelas em .data.rel.ro às vezes caem no segmento R+X por causa do layout do payload.
    for s in elf.load_segments() {
        if s.filesz > 0 && s.readable() {
            faixas.push((s.vaddr, s.vaddr + s.filesz));
        }
    }

    for nome in nomes {
        let mut needle = nome.as_bytes().to_vec();
        needle.push(0);
        for off in find_all(&elf.data, &needle) {
            let str_va = match elf.off_to_va(off as u64) {
                Some(v) => v,
                None => continue,
            };
            // procura qwords que apontem para essa string
            let pat = str_va.to_le_bytes();
            for poff in find_all(&elf.data, &pat) {
                let ref_va = match elf.off_to_va(poff as u64) {
                    Some(v) => v,
                    None => continue,
                };
                // entrada luaL_Reg = { name, func } -> func logo depois
                if let Some(func) = elf.u64_at(ref_va + 8) {
                    if elf.in_exec(func) && looks_like_prologue(elf, func) {
                        achados.entry(nome.to_string()).or_insert(func);
                    }
                }
            }
        }
    }
    achados
}

/// Desmonta um buffer arbitrário já carregado (sem parar no primeiro `ret`).
pub fn decode_bytes(bytes: &[u8], va: u64) -> Vec<Instruction> {
    let mut out = Vec::new();
    let mut dec = Decoder::with_ip(64, bytes, va, DecoderOptions::NONE);
    while dec.can_decode() {
        out.push(dec.decode());
    }
    out
}

/// Comprimento do prólogo que pode ser copiado para o stub sem relocar nada.
///
/// Para no primeiro `call`/`jmp`/`jcc` relativo: copiar esses bytes para outro
/// endereço muda o destino. Devolve pelo menos 5 bytes (tamanho do `jmp rel32`).
pub fn prologo_seguro(elf: &Elf, start: u64) -> Option<usize> {
    use iced_x86::{Decoder, DecoderOptions, FlowControl};
    let bytes = elf.bytes_at(start, 64)?;
    let mut dec = Decoder::with_ip(64, bytes, start, DecoderOptions::NONE);
    let mut total = 0usize;
    while total < 14 {
        if !dec.can_decode() {
            return None;
        }
        let i = dec.decode();
        match i.flow_control() {
            FlowControl::Next | FlowControl::Return | FlowControl::IndirectBranch | FlowControl::IndirectCall => {}
            _ => return if total >= 5 { Some(total) } else { None },
        }
        // instrução com deslocamento RIP-relativo também não sobrevive à cópia
        if i.is_ip_rel_memory_operand() {
            return if total >= 5 { Some(total) } else { None };
        }
        total += i.len();
        if total > 32 {
            return None;
        }
    }
    Some(total)
}
