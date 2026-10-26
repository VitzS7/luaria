//! Importações dinâmicas: `.dynsym` + `.rela.plt` e resolução de stub PLT.
//!
//! Serve para reconhecer chamadas a funções de biblioteca (`setjmp`, `longjmp`,
//! `pthread_create`...) sem símbolos estáticos: o payload só exporta JNI, mas os
//! imports continuam nomeados na tabela dinâmica.

use std::collections::HashMap;

use crate::elf::{Elf, Section};

#[derive(Debug, Default)]
pub struct Imports {
    /// GOT (VA do slot) -> índice em `.dynsym`
    got_para_simbolo: HashMap<u64, usize>,
    pub nomes: Vec<String>,
    /// Stub PLT (VA) -> nome do símbolo
    pub plt: HashMap<u64, String>,
}

fn cstr(d: &[u8], off: usize) -> String {
    let end = d[off..].iter().position(|&c| c == 0).unwrap_or(0);
    String::from_utf8_lossy(&d[off..off + end]).to_string()
}

impl Imports {
    pub fn load(elf: &Elf) -> Imports {
        let mut imp = Imports::default();
        let dynsym = match elf.section(".dynsym") {
            Some(s) => s,
            None => return imp,
        };
        let dynstr = match elf.section(".dynstr") {
            Some(s) => s,
            None => return imp,
        };
        let n = (dynsym.size / 24) as usize;
        for i in 0..n {
            let o = (dynsym.offset + i as u64 * 24) as usize;
            if o + 24 > elf.data.len() {
                break;
            }
            let st_name = u32::from_le_bytes(elf.data[o..o + 4].try_into().unwrap()) as usize;
            imp.nomes.push(cstr(&elf.data, dynstr.offset as usize + st_name));
        }

        if let Some(rela) = elf.section(".rela.plt") {
            if dynsym.link as usize <= elf.sections.len() {
                let cnt = (rela.size / 24) as usize;
                for i in 0..cnt {
                    let o = (rela.offset + i as u64 * 24) as usize;
                    if o + 24 > elf.data.len() {
                        break;
                    }
                    let r_offset = u64::from_le_bytes(elf.data[o..o + 8].try_into().unwrap());
                    let r_info = u64::from_le_bytes(elf.data[o + 8..o + 16].try_into().unwrap());
                    let sym = (r_info >> 32) as usize;
                    imp.got_para_simbolo.insert(r_offset, sym);
                }
            }
        }

        // Cada stub de `.plt` é um salto indireto para a entrada do GOT.
        if let Some(plt) = elf.section(".plt") {
            let mut va = plt.addr;
            let fim = plt.addr + plt.size;
            while va < fim {
                if let Some(nome) = imp.simbolo_do_stub(elf, va) {
                    imp.plt.insert(va, nome);
                }
                va += 16;
            }
        }
        imp
    }

    /// Lê o stub em `stub`, acha o slot GOT e devolve o nome importado.
    fn simbolo_do_stub(&self, elf: &Elf, stub: u64) -> Option<String> {
        let b = elf.bytes_at(stub, 16)?;
        // pula endbr64
        let i = if b[0] == 0xF3 && b[1] == 0x0F && b[2] == 0x1E && b[3] == 0xFA { 4 } else { 0 };
        if b[i] != 0xFF || b[i + 1] != 0x25 {
            return None;
        }
        let disp = i32::from_le_bytes(b[i + 2..i + 6].try_into().ok()?);
        let got = (stub + i as u64 + 6).wrapping_add(disp as i64 as u64);
        let sym = *self.got_para_simbolo.get(&got)?;
        self.nomes.get(sym).cloned()
    }

    /// Nome do import que a instrução em `instr_va` chama, se for um stub PLT.
    pub fn chamada(&self, destino: u64) -> Option<&str> {
        self.plt.get(&destino).map(|s| s.as_str())
    }

    pub fn stub(&self, nome: &str) -> Option<u64> {
        self.plt.iter().find(|(_, n)| n.as_str() == nome).map(|(a, _)| *a)
    }
}

#[allow(dead_code)]
fn _unused(_: &Section) {}
