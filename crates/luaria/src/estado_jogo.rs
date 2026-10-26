//! Lógica pura da troca de jogo: quando reconectar e quais `lua_State`s ainda
//! valem a pena lembrar. Fica separada do motor para testar sem cliente.

use std::collections::HashMap;
use std::time::{Duration, Instant};

/// Quanto tempo um `lua_State` rejeitado (LuaApp) fica na lista. Ao trocar de
/// lugar o cliente libera estados e o alocador reaproveita o endereço, então
/// lembrar para sempre acabaria recusando um estado de jogo legítimo.
pub const VALIDADE_REJEICAO: Duration = Duration::from_secs(20);

/// Teto da lista: acima disso o cliente está no menu e vale parar de insistir.
pub const MAX_REJEITADOS: usize = 25;

/// O que identifica uma instância do cliente. `inicio` vem do `starttime` de
/// `/proc/<pid>/stat`: o kernel reaproveita pid, então só o pid não basta.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Identidade {
    pub pid: i32,
    pub inicio: u64,
    pub base: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Motivo {
    PidNovo,
    ProcessoReaproveitado,
    PayloadRemapeado,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Decisao {
    Manter,
    Reconectar(Motivo),
    /// O processo conhecido morreu e não há outro no lugar.
    ClienteSumiu,
}

/// Compara a identidade que o motor guardou com a que existe agora.
/// `atual` é a leitura de `/proc` do pid conhecido; `substituto` é um cliente
/// achado por varredura quando o conhecido não existe mais.
pub fn decidir(
    conhecida: &Identidade,
    atual: Option<&Identidade>,
    substituto: Option<&Identidade>,
) -> Decisao {
    match atual {
        Some(a) if a.pid == conhecida.pid && a.inicio != conhecida.inicio => {
            Decisao::Reconectar(Motivo::ProcessoReaproveitado)
        }
        Some(a) if a.base != conhecida.base => Decisao::Reconectar(Motivo::PayloadRemapeado),
        Some(_) => Decisao::Manter,
        None => match substituto {
            Some(_) => Decisao::Reconectar(Motivo::PidNovo),
            None => Decisao::ClienteSumiu,
        },
    }
}

/// Slots que o `luau_load` + a sonda empurram antes da execução; o resto é folga.
pub const SLOTS_CARGA: u64 = 4;
/// Folga extra somada ao `maxstacksize` do proto antes de executar.
pub const MARGEM_EXEC_SLOTS: u64 = 8;
/// `luaD_call` lança "C stack overflow" em 200 (0xc8).
pub const MAX_NCCALLS: u16 = 180;

/// Slots livres entre `top` e `stack_last`. `None` se o estado é incoerente.
pub fn slots_livres(stack: u64, top: u64, stack_last: u64) -> Option<u64> {
    if stack == 0 || stack_last < stack || top < stack || top > stack_last {
        return None;
    }
    Some((stack_last - top) / 16)
}

/// Por que não crescer a pilha: `luaD_precall` chama `luaD_growstack` quando a
/// folga não cobre `maxstacksize`, e o growstack lança erro Lua sem proteção em
/// chamada remota, derrubando a thread. O `luau_load` também empilha sem checar.
pub fn checar_folga(
    stack: u64,
    top: u64,
    stack_last: u64,
    ncalls: u16,
    necessarios: u64,
) -> Result<u64, String> {
    let Some(livres) = slots_livres(stack, top, stack_last) else {
        return Err(format!("inconsistent stack (top={top:#x} stack_last={stack_last:#x})"));
    };
    if ncalls >= MAX_NCCALLS {
        return Err(format!("nCcalls={ncalls} near the limit of {MAX_NCCALLS}"));
    }
    if livres <= necessarios {
        return Err(format!("{livres} free slots, needs more than {necessarios}"));
    }
    Ok(livres)
}

/// Memória do que já foi visto nesta conexão.
#[derive(Debug, Default)]
pub struct Estados {
    rejeitados: HashMap<u64, Instant>,
    /// VMs (`lua_State` de thread de app) rejeitadas; mesma validade que `rejeitados`.
    vms_app: HashMap<u64, Instant>,
    place_id: Option<u64>,
    novos: u32,
    repetidos: u32,
}

impl Estados {
    pub fn rejeitar(&mut self, l: u64, agora: Instant) {
        self.rejeitados.insert(l, agora);
        self.novos += 1;
    }

    pub fn rejeitar_vm(&mut self, g: u64, agora: Instant) {
        self.vms_app.insert(g, agora);
    }

    pub fn vm_rejeitada(&mut self, g: u64, agora: Instant) -> bool {
        self.vms_app
            .retain(|_, desde| agora.saturating_duration_since(*desde) < VALIDADE_REJEICAO);
        self.vms_app.contains_key(&g)
    }

    pub fn foi_rejeitado(&mut self, l: u64, agora: Instant) -> bool {
        self.descartar_vencidos(agora);
        let visto = self.rejeitados.contains_key(&l);
        if visto {
            self.repetidos += 1;
        }
        visto
    }

    /// (estados novos rejeitados, reencontros de estado já rejeitado). Se os
    /// reencontros dominam, o freio está pegando sempre as mesmas threads do app.
    pub fn contadores(&self) -> (u32, u32) {
        (self.novos, self.repetidos)
    }

    pub fn place(&self) -> Option<u64> {
        self.place_id
    }

    /// Só viu LuaApp: rejeitou algum estado e nunca viu PlaceId. Fica false
    /// depois do primeiro PlaceId, até `limpar()`.
    pub fn so_menu(&self) -> bool {
        self.place_id.is_none() && self.novos > 0
    }

    pub fn quantidade(&self) -> usize {
        self.rejeitados.len()
    }

    /// Poda os vencidos antes de contar: sem isso entradas velhas de LuaApp
    /// somavam com as novas e davam "esgotou" com poucos estados vivos.
    pub fn esgotou(&mut self, agora: Instant) -> bool {
        self.descartar_vencidos(agora);
        self.rejeitados.len() >= MAX_REJEITADOS
    }

    pub fn amostra(&self, n: usize) -> Vec<u64> {
        self.rejeitados.keys().copied().take(n).collect()
    }

    pub fn limpar(&mut self) {
        *self = Self::default();
    }

    /// Registra o PlaceId visto num estado de jogo. Mudou = trocou de lugar:
    /// todo `lua_State` lembrado pertence ao DataModel antigo.
    pub fn observar_place(&mut self, place: u64) -> bool {
        let trocou = self.place_id.is_some_and(|p| p != place);
        if trocou {
            self.rejeitados.clear();
            self.vms_app.clear();
        }
        self.place_id = Some(place);
        trocou
    }

    fn descartar_vencidos(&mut self, agora: Instant) {
        self.rejeitados
            .retain(|_, desde| agora.saturating_duration_since(*desde) < VALIDADE_REJEICAO);
    }
}

#[cfg(test)]
mod testes {
    use super::*;

    fn id(pid: i32, inicio: u64, base: u64) -> Identidade {
        Identidade { pid, inicio, base }
    }

    #[test]
    fn mesmo_processo_mantem() {
        let a = id(10, 500, 0x7000);
        assert_eq!(decidir(&a, Some(&a), None), Decisao::Manter);
    }

    #[test]
    fn pid_reaproveitado_reconecta() {
        let a = id(10, 500, 0x7000);
        let b = id(10, 900, 0x7000);
        assert_eq!(
            decidir(&a, Some(&b), None),
            Decisao::Reconectar(Motivo::ProcessoReaproveitado)
        );
    }

    #[test]
    fn base_diferente_reconecta() {
        let a = id(10, 500, 0x7000);
        let b = id(10, 500, 0x9000);
        assert_eq!(
            decidir(&a, Some(&b), None),
            Decisao::Reconectar(Motivo::PayloadRemapeado)
        );
    }

    #[test]
    fn processo_morto_com_substituto_reconecta() {
        let a = id(10, 500, 0x7000);
        let novo = id(77, 800, 0x8000);
        assert_eq!(decidir(&a, None, Some(&novo)), Decisao::Reconectar(Motivo::PidNovo));
    }

    #[test]
    fn processo_morto_sem_substituto_sumiu() {
        let a = id(10, 500, 0x7000);
        assert_eq!(decidir(&a, None, None), Decisao::ClienteSumiu);
    }

    #[test]
    fn rejeicao_vence_com_o_tempo() {
        let t0 = Instant::now();
        let mut e = Estados::default();
        e.rejeitar(0x1000, t0);
        assert!(e.foi_rejeitado(0x1000, t0 + Duration::from_secs(5)));
        assert!(!e.foi_rejeitado(0x1000, t0 + VALIDADE_REJEICAO + Duration::from_secs(1)));
        assert_eq!(e.quantidade(), 0);
    }

    #[test]
    fn troca_de_place_descarta_estados_antigos() {
        let t0 = Instant::now();
        let mut e = Estados::default();
        assert!(!e.observar_place(111));
        e.rejeitar(0x2000, t0);
        assert!(!e.observar_place(111));
        assert!(e.foi_rejeitado(0x2000, t0));
        assert!(e.observar_place(222));
        assert!(!e.foi_rejeitado(0x2000, t0));
    }

    #[test]
    fn esgota_no_limite() {
        let t0 = Instant::now();
        let mut e = Estados::default();
        for i in 0..MAX_REJEITADOS as u64 {
            assert!(!e.esgotou(t0));
            e.rejeitar(0x1000 + i * 8, t0);
        }
        assert!(e.esgotou(t0));
        e.limpar();
        assert!(!e.esgotou(t0));
    }

    #[test]
    fn vencidos_nao_contam_para_esgotar() {
        let t0 = Instant::now();
        let mut e = Estados::default();
        for i in 0..MAX_REJEITADOS as u64 {
            e.rejeitar(0x1000 + i * 8, t0);
        }
        assert!(e.esgotou(t0));
        assert!(!e.esgotou(t0 + VALIDADE_REJEICAO + Duration::from_secs(1)));
    }

    #[test]
    fn so_menu_ate_ver_um_jogo() {
        let t0 = Instant::now();
        let mut e = Estados::default();
        assert!(!e.so_menu());
        e.rejeitar(0x1000, t0);
        assert!(e.so_menu());
        assert_eq!(e.place(), None);
        e.observar_place(42);
        assert!(!e.so_menu());
        assert_eq!(e.place(), Some(42));
    }

    #[test]
    fn vm_rejeitada_vale_ate_a_validade() {
        let t0 = Instant::now();
        let mut e = Estados::default();
        e.rejeitar_vm(0x3000, t0);
        assert!(e.vm_rejeitada(0x3000, t0 + Duration::from_secs(5)));
        assert!(!e.vm_rejeitada(0x3000, t0 + VALIDADE_REJEICAO + Duration::from_secs(1)));
    }

    #[test]
    fn troca_de_place_limpa_vms() {
        let t0 = Instant::now();
        let mut e = Estados::default();
        e.observar_place(111);
        e.rejeitar_vm(0x3000, t0);
        assert!(e.observar_place(222));
        assert!(!e.vm_rejeitada(0x3000, t0));
    }

    /// Pilha com `livres` slots entre `top` e `stack_last`.
    fn pilha_com(livres: u64) -> (u64, u64, u64) {
        let stack = 0x1000;
        let stack_last = stack + 16 * (livres + 100);
        (stack, stack + 16 * 100, stack_last)
    }

    #[test]
    fn carga_com_nativas_exige_mais_de_7_livres() {
        let (s, t, l) = pilha_com(7);
        assert!(checar_folga(s, t, l, 0, SLOTS_CARGA + 3).is_err());
        let (s, t, l) = pilha_com(8);
        assert!(checar_folga(s, t, l, 0, SLOTS_CARGA + 3).is_ok());
    }

    #[test]
    fn folga_exata_e_rejeitada() {
        let (stack, top, last) = pilha_com(10);
        assert!(checar_folga(stack, top, last, 0, 10).is_err());
    }

    #[test]
    fn folga_acima_do_necessario_passa() {
        let (stack, top, last) = pilha_com(10);
        assert_eq!(checar_folga(stack, top, last, 0, 9), Ok(10));
    }

    #[test]
    fn stack_last_antes_do_top_e_erro() {
        let (stack, top, _) = pilha_com(10);
        assert!(checar_folga(stack, top, stack + 16, 0, 1).is_err());
    }

    #[test]
    fn stack_last_zero_e_erro() {
        let (stack, top, _) = pilha_com(10);
        assert!(checar_folga(stack, top, 0, 0, 1).is_err());
    }

    #[test]
    fn ncalls_no_limite_e_erro() {
        let (stack, top, last) = pilha_com(1000);
        assert!(checar_folga(stack, top, last, MAX_NCCALLS, 1).is_err());
        assert!(checar_folga(stack, top, last, MAX_NCCALLS - 1, 1).is_ok());
    }

    #[test]
    fn maxstack_mais_margem_exige_264_livres() {
        let necessarios = 255 + MARGEM_EXEC_SLOTS;
        let (stack, top, last) = pilha_com(263);
        assert!(checar_folga(stack, top, last, 0, necessarios).is_err());
        let (stack, top, last) = pilha_com(264);
        assert!(checar_folga(stack, top, last, 0, necessarios).is_ok());
    }

    #[test]
    fn contadores_separam_novos_de_repetidos() {
        let t0 = Instant::now();
        let mut e = Estados::default();
        e.rejeitar(0x1000, t0);
        assert!(e.foi_rejeitado(0x1000, t0));
        assert!(e.foi_rejeitado(0x1000, t0));
        assert!(!e.foi_rejeitado(0x2000, t0));
        assert_eq!(e.contadores(), (1, 2));
    }
}
