//! Layout do anel de comunicação host→jogo (região mmap RW compartilhada).
//!
//! Por que um anel com slots e não uma chamada remota por requisição: o jogo
//! não pode ser chamado de dentro de `luaD_call` sem reentrar, então ele só
//! escreve o pedido na memória e o host atende fora dessa chamada. Este módulo
//! só define os offsets e a máquina de estados; não toca em memória nem em
//! processo.

/// Assinatura gravada no início da região, para o jogo reconhecer o anel.
pub const MAGIC: &[u8; 8] = b"SOBRPNT1";
pub const VERSAO: u32 = 1;
pub const TAM_ANEL: usize = 0x401000;
pub const N_SLOTS: usize = 8;

pub const OFF_MAGIC: usize = 0;
pub const OFF_VERSAO: usize = 8;
/// Host incrementa a cada passada do loop de atendimento; o jogo só observa.
pub const OFF_H_PULSO: usize = 0xc;

pub const OFF_SLOTS: usize = 0x40;
pub const TAM_CAB_SLOT: usize = 0x40;

// Campos escritos pelo jogo.
pub const SLOT_DONO: usize = 0x0;
pub const SLOT_G_REQ: usize = 0x4;
pub const SLOT_G_CONS: usize = 0x8;
pub const SLOT_TIPO: usize = 0xc;
pub const SLOT_LEN_REQ: usize = 0x10;
pub const SLOT_FLAGS: usize = 0x14;
// Campos escritos pelo host.
pub const SLOT_H_ACK: usize = 0x20;
pub const SLOT_H_RESP: usize = 0x24;
pub const SLOT_STATUS: usize = 0x28;
pub const SLOT_LEN_RESP: usize = 0x2c;

pub const OFF_DADOS: usize = 0x1000;
pub const TAM_SLOT_DADOS: usize = 0x80000;
pub const MAX_REQ: usize = 0x10000;
pub const MAX_RESP: usize = 0x70000;

pub const TIPO_HTTP: u32 = 1;
pub const TIPO_WS: u32 = 2;
pub const TIPO_CLIP_ESCREVER: u32 = 3;
pub const TIPO_CLIP_LER: u32 = 4;
pub const TIPO_CONSOLE: u32 = 5;
pub const TIPO_FILA_TELEPORT: u32 = 6;

pub const FLAG_SEM_RESPOSTA: u32 = 1;

pub fn off_cab_slot(i: usize) -> usize {
    OFF_SLOTS + i * TAM_CAB_SLOT
}

pub fn off_req(i: usize) -> usize {
    OFF_DADOS + i * TAM_SLOT_DADOS
}

pub fn off_resp(i: usize) -> usize {
    off_req(i) + MAX_REQ
}

/// Handle devolvido ao Lua: `seq` e índice do slot empacotados num f64.
/// Por que `seq*8 + slot`: cabe em f64 exato enquanto seq < 2^50, e o slot sai do `% 8`.
pub fn handle(seq: u32, slot: usize) -> f64 {
    (seq as f64) * 8.0 + slot as f64
}

pub fn separar_handle(h: f64) -> (u32, usize) {
    let n = h as u64;
    ((n / 8) as u32, (n % 8) as usize)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EstadoSlot {
    Livre,
    Pendente,
    EmAtendimento,
    Pronta,
}

/// Ordem de checagem torna os estados exclusivos: primeiro se a resposta já
/// chegou (`h_resp == g_req`), depois o ack. Assim uma resposta entregue
/// vence um ack ainda não visto, caso que o host nunca produz (ele faz ack antes).
pub fn estado(g_req: u32, g_cons: u32, h_ack: u32, h_resp: u32) -> EstadoSlot {
    if h_resp == g_req {
        return if g_cons == g_req {
            EstadoSlot::Livre
        } else {
            EstadoSlot::Pronta
        };
    }
    if g_req != h_ack {
        EstadoSlot::Pendente
    } else {
        EstadoSlot::EmAtendimento
    }
}

/// Slot pode receber nova requisição: a anterior foi respondida e consumida.
pub fn pode_reusar(g_req: u32, g_cons: u32, h_resp: u32) -> bool {
    h_resp == g_req && g_cons == g_req
}

#[cfg(test)]
mod testes {
    use super::*;

    #[test]
    fn slot_cabe_em_512_kb() {
        assert!(TAM_SLOT_DADOS <= 512 * 1024);
        assert!(MAX_REQ + MAX_RESP <= TAM_SLOT_DADOS);
    }

    #[test]
    fn anel_comporta_slots_e_dados() {
        assert!(OFF_DADOS + N_SLOTS * TAM_SLOT_DADOS <= TAM_ANEL);
        assert_eq!(off_req(N_SLOTS - 1) + TAM_SLOT_DADOS, TAM_ANEL);
    }

    #[test]
    fn cabecalhos_nao_sobrepoem_dados_nem_entre_si() {
        assert!(OFF_MAGIC + MAGIC.len() <= OFF_VERSAO);
        assert!(OFF_VERSAO + 4 <= OFF_H_PULSO);
        assert!(OFF_H_PULSO + 4 <= OFF_SLOTS);
        assert!(off_cab_slot(N_SLOTS - 1) + TAM_CAB_SLOT <= OFF_DADOS);
        for i in 1..N_SLOTS {
            assert!(off_cab_slot(i - 1) + TAM_CAB_SLOT <= off_cab_slot(i));
        }
    }

    #[test]
    fn campos_do_slot_cabem_e_nao_se_sobrepoem() {
        let campos = [
            SLOT_DONO,
            SLOT_G_REQ,
            SLOT_G_CONS,
            SLOT_TIPO,
            SLOT_LEN_REQ,
            SLOT_FLAGS,
            SLOT_H_ACK,
            SLOT_H_RESP,
            SLOT_STATUS,
            SLOT_LEN_RESP,
        ];
        for (i, a) in campos.iter().enumerate() {
            assert!(a + 4 <= TAM_CAB_SLOT);
            for b in &campos[i + 1..] {
                assert!(a + 4 <= *b || b + 4 <= *a, "campos {a:#x} e {b:#x} se sobrepõem");
            }
        }
    }

    #[test]
    fn handle_ida_e_volta() {
        for &seq in &[0u32, 1, 2, 1000, u32::MAX - 1, u32::MAX] {
            for slot in 0..N_SLOTS {
                assert_eq!(separar_handle(handle(seq, slot)), (seq, slot));
            }
        }
    }

    #[test]
    fn handle_exato_em_f64() {
        assert!((u32::MAX as f64) * 8.0 + 7.0 < 2f64.powi(53));
    }

    #[test]
    fn estado_nos_quatro_casos() {
        assert_eq!(estado(5, 5, 5, 5), EstadoSlot::Livre);
        assert_eq!(estado(5, 4, 5, 5), EstadoSlot::Pronta);
        assert_eq!(estado(5, 4, 4, 4), EstadoSlot::Pendente);
        assert_eq!(estado(5, 4, 5, 4), EstadoSlot::EmAtendimento);
    }

    #[test]
    fn estado_com_wrap_do_u32() {
        let seq = u32::MAX;
        let prox = seq.wrapping_add(1);
        assert_eq!(estado(prox, prox, prox, prox), EstadoSlot::Livre);
        assert_eq!(estado(prox, seq, seq, seq), EstadoSlot::Pendente);
        assert_eq!(estado(prox, seq, prox, seq), EstadoSlot::EmAtendimento);
        assert_eq!(estado(prox, seq, prox, prox), EstadoSlot::Pronta);
    }

    #[test]
    fn pode_reusar_so_quando_consumido() {
        assert!(pode_reusar(7, 7, 7));
        assert!(!pode_reusar(7, 6, 7));
        assert!(!pode_reusar(7, 7, 6));
        assert!(pode_reusar(0, 0, 0));
    }
}
