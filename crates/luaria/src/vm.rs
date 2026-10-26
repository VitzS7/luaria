//! Resolução das âncoras da VM Luau dentro do payload.
//!
//! Nada aqui é offset fixo. O método:
//!   1. string única em `.rodata` -> xref -> função que a usa;
//!   2. `lua_resume` sai da string "cannot resume dead coroutine";
//!   3. `luau_load` sai do **wrapper** que formata o erro de versão de bytecode:
//!      o wrapper chama `luau_load(L, nome, dados, tam, env)`;
//!   4. `luaG_typeerror` sai da string "attempt to index %s with '%s'".
//!
//! Descoberta: a função que referencia a string de
//! "bytecode type version mismatch" NÃO é o carregador — é um formatador de
//! mensagem que recebe um descritor. O carregador é o alvo da chamada de 5
//! argumentos dentro dele.

use std::collections::BTreeMap;

use crate::elf::Elf;
use crate::xref::XrefTable;
use crate::scan;

#[derive(Debug, Default, Clone)]
pub struct Anchors {
    pub luau_load: Option<u64>,
    pub luau_load_wrapper: Option<u64>,
    pub lua_g_typeerror: Option<u64>,
    pub lua_resume: Option<u64>,
    pub lua_d_call: Option<u64>,
    /// funções da biblioteca base que conseguimos nomear (informativo)
    pub base_lib: BTreeMap<String, u64>,
}

const STR_LOAD: &str = "%s: bytecode type version mismatch (expected [%d..%d], got %d)";
const STR_TYPEERROR: &str = "attempt to index %s with '%s'";
const STR_RESUME: &str = "cannot resume dead coroutine";

pub fn resolve(elf: &Elf) -> Anchors {
    let tabela = XrefTable::build(elf);
    resolve_com(elf, &tabela)
}

pub fn resolve_com(elf: &Elf, tabela: &XrefTable) -> Anchors {
    let mut a = Anchors::default();

    // --- âncoras por string + xref ------------------------------------------
    a.lua_g_typeerror = por_string(elf, tabela, STR_TYPEERROR);
    a.lua_resume = por_string(elf, tabela, STR_RESUME);

    // --- luau_load: wrapper -> chamada de 5 argumentos ----------------------
    if let Some((load, wrapper)) = resolve_luau_load(elf, tabela) {
        a.luau_load = Some(load);
        a.luau_load_wrapper = Some(wrapper);
    }

    a.lua_d_call = resolve_lua_d_call(elf);

    a
}

/// `luaD_call` incrementa `L->nCcalls` (+0x58, u16) e compara com 200
/// (`LUAI_MAXCCALLS`). A sequência de bytes é específica o bastante para achar
/// a função sem símbolo.
pub fn resolve_lua_d_call(elf: &Elf) -> Option<u64> {
    const PADRAO: [u8; 9] = [0x8B, 0x47, 0x58, 0xFF, 0xC0, 0x66, 0x89, 0x47, 0x58];
    let idx = crate::eh_frame::Indice::novo(elf);
    for hit in scan::find_all(&elf.data, &PADRAO) {
        if let Some(va) = elf.off_to_va(hit as u64) {
            if let Some(f) = idx.inicio(va) {
                return Some(f);
            }
        }
    }
    None
}

fn por_string(elf: &Elf, tabela: &XrefTable, s: &str) -> Option<u64> {
    let va = scan::find_cstr(elf, s).first().copied()?;
    // fica com a função cujo xref está mais perto do início
    let mut melhor: Option<(u64, u64)> = None; // (distancia, inicio)
    for r in tabela.referencias(va) {
        if let Some(f) = scan::find_fn_start(elf, r) {
            let d = r.saturating_sub(f);
            if melhor.map(|(md, _)| d < md).unwrap_or(true) {
                melhor = Some((d, f));
            }
        }
    }
    melhor.map(|(_, f)| f)
}

/// Acha o `luau_load` real.
///
/// Caminho medido neste payload: a string do erro de versão de bytecode aparece
/// dentro de um wrapper que recebe um descritor (`{nome, dados, tam, env}`), e é
/// o wrapper que chama `luau_load(L, nome, dados, tam, env)`.
pub fn resolve_luau_load(elf: &Elf, tabela: &XrefTable) -> Option<(u64, u64)> {
    let s = scan::find_cstr(elf, STR_LOAD).first().copied()?;
    let mut wrapper: Option<u64> = None;
    for r in tabela.referencias(s) {
        if let Some(f) = scan::find_fn_start(elf, r) {
            wrapper = Some(match wrapper {
                Some(atual) => {
                    if r.saturating_sub(f) < r.saturating_sub(atual) {
                        f
                    } else {
                        atual
                    }
                }
                None => f,
            });
        }
    }
    let wrapper = wrapper?;
    for (site, destino) in scan::direct_calls(elf, wrapper, 0x2000) {
        if !elf.in_exec(destino) {
            continue;
        }
        if recebe_cinco_args(elf, site) {
            return Some((destino, wrapper));
        }
    }
    None
}

/// Confere se a instrução em `site` é precedida pela montagem de 5 argumentos
/// (rsi/rdx/rcx/r8 escritos antes da chamada).
fn recebe_cinco_args(elf: &Elf, site: u64) -> bool {
    use iced_x86::{Mnemonic, OpKind, Register};
    let ini = site.saturating_sub(0x80);
    let bytes = match elf.bytes_at(ini, (site - ini) as usize) {
        Some(b) => b,
        None => return false,
    };
    let instrs = scan::decode_bytes(bytes, ini);
    let (mut rsi, mut rdx, mut rcx, mut r8) = (false, false, false, false);
    for i in instrs.iter().rev().take(16) {
        if i.mnemonic() == Mnemonic::Call || i.mnemonic() == Mnemonic::Jmp {
            break;
        }
        for k in 0..i.op_count() {
            if i.op_kind(k) != OpKind::Register {
                continue;
            }
            match if k == 0 { i.op0_register() } else { i.op1_register() } {
                Register::RSI => rsi = true,
                Register::RDX => rdx = true,
                Register::RCX => rcx = true,
                Register::R8 | Register::R8D => r8 = true,
                _ => {}
            }
        }
    }
    rsi && rdx && rcx && r8
}

impl Anchors {
    pub fn resumo(&self) -> String {
        let mut s = String::new();
        let campo = |n: &str, v: Option<u64>| match v {
            Some(x) => format!("{n:<20} +0x{x:x}\n"),
            None => format!("{n:<20} NOT FOUND\n"),
        };
        s.push_str(&campo("luau_load", self.luau_load));
        s.push_str(&campo("luau_load_wrapper", self.luau_load_wrapper));
        s.push_str(&campo("luaG_typeerror", self.lua_g_typeerror));
        s.push_str(&campo("lua_resume", self.lua_resume));
        s.push_str(&campo("luaD_call", self.lua_d_call));
        s.push_str(&format!("base lib: {} functions\n", self.base_lib.len()));
        s
    }

    pub fn to_json(&self) -> serde_json::Value {
        let hex = |v: Option<u64>| match v {
            Some(x) => serde_json::Value::String(format!("0x{x:x}")),
            None => serde_json::Value::Null,
        };
        let mut base = serde_json::Map::new();
        for (k, v) in &self.base_lib {
            base.insert(k.clone(), serde_json::Value::String(format!("0x{v:x}")));
        }
        serde_json::json!({
            "luau_load": hex(self.luau_load),
            "luau_load_wrapper": hex(self.luau_load_wrapper),
            "lua_g_typeerror": hex(self.lua_g_typeerror),
            "lua_resume": hex(self.lua_resume),
            "luaD_call": hex(self.lua_d_call),
            "base_lib": base,
        })
    }
}
