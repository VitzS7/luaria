//! Tabela de referências: uma única passada de desmontagem sobre todo o `.text`,
//! com as referências ordenadas por alvo.
//!
//! Uma passada custa ~8 s neste payload (82 MB de código), então ela é feita uma
//! vez e consultada por busca binária. Gráfico de chamadas e xrefs de string saem
//! da mesma tabela.

use iced_x86::{Decoder, DecoderOptions, FlowControl, OpKind};

use crate::elf::Elf;

#[derive(Debug, Default)]
pub struct XrefTable {
    /// (alvo, origem) ordenado por alvo — chamadas diretas e saltos incondicionais
    pub chamadas: Vec<(u64, u64)>,
    /// (alvo, origem) ordenado por alvo — operandos RIP-relativos e imediatos 64-bit
    pub dados: Vec<(u64, u64)>,
}

impl XrefTable {
    pub fn build(elf: &Elf) -> XrefTable {
        let mut t = XrefTable::default();
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
                if instr.is_ip_rel_memory_operand() {
                    t.dados.push((instr.ip_rel_memory_address(), ip));
                    continue;
                }
                let fc = instr.flow_control();
                if (fc == FlowControl::Call || fc == FlowControl::UnconditionalBranch)
                    && instr.op0_kind() == OpKind::NearBranch64
                {
                    t.chamadas.push((instr.near_branch64(), ip));
                } else if instr.op0_kind() == OpKind::Immediate64 && instr.op1_kind() == OpKind::Register {
                    t.dados.push((instr.immediate64(), ip));
                }
            }
        }
        t.chamadas.sort_unstable();
        t.dados.sort_unstable();
        t
    }

    fn faixa(v: &[(u64, u64)], alvo: u64) -> &[(u64, u64)] {
        let ini = v.partition_point(|(t, _)| *t < alvo);
        let fim = v.partition_point(|(t, _)| *t <= alvo);
        &v[ini..fim]
    }

    /// Origens que chamam/saltam para `alvo`.
    pub fn chamadores(&self, alvo: u64) -> Vec<u64> {
        Self::faixa(&self.chamadas, alvo).iter().map(|(_, f)| *f).collect()
    }

    /// Origens que referenciam `alvo` como dado (rip-relativo ou imediato).
    pub fn referencias(&self, alvo: u64) -> Vec<u64> {
        Self::faixa(&self.dados, alvo).iter().map(|(_, f)| *f).collect()
    }

    pub fn total(&self) -> (usize, usize) {
        (self.chamadas.len(), self.dados.len())
    }
}
