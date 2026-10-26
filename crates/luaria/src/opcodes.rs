//! Remapeamento de opcodes do bytecode.
//!
//! O compilador público do Luau numera os opcodes pela ordem do enum
//! `LuauOpcode`; a VM do cliente embaralha esses valores por build (a tabela de
//! despacho do interpretador tem 256 ponteiros, só 91 preenchidos, em posições
//! aleatórias). O que não muda entre builds é a **ordem física dos handlers** em
//! `lvmexecute.cpp`: ela segue a ordem das cláusulas `VM_CASE` no arquivo.
//!
//! Então o mapa sai da própria tabela viva: ordena-se os 91 ponteiros não nulos
//! por endereço e casa-se com `OPS_ARQUIVO`; cada nome vira o byte que o cliente
//! espera. A tradução do bytecode usa `OPS_ENUM` (ordem que o compilador grava).

/// Ordem das cláusulas `VM_CASE` em `VM/src/lvmexecute.cpp` (Luau release/735).
pub const OPS_ARQUIVO: [&str; 91] = [
    "NOP", "LOADNIL", "LOADB", "LOADN", "LOADK", "MOVE", "GETGLOBAL", "SETGLOBAL",
    "GETUPVAL", "SETUPVAL", "CLOSEUPVALS", "GETIMPORT", "GETTABLEKS", "SETTABLEKS",
    "GETTABLE", "SETTABLE", "GETTABLEN", "SETTABLEN", "NEWCLOSURE", "NAMECALL",
    "CALL", "CALLFB", "RETURN", "JUMP", "JUMPIF", "JUMPIFNOT", "JUMPIFEQ",
    "JUMPIFNOTEQ", "JUMPIFLE", "JUMPIFNOTLE", "JUMPIFLT", "JUMPIFNOTLT", "ADD",
    "SUB", "MUL", "DIV", "IDIV", "MOD", "POW", "ADDK", "SUBK", "MULK", "DIVK",
    "IDIVK", "MODK", "POWK", "AND", "OR", "ANDK", "ORK", "CONCAT", "NOT", "MINUS",
    "LENGTH", "NEWTABLE", "DUPTABLE", "SETLIST", "FORNPREP", "FORNLOOP", "FORGPREP",
    "FORGLOOP", "FORGPREP_INEXT", "FORGPREP_NEXT", "NATIVECALL", "GETVARARGS",
    "DUPCLOSURE", "PREPVARARGS", "JUMPBACK", "LOADKX", "JUMPX", "FASTCALL",
    "COVERAGE", "CAPTURE", "SUBRK", "DIVRK", "FASTCALL1", "FASTCALL2", "FASTCALL2K",
    "FASTCALL3", "BREAK", "JUMPXEQKNIL", "JUMPXEQKB", "JUMPXEQKN", "JUMPXEQKS",
    "GETUDATAKS", "SETUDATAKS", "NAMECALLUDATA", "NEWCLASSMEMBER", "CMPPROTO",
    "FASTPCALL", "NEWCLASS",
];

/// Ordem do enum `LuauOpcode` na mesma release — é o byte que `luau-compile`
/// grava em `insn & 0xff`.
pub const OPS_ENUM: [&str; 91] = [
    "NOP", "BREAK", "LOADNIL", "LOADB", "LOADN", "LOADK", "MOVE", "GETGLOBAL",
    "SETGLOBAL", "GETUPVAL", "SETUPVAL", "CLOSEUPVALS", "GETIMPORT", "GETTABLE",
    "SETTABLE", "GETTABLEKS", "SETTABLEKS", "GETTABLEN", "SETTABLEN", "NEWCLOSURE",
    "NAMECALL", "CALL", "RETURN", "JUMP", "JUMPBACK", "JUMPIF", "JUMPIFNOT",
    "JUMPIFEQ", "JUMPIFLE", "JUMPIFLT", "JUMPIFNOTEQ", "JUMPIFNOTLE", "JUMPIFNOTLT",
    "ADD", "SUB", "MUL", "DIV", "MOD", "POW", "ADDK", "SUBK", "MULK", "DIVK",
    "MODK", "POWK", "AND", "OR", "ANDK", "ORK", "CONCAT", "NOT", "MINUS", "LENGTH",
    "NEWTABLE", "DUPTABLE", "SETLIST", "FORNPREP", "FORNLOOP", "FORGLOOP",
    "FORGPREP_INEXT", "FASTCALL3", "FORGPREP_NEXT", "NATIVECALL", "GETVARARGS",
    "DUPCLOSURE", "PREPVARARGS", "LOADKX", "JUMPX", "FASTCALL", "COVERAGE",
    "CAPTURE", "SUBRK", "DIVRK", "FASTCALL1", "FASTCALL2", "FASTCALL2K", "FORGPREP",
    "JUMPXEQKNIL", "JUMPXEQKB", "JUMPXEQKN", "JUMPXEQKS", "IDIV", "IDIVK",
    "GETUDATAKS", "SETUDATAKS", "NAMECALLUDATA", "NEWCLASSMEMBER", "CALLFB",
    "CMPPROTO", "FASTPCALL", "NEWCLASS",
];

/// Palavras que a instrução ocupa além da própria (`getOpLength`).
fn palavras_aux(nome: &str) -> usize {
    match nome {
        "GETGLOBAL" | "SETGLOBAL" | "GETIMPORT" | "GETTABLEKS" | "SETTABLEKS"
        | "NAMECALL" | "JUMPIFEQ" | "JUMPIFLE" | "JUMPIFLT" | "JUMPIFNOTEQ"
        | "JUMPIFNOTLE" | "JUMPIFNOTLT" | "NEWTABLE" | "SETLIST" | "FORGLOOP"
        | "LOADKX" | "FASTCALL2" | "FASTCALL2K" | "FASTCALL3" | "JUMPXEQKNIL"
        | "JUMPXEQKB" | "JUMPXEQKN" | "JUMPXEQKS" | "GETUDATAKS" | "SETUDATAKS"
        | "NAMECALLUDATA" | "NEWCLASSMEMBER" | "CALLFB" | "CMPPROTO" | "NEWCLASS" => 1,
        _ => 0,
    }
}

/// Tradução `opcode do compilador -> opcode do cliente`.
pub struct Mapa {
    pub por_enum: [u8; 91],
}

impl Mapa {
    /// Recebe os pares `(endereço do handler, byte do opcode)` lidos da tabela de
    /// despacho viva e casa a ordem dos endereços com a ordem do arquivo.
    pub fn das_entradas(entradas: &[(u64, u8)]) -> Result<Mapa, String> {
        if entradas.len() != OPS_ARQUIVO.len() {
            return Err(format!(
                "dispatch table has {} valid entries, expected {}",
                entradas.len(),
                OPS_ARQUIVO.len()
            ));
        }
        let mut ord = entradas.to_vec();
        ord.sort_unstable();
        let mut por_nome = std::collections::HashMap::new();
        for (i, (_, byte)) in ord.iter().enumerate() {
            por_nome.insert(OPS_ARQUIVO[i], *byte);
        }
        let mut por_enum = [0u8; 91];
        for (i, nome) in OPS_ENUM.iter().enumerate() {
            por_enum[i] = *por_nome
                .get(nome)
                .ok_or_else(|| format!("handler for {nome} is not in the table"))?;
        }
        Ok(Mapa { por_enum })
    }

    /// Reconstrói o mapa a partir da saída de `luaria-cli opcodes`
    /// (linhas "idx NOME -> 0xXX").
    pub fn do_texto(txt: &str) -> Result<Mapa, String> {
        let mut por_enum = [0u8; 91];
        let mut vistos = 0;
        for linha in txt.lines() {
            let mut campos = linha.split_whitespace();
            let (Some(indice), Some(_nome), Some(seta), Some(valor)) =
                (campos.next(), campos.next(), campos.next(), campos.next())
            else {
                continue;
            };
            if seta != "->" {
                continue;
            }
            let i: usize = indice.parse().map_err(|_| format!("invalid index: {indice}"))?;
            let v = valor.trim_start_matches("0x");
            por_enum[i] = u8::from_str_radix(v, 16).map_err(|_| format!("invalid value: {valor}"))?;
            vistos += 1;
        }
        if vistos != 91 {
            return Err(format!("incomplete map: {vistos} rows"));
        }
        Ok(Mapa { por_enum })
    }

    pub fn byte_de(&self, nome: &str) -> Option<u8> {
        OPS_ENUM.iter().position(|n| *n == nome).map(|i| self.por_enum[i])
    }
}

fn varint(d: &[u8], p: &mut usize) -> Result<u64, String> {
    let mut r = 0u64;
    let mut s = 0;
    loop {
        let b = *d.get(*p).ok_or("truncated bytecode")?;
        *p += 1;
        r |= ((b & 0x7f) as u64) << s;
        s += 7;
        if b & 0x80 == 0 {
            return Ok(r);
        }
        if s > 63 {
            return Err("varint grande demais".into());
        }
    }
}

fn u8_em(d: &[u8], p: &mut usize) -> Result<u8, String> {
    let b = *d.get(*p).ok_or("truncated bytecode")?;
    *p += 1;
    Ok(b)
}

fn u32_em(d: &[u8], p: usize) -> Result<u32, String> {
    d.get(p..p + 4)
        .map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
        .ok_or_else(|| "truncated bytecode".to_string())
}

fn pular(d: &[u8], p: &mut usize, n: usize) -> Result<(), String> {
    if *p + n > d.len() {
        return Err("truncated bytecode".into());
    }
    *p += n;
    Ok(())
}

/// Uma proto e o intervalo da sua seção de código dentro do chunk.
pub struct Proto {
    pub sizecode: usize,
    pub codigo: std::ops::Range<usize>,
    /// Índices (na mesma ordem de `percorrer`) das protos filhas. O dump grava
    /// o índice global de cada filha, não a posição dentro do `p[]`.
    pub filhos: Vec<usize>,
}

/// Dump decodificado: as protos em ordem e o índice da proto principal.
pub struct Chunk {
    pub protos: Vec<Proto>,
    pub principal: usize,
}

/// Percorre o chunk inteiro e devolve as protos, na ordem em que aparecem.
///
/// Espelha o leitor de `VM/src/lvmload.cpp` (versões 3..14) porque só assim dá
/// para saber onde cada array de instruções começa — as palavras auxiliares
/// (índices de constante) não têm opcode e não podem ser remapeadas.
pub fn percorrer(bytecode: &[u8]) -> Result<Vec<Proto>, String> {
    Ok(ler(bytecode)?.protos)
}

/// Índice da proto principal (a que `luau_load` empilha). Fica no último
/// varint do dump.
pub fn proto_principal(bytecode: &[u8]) -> Result<usize, String> {
    Ok(ler(bytecode)?.principal)
}

fn ler(bytecode: &[u8]) -> Result<Chunk, String> {
    let mut p = 0usize;
    let versao = u8_em(bytecode, &mut p)?;
    if !(3..=14).contains(&versao) {
        return Err(format!("bytecode version {versao} outside the supported range (3..14)"));
    }
    let tipos = if versao >= 4 { u8_em(bytecode, &mut p)? } else { 0 };
    let n_strings = varint(bytecode, &mut p)?;
    for _ in 0..n_strings {
        let n = varint(bytecode, &mut p)? as usize;
        pular(bytecode, &mut p, n)?;
    }
    if tipos == 3 {
        loop {
            let idx = u8_em(bytecode, &mut p)?;
            if idx == 0 {
                break;
            }
            varint(bytecode, &mut p)?;
        }
    }
    let n_protos = varint(bytecode, &mut p)?;
    let mut saida = Vec::with_capacity(n_protos as usize);
    for _ in 0..n_protos {
        let inicio_proto = p;
        let tam_proto = if versao >= 12 { varint(bytecode, &mut p)? as usize } else { 0 };
        // `lvmload` conta o tamanho a partir do primeiro byte do corpo: o varint
        // do tamanho não entra na conta.
        let inicio_corpo = p;
        pular(bytecode, &mut p, 4)?; // maxstacksize, numparams, nups, is_vararg
        let mut flags = 0;
        if versao >= 4 {
            flags = u8_em(bytecode, &mut p)?;
            // o compilador grava o tamanho do bloco de tipos em toda proto (0
            // quando não há tipos); o reader confere o tamanho e pula
            if tipos != 0 {
                let n = varint(bytecode, &mut p)? as usize;
                pular(bytecode, &mut p, n)?;
            }
        }
        let sizecode = varint(bytecode, &mut p)? as usize;
        let codigo = p..p + sizecode * 4;
        pular(bytecode, &mut p, sizecode * 4)?;
        let sizek = varint(bytecode, &mut p)?;
        for _ in 0..sizek {
            let tag = u8_em(bytecode, &mut p)?;
            match tag {
                0 => {}                                    // LBC_CONSTANT_NIL
                1 => pular(bytecode, &mut p, 1)?,           // boolean
                2 => pular(bytecode, &mut p, 8)?,           // number
                3 => { varint(bytecode, &mut p)?; }         // string
                4 => pular(bytecode, &mut p, 4)?,           // import
                5 => {                                      // table
                    let chaves = varint(bytecode, &mut p)?;
                    for _ in 0..chaves { varint(bytecode, &mut p)?; }
                }
                6 => { varint(bytecode, &mut p)?; }         // closure
                7 => pular(bytecode, &mut p, 16)?,          // vector
                8 => {                                      // table com constantes
                    let chaves = varint(bytecode, &mut p)?;
                    for _ in 0..chaves {
                        varint(bytecode, &mut p)?;
                        pular(bytecode, &mut p, 4)?;
                    }
                }
                9 => {                                      // integer
                    pular(bytecode, &mut p, 1)?;
                    varint(bytecode, &mut p)?;
                }
                10 => {                                     // class shape
                    varint(bytecode, &mut p)?;
                    let props = varint(bytecode, &mut p)?;
                    let mets = varint(bytecode, &mut p)?;
                    for _ in 0..props + mets { varint(bytecode, &mut p)?; }
                }
                11 => pular(bytecode, &mut p, 32)?,         // vector double
                outro => return Err(format!("unknown constant type {outro}")),
            }
        }
        let sizep = varint(bytecode, &mut p)?;
        let mut filhos = Vec::with_capacity(sizep as usize);
        for _ in 0..sizep {
            filhos.push(varint(bytecode, &mut p)? as usize);
        }
        varint(bytecode, &mut p)?; // linedefined
        varint(bytecode, &mut p)?; // debugname
        let tem_lineinfo = u8_em(bytecode, &mut p)?;
        if tem_lineinfo != 0 {
            let gap = u8_em(bytecode, &mut p)? as usize;
            // um delta por instrução
            pular(bytecode, &mut p, sizecode)?;
            let intervalos = if sizecode == 0 { 1 } else { ((sizecode - 1) >> gap) + 1 };
            pular(bytecode, &mut p, intervalos * 4)?;
        }
        let tem_debug = u8_em(bytecode, &mut p)?;
        if tem_debug != 0 {
            let n_locais = varint(bytecode, &mut p)?;
            for _ in 0..n_locais {
                varint(bytecode, &mut p)?;
                varint(bytecode, &mut p)?;
                varint(bytecode, &mut p)?;
                pular(bytecode, &mut p, 1)?;
            }
            let n_up = varint(bytecode, &mut p)?;
            for _ in 0..n_up {
                varint(bytecode, &mut p)?;
            }
        }
        if versao >= 11 {
            let n_fb = varint(bytecode, &mut p)?;
            for _ in 0..n_fb {
                pular(bytecode, &mut p, 1)?; // LFT_CALLTARGET
                varint(bytecode, &mut p)?;   // pc
            }
        }
        if versao >= 12 {
            // LPF_INLINABLE: o compilador grava o custo depois das linhas
            if flags & 8 != 0 {
                varint(bytecode, &mut p)?;
            }
            let fim = inicio_corpo + tam_proto;
            if p > fim {
                return Err(format!(
                    "proto inconsistente: li {:#x} bytes, o chunk declara {:#x}",
                    p - inicio_corpo,
                    tam_proto
                ));
            }
            p = fim;
        }
        let _ = inicio_proto;
        saida.push(Proto { sizecode, codigo, filhos });
    }
    let principal = varint(bytecode, &mut p)? as usize;
    if principal >= saida.len() {
        return Err(format!("main proto {principal} outside the chunk"));
    }
    Ok(Chunk { protos: saida, principal })
}

/// Bytes crus da seção de código do proto `indice`.
pub fn code_section(bytecode: &[u8], indice: usize) -> Option<Vec<u8>> {
    let protos = percorrer(bytecode).ok()?;
    let pr = protos.get(indice)?;
    bytecode.get(pr.codigo.clone()).map(|b| b.to_vec())
}

/// Troca o byte de opcode de cada instrução pelos valores do cliente.
pub fn remapear(bytecode: &[u8], mapa: &Mapa) -> Result<Vec<u8>, String> {
    let mut d = bytecode.to_vec();
    for proto in percorrer(bytecode)? {
        let base = proto.codigo.start;
        // anda pelo tamanho da instrução: palavra auxiliar não é opcode
        let mut i = 0usize;
        while i < proto.sizecode {
            let pos = base + i * 4;
            let insn = u32_em(&d, pos)?;
            let antigo = (insn & 0xff) as usize;
            if antigo >= mapa.por_enum.len() {
                return Err(format!("opcode {antigo} outside the map (instruction {i})"));
            }
            let nome = OPS_ENUM[antigo];
            d[pos] = mapa.por_enum[antigo];
            i += 1 + palavras_aux(nome);
        }
    }
    Ok(d)
}

#[cfg(test)]
mod testes {
    use super::*;

    #[test]
    fn listas_tem_91() {
        assert_eq!(OPS_ARQUIVO.len(), 91);
        assert_eq!(OPS_ENUM.len(), 91);
    }

    #[test]
    fn mesmos_nomes_nas_duas_listas() {
        let mut a: Vec<&str> = OPS_ARQUIVO.to_vec();
        let mut b: Vec<&str> = OPS_ENUM.to_vec();
        a.sort_unstable();
        b.sort_unstable();
        assert_eq!(a, b);
    }
}
