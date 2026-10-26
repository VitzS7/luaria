//! Motor do executor: conecta no cliente, resolve as âncoras, instala o hook e
//! executa scripts. É o que a CLI e a UI compartilham.

use std::path::PathBuf;

use crate::elf::Elf;
use crate::estado_jogo::{
    checar_folga, Identidade, MARGEM_EXEC_SLOTS, MAX_NCCALLS, MAX_REJEITADOS, SLOTS_CARGA,
};
use crate::hook::Hook;
use crate::nativas;
use crate::ponte_layout;
use crate::ponte_nativas;
use crate::process::Alvo;
use crate::remote::Sessao;
use crate::vm::Anchors;
use crate::xref::XrefTable;

// Layout de `lua_State` medido por payload (ver docs/arquitetura.md).
// O precall faz base = func+1 e grava o topo; o `tt` do TValue fica em +0xc.
// FUNÇÃO é 8 neste build: é o valor que o argv pausado tinha e o que o precall usa para pular o __call.
pub(crate) const OFF_TOP: u64 = 0x28;
pub(crate) const OFF_BASE: u64 = 0x20;
pub(crate) const OFF_STACK: u64 = 0x30;
// Fim lógico da pilha (stacksize +0x80 inclui 5 slots extras após stack_last), e
// contador de chamadas C (u16). Medidos por payload.
pub(crate) const OFF_STACK_LAST: u64 = 0x08;
pub(crate) const OFF_NCCALLS: u64 = 0x58;
pub(crate) const OFF_TT: u64 = 0xC;
pub(crate) const TT_FUNCAO: u32 = 8;

// Capability neste build (medida por payload, ver docs/arquitetura.md): a VM
// calcula as caps efetivas do código em execução como
//   base & [ [proto+0x20] ]
// onde `base = [[L+0x60]+0xa0]` é o teto do contexto (cheio na thread de
// CorePackages) e `proto` é o Proto do closure em execução. [proto+0x20] é o
// PONTEIRO para um objeto de capability, não a máscara: o checker faz
// `and base, [ret]`, ou seja lê a máscara em [[proto+0x20]].
//
// Nosso chunk sai do luau_load com [proto+0x20]=0, então o checker cai no
// objeto default (restritivo) e o script roda sandboxed. Para elevar,
// `proto+0x20` tem que APONTAR para um u64 com a máscara cheia — escrever a
// máscara crua ali faz a VM desreferenciar um ponteiro inválido e derruba o
// cliente (medido na marra). O objeto da máscara fica num slot estável da página
// do stub. Caps efetivas = base & cheia = base.
pub(crate) const OFF_PROTO_CAPS: u64 = 0x20;
pub(crate) const CAPS_CHEIA: u64 = 0xFFFF_FFFF_FFFF_FFFF;
// Slot livre na página do stub onde a máscara cheia é gravada (entre RETVAL=0x748
// e ARGS=0x800).
pub(crate) const OFF_CAPS_OBJ: u64 = 0x758;
// Endereço do mmap do chunk, gravado na página do stub: um Motor novo no mesmo
// processo (UI reaberta, reconexão) readota a região em vez de vazar 2 MB por anexo.
pub(crate) const OFF_REGIAO: u64 = 0x760;
// Página de código e anel da ponte host→jogo (`ponte_nativas`/`ponte_layout`). Ficam no
// stub para a adoção de um hook já instalado reencontrar os mesmos endereços.
pub(crate) const OFF_PONTE_PAGINA: u64 = 0x768;
pub(crate) const OFF_PONTE_ANEL: u64 = 0x770;

/// Identidade/elevação pedida por `LUARIA_IDENTIDADE`. Sem a variável, eleva para 8
/// (validado ao vivo em 25/10: `GetService("Players")` e `Instance.new` passam a
/// funcionar). `0` desliga e mantém o chunk sandboxed.
pub(crate) fn identidade_pedida() -> u32 {
    identidade_de(std::env::var("LUARIA_IDENTIDADE").ok().as_deref())
}

const IDENTIDADE_PADRAO: u32 = 8;

/// Valor ilegível vira 0 (sem elevação): nunca liga escrita em memória por engano.
fn identidade_de(v: Option<&str>) -> u32 {
    let Some(v) = v else { return IDENTIDADE_PADRAO };
    v.trim().parse::<u32>().unwrap_or(0)
}

// Nome do chunk (C string): o f_parser faz strlen nele.
pub(crate) const OFF_NOME: u64 = 0x780;
// Fonte do probe de "estou num jogo?" — área livre antes do descritor.
pub(crate) const OFF_PROBE: u64 = 0x200;
pub(crate) const TAM_PROBE: usize = 0x100;

/// Primeira linha do chunk que pertence ao script do usuário (1-based).
///
/// O chunk é `wrapper(1) + instantâneo de arquivos(1) + prelude + código`. A UI usa isso para converter o
/// número de linha de um erro de volta para a linha do editor.
pub fn linha_do_usuario() -> usize {
    crate::prelude::completo().lines().count() + 3
}

/// Tamanho da região de dados mapeada no cliente para o chunk.
pub(crate) const TAM_REGIAO: usize = 0x20_0000;

/// Quantas vezes o motor reaproveita o freio quando a thread freada não serve
/// (o cliente sobrevive ao SIGSEGV: a chamada remota roda na thread do hook).
const TENTATIVAS: u32 = 3;

/// Teto da descida em `Proto::p[]`. Hub inlinado pelo host (remoto.rs) aninha
/// função dentro de função e passa fácil de 8 níveis; a descida é iterativa e
/// já tem o `feitos`, então o teto só barra um `p[]` corrompido.
const MAX_PROFUNDIDADE: usize = 64;

// Endereços medidos por payload; cada um é conferido
// pelo prólogo antes do uso (`conferir_prologo`), então payload novo falha alto.
pub(crate) const FN_CORPO_CALL: u64 = 0x66e_8721; // corpo de luaD_call: luaD_precall + luaV_execute
pub(crate) const FN_PRECALL: u64 = 0x27_31d87; // luaD_precall(L, func, nresults)
pub(crate) const FN_EXECUTAR: u64 = 0x27_356c3; // luaV_execute(L)
// luau_load(L, nome, dados, tamanho, env): undump + proteção + limpeza; é o
// caminho que o próprio cliente usa para carregar script.
pub(crate) const FN_CARREGAR: u64 = 0x273_0b12;

// Tabela de despacho do interpretador: 256 ponteiros para handler, indexada pelo
// byte de opcode. É preenchida por relocação (o arquivo só tem zeros), então a
// única leitura confiável é a memória do processo vivo. O índice é embaralhado
// por build; o conteúdo não.
pub(crate) const TAB_DESPACHO: u64 = 0x70b_8e20;
const TAM_TAB_DESPACHO: u64 = 256;

// No `loadsafe` o resultado do verificador de assinatura é comparado com 1 e, se bater, o load morre com "bytecode corrupted". Chunks
// abaixo de 0x118 bytes nunca chegam à verificação de verdade; a partir daí ela
// roda e recusa qualquer bytecode sem assinatura da Roblox. Como não dá para
// assinar, trocamos a constante do compare por um valor que o verificador nunca
// devolve (ele só retorna 0 ou 1) e o undump segue normalmente.
pub(crate) const SITE_VERIFICACAO: u64 = 0x670_18bd;
const BYTES_VERIFICACAO: [u8; 4] = [0x83, 0xf8, 0x01, 0x75];
const BYTES_VERIFICACAO_PATCH: [u8; 4] = [0x83, 0xf8, 0x7f, 0x75];
/// Tamanho a partir do qual o `loadsafe` executa a verificação de assinatura.
const TAM_MIN_VERIFICACAO: usize = 0x118;

fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect::<Vec<_>>().join(" ")
}

pub struct Motor {
    pub pid: i32,
    pub alvo: Alvo,
    pub elf: Elf,
    pub anchors: Anchors,
    pub hook: Option<Hook>,
    /// Estados já identificados como LuaApp, com validade (ver `estado_jogo`).
    pub estados: std::cell::RefCell<crate::estado_jogo::Estados>,
    pub identidade: crate::estado_jogo::Identidade,
    /// Região mmap no cliente onde o chunk é escrito (0 = ainda não criada).
    pub regiao: std::cell::Cell<u64>,
    pub indice: crate::eh_frame::Indice,
    /// Primeiros bytes da seção de código do último chunk compilado: serve para
    /// localizar o campo `code` do Proto em runtime (o offset muda por build).
    pub assinatura: std::cell::RefCell<Vec<u8>>,
    /// Último chunk já traduzido para os opcodes do cliente (usado pelo patch).
    pub chunk_remapeado: std::cell::RefCell<Vec<u8>>,
}

#[derive(Debug)]
pub enum ErroMotor {
    SemCliente,
    SemPayload(String),
    SemAncoras,
    Io(String),
    Compilacao(i32),
    TempoEsgotado,
    /// A thread freada pertence ao LuaApp (tela inicial), não a um jogo.
    ForaDoJogo,
}

impl std::fmt::Display for ErroMotor {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ErroMotor::SemCliente => write!(f, "Mocktail client is not open"),
            ErroMotor::SemPayload(e) => write!(f, "unreadable payload: {e}"),
            ErroMotor::SemAncoras => write!(f, "VM anchors not resolved in this payload"),
            ErroMotor::Io(e) => write!(f, "memory failure: {e}"),
            ErroMotor::Compilacao(s) => write!(f, "the chunk did not compile (status {s})"),
            ErroMotor::TempoEsgotado => write!(f, "no script load in 120s"),
            ErroMotor::ForaDoJogo => write!(f, "LuaApp state, not a game"),
        }
    }
}

/// Zera o flag de pausa do hook do Luaria em `off`. Sem hook nosso ali, não faz nada.
fn soltar_hook(alvo: &Alvo, off: u64) {
    if let Ok(h) = Hook::adotar(alvo, off) {
        h.liberar(alvo).ok();
    }
}

/// Sem isso, um luaria morto no meio da execução deixa o freio armado e a thread
/// do jogo presa no laço do stub. Erro é ignorado: se o processo morreu, não há
/// o que soltar.
impl Drop for Motor {
    fn drop(&mut self) {
        if let Ok(off) = self.offset_freio() {
            soltar_hook(&self.alvo, off);
        }
        if let (Some(_), Some(off)) = (&self.hook, self.anchors.luau_load) {
            soltar_hook(&self.alvo, off);
        }
    }
}

impl Motor {
    /// Conecta no processo vivo e resolve tudo que depende da versão do payload.
    pub fn conectar() -> Result<Motor, ErroMotor> {
        Self::conectar_excluindo(&[])
    }

    /// `excluidos` são pids de clientes que já têm motor: não os escolhe de novo.
    pub fn conectar_excluindo(excluidos: &[i32]) -> Result<Motor, ErroMotor> {
        let pid = crate::process::escolher_candidato(&crate::process::candidatos_payload(), excluidos)
            .ok_or(ErroMotor::SemCliente)?;
        Self::conectar_pid(pid)
    }

    pub fn conectar_pid(pid: i32) -> Result<Motor, ErroMotor> {
        let alvo = Alvo::novo(pid).map_err(|e| ErroMotor::Io(e.to_string()))?;
        let elf = Elf::open(&alvo.caminho).map_err(|e| ErroMotor::SemPayload(e.to_string()))?;
        let tabela = XrefTable::build(&elf);
        let anchors = crate::vm::resolve_com(&elf, &tabela);
        if anchors.luau_load.is_none() || anchors.lua_d_call.is_none() {
            return Err(ErroMotor::SemAncoras);
        }
        let indice = crate::eh_frame::Indice::novo(&elf);
        let base = alvo.base;
        Self::validar_ancoras(&alvo, &indice)?;
        let motor = Motor {
            pid,
            alvo,
            elf,
            anchors,
            hook: None,
            indice,
            estados: Default::default(),
            regiao: Default::default(),
            identidade: Identidade {
                pid,
                inicio: crate::process::inicio_do_processo(pid).unwrap_or(0),
                base,
            },
            assinatura: Default::default(),
            chunk_remapeado: Default::default(),
        };
        // luaria anterior pode ter morrido com o freio armado: solta antes de usar
        if let Ok(off) = motor.offset_freio() {
            soltar_hook(&motor.alvo, off);
        }
        Ok(motor)
    }

    pub fn caminho_payload(&self) -> PathBuf {
        PathBuf::from(&self.alvo.caminho)
    }

    /// (só viu LuaApp, PlaceId) para a UI classificar o cliente sem anexar.
    pub fn estado_cliente(&self) -> (bool, Option<u64>) {
        let estados = self.estados.borrow();
        (estados.so_menu(), estados.place())
    }

    /// Anel da ponte jogo→host, lido do stub do freio. `None` antes da primeira execução
    /// nossa neste processo ou se o endereço gravado não aponta mais para um anel válido.
    pub fn anel_ponte(&self) -> Option<u64> {
        let stub = Hook::adotar(&self.alvo, self.offset_freio().ok()?).ok()?.stub;
        let anel = self.alvo.u64_em(stub + OFF_PONTE_ANEL).ok().filter(|a| *a != 0)?;
        self.anel_valido(anel).then_some(anel)
    }

    /// Lê a tabela de despacho viva e monta o mapa de opcodes deste build.
    ///
    /// O interpretador despacha pela tabela indexada pelo opcode; quantos índices existem não muda,
    /// mas quais estão preenchidos (e em que byte) é embaralhado por build. A
    /// ordem dos handlers por endereço é a ordem do arquivo `lvmexecute.cpp`,
    /// então ela é a âncora estável do casamento.
    pub fn mapa_opcodes(&self) -> Result<crate::opcodes::Mapa, ErroMotor> {
        let mut entradas = Vec::new();
        for i in 0..TAM_TAB_DESPACHO {
            let p = self.alvo.base + TAB_DESPACHO + i * 8;
            let v = self.alvo.u64_em(p).map_err(|e| ErroMotor::Io(e.to_string()))?;
            if v != 0 {
                entradas.push((v, i as u8));
            }
        }
        crate::opcodes::Mapa::das_entradas(&entradas)
            .map_err(|e| ErroMotor::Io(format!("opcode map: {e}")))
    }

    /// Instala o hook (idempotente) e devolve a página de trabalho.
    pub fn garantir_hook(&mut self) -> Result<Hook, ErroMotor> {
        let off = self.anchors.luau_load.ok_or(ErroMotor::SemAncoras)?;
        if let Some(h) = &self.hook {
            return Ok(h.clone());
        }
        let mut instalado = None;
        for tentativa in 1..=12 {
            match Hook::adotar(&self.alvo, off) {
                Ok(h) => {
                    instalado = Some(h);
                    break;
                }
                Err(_) => match Hook::instalar(&self.alvo, &self.elf, off, None, true) {
                    Ok(h) => {
                        instalado = Some(h);
                        break;
                    }
                    // função quente: espera ela sair dos bytes e tenta de novo
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                        std::thread::sleep(std::time::Duration::from_millis(60 * tentativa));
                    }
                    Err(e) => return Err(ErroMotor::Io(e.to_string())),
                },
            }
        }
        let h = instalado.ok_or_else(|| {
            ErroMotor::Io("could not install the hook: function always running".into())
        })?;
        self.hook = Some(h.clone());
        Ok(h)
    }

    /// Executa Lua no cliente. Retorna as linhas de diagnóstico do caminho usado.
    ///
    /// O freio fica em `luaD_call` (chamada o tempo todo): quando armado, a
    /// primeira chamada de script para a thread com o `lua_State` vivo, o host
    /// aproveita a pausa, executa e solta.
    pub fn executar(&mut self, codigo: &str) -> Result<Vec<String>, ErroMotor> {
        let mut log = Vec::new();
        let r = self.executar_log(codigo, &mut log);
        if r.is_err() {
            for l in &log {
                eprintln!("[exec] {l}");
            }
        }
        r.map(|_| log)
    }

    /// Igual a `executar`, mas entrega o log mesmo em caso de erro — a UI usa
    /// isso para mostrar onde o diagnóstico parou.
    pub fn executar_com_log(&mut self, codigo: &str, log: &mut Vec<String>) -> Result<(), ErroMotor> {
        self.executar_log(codigo, log)
    }

    /// Captura um load real do jogo: freia no loadsafe e lê os
    /// argumentos + os primeiros bytes do chunk. Serve para descobrir a versão
    /// de bytecode que o payload realmente usa.
    pub fn capturar_load(&mut self, log: &mut Vec<String>) -> Result<(), ErroMotor> {
        let alvo = self.anchors.luau_load_wrapper.ok_or(ErroMotor::SemAncoras)?;
        let freio = match Hook::adotar(&self.alvo, alvo) {
            Ok(h) => h,
            Err(_) => Hook::instalar(&self.alvo, &self.elf, alvo, None, false)
                .map_err(|e| ErroMotor::Io(e.to_string()))?,
        };
        freio.desarmar(&self.alvo).ok();
        freio.pedir_pausa(&self.alvo).map_err(|e| ErroMotor::Io(e.to_string()))?;
        let t0 = std::time::Instant::now();
        while !freio.em_pausa(&self.alvo) {
            if t0.elapsed().as_secs() > 90 {
                freio.liberar(&self.alvo).ok();
                return Err(ErroMotor::TempoEsgotado);
            }
            std::thread::sleep(std::time::Duration::from_millis(2));
        }
        // os slots do stub são compartilhados entre threads; os registradores da
        // thread freada são a única fonte confiável dos argumentos desta chamada
        let paradas = self.alvo.parar_com_guarda();
        let r = (|| -> Result<(), ErroMotor> {
            let tid = self
                .thread_em_pausa(&paradas.tids, freio.stub)
                .ok_or(ErroMotor::Io("pause thread not found".into()))?;
            let r = regs_completos(tid).ok_or(ErroMotor::Io("no registers".into()))?;
            let a = [r[14], r[13], r[12], r[11], r[9], r[8]];
            log.push(format!(
                "load: L={:#x} nome={:#x} dados={:#x} tam={} env={}",
                a[0], a[1], a[2], a[3], a[4]
            ));
            let n = (a[3] as usize).min(64);
            if let Ok(b) = self.alvo.bytes(a[2], n) {
                log.push(format!("bytes: {}", hex(&b)));
            }
            if let Ok(nome) = self.alvo.bytes(a[1], 32) {
                log.push(format!("nome: {}", hex(&nome)));
            }
            Ok(())
        })();
        freio.liberar(&self.alvo).ok();
        r
    }

    /// Reconecta se o cliente trocou (jogo novo = processo novo) e só então
    /// executa. O freio sai sempre desarmado, qualquer que seja o resultado.
    fn executar_log(&mut self, codigo: &str, log: &mut Vec<String>) -> Result<(), ErroMotor> {
        self.revalidar(log)?;
        let off_call = self.offset_freio()?;
        let r = self.executar_com_freio(codigo, off_call, log);
        soltar_hook(&self.alvo, off_call);
        r
    }

    fn offset_freio(&self) -> Result<u64, ErroMotor> {
        match std::env::var("LUARIA_FREIO") {
            Ok(v) => u64::from_str_radix(v.trim_start_matches("0x"), 16)
                .map_err(|_| ErroMotor::Io("invalid LUARIA_FREIO".into())),
            Err(_) => self.anchors.lua_d_call.ok_or(ErroMotor::SemAncoras),
        }
    }

    fn executar_com_freio(
        &mut self,
        codigo: &str,
        off_call: u64,
        log: &mut Vec<String>,
    ) -> Result<(), ErroMotor> {
        let freio = match Hook::adotar(&self.alvo, off_call) {
            Ok(h) => h,
            Err(_) => Hook::instalar(&self.alvo, &self.elf, off_call, None, false)
                .map_err(|e| ErroMotor::Io(e.to_string()))?,
        };
        freio.desarmar(&self.alvo).map_err(|e| ErroMotor::Io(e.to_string()))?;

        let chunk = self.preparar_chunk(codigo, log)?;
        let nome = freio.stub + OFF_NOME;
        self.alvo
            .escrever(nome, b"=luaria\0")
            .map_err(|e| ErroMotor::Io(e.to_string()))?;
        let tam_dados = chunk.len();
        // Chunk grande cai na verificação de assinatura do loadsafe e volta
        // status 1; neutraliza o compare só quando o tamanho chega lá.
        if tam_dados >= TAM_MIN_VERIFICACAO {
            self.dispensar_verificacao(log)?;
        }
        let trap = crate::remote::preparar_trap(&self.alvo, freio.stub)
            .map_err(|e| ErroMotor::Io(e.to_string()))?;

        // O freio consome uma entrada em luaD_call por tentativa. Se a thread
        // que parou não serve (estado quebrado pelo undump), limpa o estado e
        // arma de novo — a thread que morreu no SIGSEGV é descartável e o
        // cliente continua jogando.
        let mut ultimo = ErroMotor::TempoEsgotado;
        let comeco = std::time::Instant::now();
        let mut tentativa = 0u32;
        // Sem o sleep de 50 ms no ramo do app o laço gira sem pausa: este teto
        // evita travar para sempre se o cliente estiver parado no menu.
        const VOLTAS_APP_MAX: u32 = 3000;
        const TEMPO_APP_MAX: u64 = 30;
        let mut app_seguidas = 0u32;
        let mut app_desde: Option<std::time::Instant> = None;
        fn erro_menu(voltas: u32) -> ErroMotor {
            ErroMotor::Io(format!(
                "{voltas} LuaApp captures in a row and none from a game; is the client on the menu?"
            ))
        }
        loop {
            tentativa += 1;
            if comeco.elapsed().as_secs() > 180 {
                freio.liberar(&self.alvo).ok();
                return Err(ultimo);
            }
            freio.liberar(&self.alvo).ok();
            if tentativa > 1 {
                log.push(format!("attempt {tentativa}/{TENTATIVAS}"));
                // deixa a thread anterior sair do laço do stub antes de armar de novo
                std::thread::sleep(std::time::Duration::from_millis(300));
            }
            freio.pedir_pausa(&self.alvo).map_err(|e| ErroMotor::Io(e.to_string()))?;
            let t0 = std::time::Instant::now();
            while !freio.em_pausa(&self.alvo) {
                if t0.elapsed().as_secs() > 20 {
                    break;
                }
                // primeiros 5 ms giram sem dormir: o sleep do kernel custa ~1 ms e atrasa cada volta
                if t0.elapsed() < std::time::Duration::from_millis(5) {
                    std::hint::spin_loop();
                } else {
                    std::thread::sleep(std::time::Duration::from_micros(200));
                }
            }
            if !freio.em_pausa(&self.alvo) {
                log.push("the pause did not arrive in 20s (no thread went through the hook?)".into());
                ultimo = ErroMotor::TempoEsgotado;
                continue;
            }
            // L já rejeitado é thread do app: solta antes de mexer nas faixas ou
            // no chunk, senão cada volta custa a restauração inteira.
            let agora = std::time::Instant::now();
            let l_slot = freio.args(&self.alvo).map(|a| a[0]).ok();
            if l_slot.is_some_and(|l| self.estados.borrow_mut().foi_rejeitado(l, agora)) {
                freio.liberar(&self.alvo).ok();
                app_seguidas += 1;
                app_desde.get_or_insert(agora);
                if app_seguidas > VOLTAS_APP_MAX
                    || app_desde.is_some_and(|d| d.elapsed().as_secs() > TEMPO_APP_MAX)
                {
                    return Err(erro_menu(app_seguidas));
                }
                tentativa -= 1;
                continue;
            }
            if let Err(e) = self.alvo.atualizar_faixas() {
                freio.liberar(&self.alvo).ok();
                return Err(ErroMotor::Io(format!("unreadable maps: {e}")));
            }
            match self.fase_remota(&freio, nome, &chunk, trap, log) {
                Ok(()) => {
                    freio.liberar(&self.alvo).ok();
                    return Ok(());
                }
                Err(ErroMotor::ForaDoJogo) => {
                    app_seguidas += 1;
                    app_desde.get_or_insert(agora);
                    if app_seguidas > VOLTAS_APP_MAX
                        || app_desde.is_some_and(|d| d.elapsed().as_secs() > TEMPO_APP_MAX)
                    {
                        freio.liberar(&self.alvo).ok();
                        return Err(erro_menu(app_seguidas));
                    }
                    if self.estados.borrow_mut().esgotou(std::time::Instant::now()) {
                        let (novos, repetidos) = self.estados.borrow().contadores();
                        log.push(format!(
                            "capture: {novos} new app states, {repetidos} re-hits of an already rejected state"
                        ));
                        let lista: Vec<String> = self
                            .estados
                            .borrow()
                            .amostra(6)
                            .iter()
                            .map(|v| format!("{v:#x}"))
                            .collect();
                        freio.liberar(&self.alvo).ok();
                        return Err(ErroMotor::Io(format!(
                            "{MAX_REJEITADOS} LuaApp lua_States and none from a game (e.g. {}); is the client on the menu?",
                            lista.join(", ")
                        )));
                    }
                    // thread do app: solta, deixa outra entrar e tenta de novo.
                    // Não conta como falha — o jogo pode estar carregando.
                    freio.liberar(&self.alvo).ok();
                    tentativa -= 1;
                    continue;
                }
                Err(e) => {
                    app_seguidas = 0;
                    app_desde = None;
                    log.push(format!("attempt {tentativa} failed: {e}"));
                    ultimo = e;
                    if tentativa >= TENTATIVAS {
                        freio.liberar(&self.alvo).ok();
                        return Err(ultimo);
                    }
                }
            }
        }
    }

    /// Embrulha o script (prelude + fronteira de erro), compila e devolve o
    /// bytecode já no mapa de opcodes do cliente.
    ///
    /// A fronteira é `coroutine.resume`: o fast-pcall da Roblox não sobrevive à
    /// nossa pilha falsa, mas `lua_resume` devolve `(false, erro)` sem tocá-lo.
    fn preparar_chunk(&self, codigo: &str, log: &mut Vec<String>) -> Result<Vec<u8>, ErroMotor> {
        let (codigo, remoto) = crate::remoto::resolver_remoto(codigo, log);
        let codigo = codigo.as_str();
        let corpo = if std::env::var("LUARIA_SEM_PRELUDE").is_ok() {
            codigo.to_string()
        } else {
            // a identidade entra colada à linha do instantaneo (antes do prelude,
            // que lê __luaria_identidade ao rodar) e não desloca linha_do_usuario().
            let elev = match identidade_pedida() {
                0 => String::new(),
                n => format!(" __luaria_identidade = {n};"),
            };
            format!(
                "{}{}\n{}\n{}{codigo}",
                crate::arquivos::instantaneo(),
                elev,
                crate::prelude::completo(),
                crate::prelude::ponte_globais(codigo)
            )
        };
        let fonte = match std::env::var("LUARIA_WRAP").as_deref() {
            Ok("0") => corpo,
            Ok("pcall") => format!("return pcall(function()\n{corpo}\nend)\n"),
            _ => embrulhar(&corpo),
        };
        let b = compilar(&fonte).map_err(|e| ErroMotor::Io(format!(
            "luau-compile: {e}{}",
            if remoto { " (the code has inlined remote content; line numbers may not match)" } else { "" }
        )))?;
        // o bytecode público numera opcode pela ordem do enum; este build
        // embaralha os valores, então traduz antes de entregar ao loader
        let mapa = self.mapa_opcodes()?;
        let remapeado = crate::opcodes::remapear(&b, &mapa)
            .map_err(|e| ErroMotor::Io(format!("remapeamento: {e}")))?;
        // LUARIA_SEM_REMAP=1 manda o canônico e deixa o undump do cliente traduzir
        let final_ = if std::env::var("LUARIA_SEM_REMAP").is_ok() { b } else { remapeado.clone() };
        if final_.len() > TAM_REGIAO {
            return Err(ErroMotor::Io(format!("bytecode of {} bytes exceeds {} KB", final_.len(), TAM_REGIAO / 1024)));
        }
        *self.chunk_remapeado.borrow_mut() = final_.clone();
        if let Some(code) = crate::opcodes::code_section(&remapeado, 0) {
            *self.assinatura.borrow_mut() = code[..code.len().min(16)].to_vec();
        }
        log.push(format!("luau-compile: {} bytes", final_.len()));
        Ok(final_)
    }

    /// Valida os prólogos vivos e grava o bloco de nativas na página do stub (só se os
    /// bytes diferirem: reescrever código que um script anterior ainda possa estar
    /// chamando seria perigoso). `None` = roda sem nativas, o executor continua.
    fn preparar_nativas(&self, sessao: &Sessao, trap: u64, stub: u64, log: &mut Vec<String>) -> Option<[u64; nativas::QTD as usize]> {
        if !nativas::ligadas() {
            log.push("nativas desligadas (LUARIA_NATIVAS=0)".into());
            return None;
        }
        let conferido = nativas::FUNCOES_JOGO
            .iter()
            .try_for_each(|f| self.conferir_prologo(f.va, &f.prologo));
        if let Err(e) = conferido {
            log.push(format!("WARNING: natives disabled, payload changed: {e}"));
            return None;
        }
        let bloco = nativas::montar_bloco(self.alvo.base);
        let destino = stub + nativas::OFF_NATIVAS;
        if self.alvo.bytes(destino, bloco.len()).map_or(true, |atual| atual != bloco) {
            let gravou = self.alvo.escrever(destino, &bloco).is_ok()
                && self.alvo.bytes(destino, bloco.len()).map_or(false, |lido| lido == bloco);
            if !gravou {
                log.push("WARNING: natives disabled, failed to write the block to the stub".into());
                return None;
            }
            log.push(format!("natives: {} byte block written at stub+{:#x}", bloco.len(), nativas::OFF_NATIVAS));
        }
        let (pagina, _) = match self.preparar_ponte(sessao, stub, trap) {
            Ok(ponte) => ponte,
            Err(e) => {
                log.push(format!("AVISO: nativas desligadas, ponte: {e}"));
                return None;
            }
        };
        Some([
            stub + nativas::OFF_GETRAWMETATABLE,
            stub + nativas::OFF_SETRAWMETATABLE,
            stub + nativas::OFF_SETREADONLY,
            stub + nativas::OFF_GETCONSTANT,
            stub + nativas::OFF_GETUPVALUE,
            stub + nativas::OFF_GETGC,
            pagina + ponte_nativas::OFF_PE as u64,
            pagina + ponte_nativas::OFF_PR as u64,
            pagina + ponte_nativas::OFF_SK as u64,
            pagina + ponte_nativas::OFF_SU as u64,
        ])
    }

    /// Página de código e anel da ponte, guardados no stub. Reusa os da execução
    /// anterior (ou de um Motor que readotou o stub) se ainda baterem; senão aloca.
    /// Por que reescrever a página se o conteúdo difere: o endereço do anel ou do
    /// pushlstring pode ter mudado desde que ela foi gravada.
    fn preparar_ponte(&self, sessao: &Sessao, stub: u64, trap: u64) -> Result<(u64, u64), ErroMotor> {
        self.conferir_prologo(nativas::PUSHLSTRING_VA, &nativas::PROLOGO_PUSHLSTRING)?;
        self.conferir_prologo(nativas::BARRIERF_VA, &nativas::PROLOGO_BARRIERF)?;
        self.conferir_prologo(nativas::LUA_SETUPVALUE_VA, &nativas::PROLOGO_LUA_SETUPVALUE)?;
        let guardado_anel = self.alvo.u64_em(stub + OFF_PONTE_ANEL).unwrap_or(0);
        let anel = if self.anel_valido(guardado_anel) {
            guardado_anel
        } else {
            self.criar_anel(sessao, trap)?
        };
        let base = self.alvo.base;
        let esperado = ponte_nativas::montar_pagina(
            base + nativas::PUSHLSTRING_VA,
            base + nativas::BARRIERF_VA,
            base + nativas::LUA_SETUPVALUE_VA,
            anel,
        );
        let guardada = self.alvo.u64_em(stub + OFF_PONTE_PAGINA).unwrap_or(0);
        let pagina = if guardada != 0 && self.alvo.bytes(guardada, ponte_nativas::TAM_PAGINA).is_ok() {
            guardada
        } else {
            // Por que mmap e não página do payload: o payload medido tem uma única página zerada e o stub já a usa.
            self.mmap_anonimo(sessao, trap, ponte_nativas::TAM_PAGINA, PROT_RWX)?
        };
        if self.alvo.bytes(pagina, ponte_nativas::TAM_PAGINA).map_or(true, |atual| atual != esperado) {
            self.alvo.escrever(pagina, &esperado).map_err(|e| ErroMotor::Io(e.to_string()))?;
        }
        if pagina != guardada || anel != guardado_anel {
            self.alvo.escrever(stub + OFF_PONTE_PAGINA, &pagina.to_le_bytes()).map_err(|e| ErroMotor::Io(e.to_string()))?;
            self.alvo.escrever(stub + OFF_PONTE_ANEL, &anel.to_le_bytes()).map_err(|e| ErroMotor::Io(e.to_string()))?;
        }
        Ok((pagina, anel))
    }

    fn anel_valido(&self, anel: u64) -> bool {
        anel != 0 && self.alvo.bytes(anel, 12).map_or(false, |cabecalho| cabecalho == cabecalho_anel())
    }

    fn criar_anel(&self, sessao: &Sessao, trap: u64) -> Result<u64, ErroMotor> {
        let anel = self.mmap_anonimo(sessao, trap, ponte_layout::TAM_ANEL, PROT_RW)?;
        self.alvo.escrever(anel, &cabecalho_anel()).map_err(|e| ErroMotor::Io(e.to_string()))?;
        Ok(anel)
    }

    /// mmap anônimo dentro do cliente, chamado remotamente com a thread parada.
    fn mmap_anonimo(&self, sessao: &Sessao, trap: u64, tam: usize, prot: u64) -> Result<u64, ErroMotor> {
        let mmap = crate::plt::Imports::load(&self.elf)
            .stub("mmap")
            .ok_or_else(|| ErroMotor::Io("payload does not import mmap".into()))?;
        const MAP_PRIVADO_ANONIMO: u64 = 0x22;
        let r = sessao
            .chamar(
                &self.alvo,
                self.alvo.base + mmap,
                [0, tam as u64, prot, MAP_PRIVADO_ANONIMO, u64::MAX, 0],
                trap,
                0,
            )
            .map_err(|e| ErroMotor::Io(e.to_string()))?;
        if r == u64::MAX || r == 0 || r >= 0x8000_0000_0000 {
            return Err(ErroMotor::Io(format!("remote mmap failed ({r:#x})")));
        }
        Ok(r)
    }

    /// Região de dados do chunk: mmap anônimo dentro do cliente, criado na
    /// primeira execução. A página livre do payload só comporta ~18 KB, pouco
    /// para o teste da UNC. Reaproveitada nas execuções seguintes.
    fn regiao_dados(&self, sessao: &Sessao, stub: u64, trap: u64) -> Result<u64, ErroMotor> {
        if self.regiao.get() != 0 {
            return Ok(self.regiao.get());
        }
        let guardada = self.alvo.u64_em(stub + OFF_REGIAO).unwrap_or(0);
        if guardada != 0 && self.alvo.regiao_gravavel(guardada, TAM_REGIAO as u64) {
            self.regiao.set(guardada);
            return Ok(guardada);
        }
        let r = self.mmap_anonimo(sessao, trap, TAM_REGIAO, PROT_RW)?;
        self.alvo.escrever(stub + OFF_REGIAO, &r.to_le_bytes()).map_err(|e| ErroMotor::Io(e.to_string()))?;
        self.regiao.set(r);
        Ok(r)
    }

    /// Tudo que acontece com as threads paradas. Qualquer erro aqui ainda passa
    /// pela limpeza de `executar_log`.
    /// `stack_last` e nCcalls de `l`. Leitura falha fica do lado seguro: `stack_last`
    /// zero e nCcalls no teto fazem `checar_folga` recusar.
    fn ler_folga(&self, l: u64) -> (u64, u16) {
        let stack_last = self.alvo.u64_em(l + OFF_STACK_LAST).unwrap_or(0);
        let nc = self
            .alvo
            .bytes(l + OFF_NCCALLS, 2)
            .map(|b| u16::from_le_bytes([b[0], b[1]]))
            .unwrap_or(MAX_NCCALLS);
        (stack_last, nc)
    }

    fn fase_remota(
        &self,
        freio: &Hook,
        nome: u64,
        chunk: &[u8],
        trap: u64,
        log: &mut Vec<String>,
    ) -> Result<(), ErroMotor> {
        let paradas = self.alvo.parar_com_guarda();
        let paradas = &paradas.tids;
        log.push(format!("threads paradas: {}", paradas.len()));
        (|| -> Result<(), ErroMotor> {
            // A thread que importa é a que está girando no laço de pausa do
            // stub: é a dona do lua_State da chamada freada. Os slots do stub
            // são globais — qualquer outra entrada em luaD_call os regrava —
            // então os argumentos têm de vir dos registradores dela.
            let tid = self.thread_em_pausa(paradas, freio.stub).ok_or_else(|| {
                ErroMotor::Io("no thread stopped in the stub pause loop".into())
            })?;
            // A chamada remota segura TODAS as threads até voltar. Se a thread
            // freada for a principal/render, o quadro para junto e o watchdog do
            // Mocktail derruba o cliente; os workers de script batem no
            // luaD_call o tempo todo, então basta recusar a Main e tentar de novo.
            if let Some(nome) = self.alvo.nome_thread(tid) {
                log.push(format!("thread {tid} ({nome}) paused"));
                if nome == "Main" && std::env::var("LUARIA_PERMITIR_MAIN").is_err() {
                    return Err(ErroMotor::Io(format!(
                        "thread {tid} is the main thread; refused so the render is not paused"
                    )));
                }
            }
            let regs = regs_completos(tid)
                .ok_or_else(|| ErroMotor::Io(format!("no registers for thread {tid}")))?;
            let l = regs[14];
            let argv = regs[13];
            let nres = regs[12];
            if l < 0x1000 {
                return Err(ErroMotor::Io(format!("invalid lua_State ({l:#x}) in thread {tid}")));
            }
            let g = self.alvo.u64_em(l + 0x18).unwrap_or(0);
            log.push(format!("L={l:#x} G={g:#x}"));
            let top_antes = self.alvo.u64_em(l + OFF_TOP).map_err(|e| {
                ErroMotor::Io(format!("lua_State {l:#x} of thread {tid} unreadable: {e}"))
            })?;
            let base = self.alvo.u64_em(l + OFF_BASE).unwrap_or(0);
            let stack = self.alvo.u64_em(l + OFF_STACK).unwrap_or(0);
            // coerência mínima: topo e base dentro da pilha alocada
            let dentro = |p: u64| p >= stack && p.saturating_sub(stack) <= 0x100_000;
            if !dentro(top_antes) || (base != 0 && !dentro(base)) {
                return Err(ErroMotor::Io(format!(
                    "inconsistent state in thread {tid}: top={top_antes:#x} base={base:#x} stack={stack:#x}"
                )));
            }
            log.push(format!(
                "hook thread: {tid} (stub+{:#x}), lua_State = {l:#x}",
                regs[16].saturating_sub(freio.stub)
            ));
            log.push(format!("top={top_antes:#x} base={base:#x} stack={stack:#x} argv={argv:#x} nres={nres}"));
            // Sem folga, o luau_load/luaD_precall tentaria crescer a pilha dentro da
            // chamada remota. Erro comum (não rejeita o estado): a folga muda com o tempo.
            let (stack_last, nc) = self.ler_folga(l);
            let extra = if nativas::ligadas() { nativas::QTD } else { 0 };
            let livres = checar_folga(stack, top_antes, stack_last, nc, SLOTS_CARGA + extra)
                .map_err(|m| ErroMotor::Io(format!("thread {tid} has no stack headroom: {m}")))?;
            log.push(format!("stack_last={stack_last:#x} livres={livres} nCcalls={nc}"));
            if let Ok(slots) = freio.args(&self.alvo) {
                if slots[0] != l {
                    log.push(format!(
                        "stub slots had L={:#x} (another thread); register value used",
                        slots[0]
                    ));
                }
            }
            for (rotulo, ptr) in [("argv", argv), ("top", top_antes), ("base", base)] {
                if let Ok(b) = self.alvo.bytes(ptr, 16) {
                    log.push(format!("  {rotulo}@{ptr:#x}: {}", hex(&b)));
                }
            }
            if std::env::var("LUARIA_DIAG").is_ok() {
                for off in (0..0xa0u64).step_by(8) {
                    log.push(format!("  L+{off:#04x} = {:#018x}", self.alvo.u64_em(l + off).unwrap_or(0)));
                }
                let ud = self.alvo.u64_em(l + 0x98).unwrap_or(0);
                for off in (0..0x100u64).step_by(8) {
                    log.push(format!("  UD+{off:#04x} = {:#018x}", self.alvo.u64_em(ud + off).unwrap_or(0)));
                }
            }
            if std::env::var("LUARIA_PROBE").is_ok() {
                return Ok(());
            }

            // Estado já descartado antes: não para o mundo de novo para nada.
            // Rejeição por VM (G compartilhado entre threads do mesmo app) fica
            // atrás de LUARIA_REJEITAR_VM até medir se G de app e de jogo são disjuntos.
            let agora = std::time::Instant::now();
            let por_vm = std::env::var("LUARIA_REJEITAR_VM").is_ok() && g != 0;
            if por_vm && self.estados.borrow_mut().vm_rejeitada(g, agora) {
                return Err(ErroMotor::ForaDoJogo);
            }
            if self.estados.borrow_mut().foi_rejeitado(l, agora) {
                return Err(ErroMotor::ForaDoJogo);
            }

            let sessao = Sessao::ja_parada(tid).map_err(|e| ErroMotor::Io(e.to_string()))?;

            // O cliente mantém o LuaApp (menu) vivo ao lado do jogo. As threads
            // do app também chamam luaD_call, então sem filtro o script roda no
            // ambiente sandboxed da tela inicial (sem Players, sem Instance.new).
            let Some(place) = self.sonda_jogo(&sessao, l, top_antes, freio, trap, log)? else {
                log.push(format!("L={l:#x} G={g:#x}: app (LuaApp)"));
                let mut estados = self.estados.borrow_mut();
                estados.rejeitar(l, agora);
                if por_vm {
                    estados.rejeitar_vm(g, agora);
                }
                return Err(ErroMotor::ForaDoJogo);
            };
            log.push(format!("L={l:#x} G={g:#x}: jogo (PlaceId {place})"));
            if self.estados.borrow_mut().observar_place(place) {
                log.push(format!("place change detected: now PlaceId {place}"));
            }

            let nativas = self.preparar_nativas(&sessao, trap, freio.stub, log);
            let src_addr = self.regiao_dados(&sessao, freio.stub, trap)?;
            self.alvo.escrever(src_addr, chunk).map_err(|e| ErroMotor::Io(e.to_string()))?;
            let (func, assinatura) =
                self.carregar(&sessao, tid, freio, l, top_antes, nome, src_addr, chunk.len(), trap, log)?;
            self.chamar_closure(&sessao, tid, l, func, top_antes, &assinatura, nativas, trap, log)
        })()
    }


    /// Põe a closure do script no topo da pilha de `l` e devolve o endereço dela.
    ///
    /// A chamada é `luau_load(L, nome, dados, tamanho, env)` deste build. Ele monta o descritor, roda o undump dentro de um
    /// `rawrunprotected` e devolve o status (0 = ok); a closure fica empilhada
    /// em `L->top`. Chamar o undump direto **não** funciona: ele é
    /// o `func(L, ud)` do pcall interno e sem a proteção o estado da pilha não
    /// é restaurado.
    fn carregar(
        &self,
        sessao: &Sessao,
        _tid: i32,
        freio: &Hook,
        l: u64,
        top_antes: u64,
        nome: u64,
        src_addr: u64,
        tam_dados: usize,
        trap: u64,
        log: &mut Vec<String>,
    ) -> Result<(u64, Vec<u8>), ErroMotor> {
        if let Ok(b) = self.alvo.bytes(src_addr, 24) {
            log.push(format!("stub+{src_addr:#x} before load: {}", hex(&b)));
        }
        let status = sessao
            .chamar(
                &self.alvo,
                self.alvo.base + FN_CARREGAR,
                [l, nome, src_addr, tam_dados as u64, 0, 0],
                trap,
                0,
            )
            .map_err(|e| ErroMotor::Io(e.to_string()))?;
        let top = self.alvo.u64_em(l + OFF_TOP).unwrap_or(0);
        log.push(format!(
            "luau_load(+{FN_CARREGAR:#x}) status={status}; top {top_antes:#x} -> {top:#x}"
        ));
        if status != 0 {
            if top == top_antes + 16 {
                let val = self.alvo.u64_em(top - 16).unwrap_or(0);
                let tt = self.alvo.u32_em(top - 16 + OFF_TT).unwrap_or(0xff);
                let txt = if tt == 6 { self.ler_tstring(val) } else { None };
                log.push(format!("loader error: tt={tt} {txt:?}"));
            }
            let _ = self.alvo.escrever(l + OFF_TOP, &top_antes.to_le_bytes());
            return Err(ErroMotor::Compilacao(status as i32));
        }
        let f = self.closure_no_topo(l, top_antes, log).ok_or_else(|| {
            ErroMotor::Io("load returned without a closure on top of the stack (tt != 8)".into())
        })?;
        // O undump do cliente reescreve o byte de opcode de TODAS as palavras do
        // array de código (inclusive as auxiliares), então o que ele deixa na
        // memória não é executável. Escrevemos por cima o nosso bytecode já
        // traduzido — é o que fecha o caminho.
        if std::env::var("LUARIA_SEM_PATCH").is_err() {
            let chunk = self.chunk_remapeado.borrow().clone();
            // Elevação de capability (desligada por padrão): grava a máscara cheia
            // num slot do stub e passa o ENDEREÇO dele para apontar cada proto+0x20.
            let caps_obj = if identidade_pedida() != 0 {
                let obj = freio.stub + OFF_CAPS_OBJ;
                self.alvo.escrever(obj, &CAPS_CHEIA.to_le_bytes()).map_err(|e| ErroMotor::Io(e.to_string()))?;
                log.push(format!("elevation: proto+{OFF_PROTO_CAPS:#x} -> {obj:#x} (full mask)"));
                Some(obj)
            } else {
                None
            };
            self.patchar_codigo(f, &chunk, caps_obj, log)?;
        }
        Ok((f, self.assinatura.borrow().clone()))
    }

    /// Reescreve as seções de código de todas as protos da closure.
    ///
    /// Layout medido no undump deste payload: `code` em
    /// `Proto+0x38`, `codeentry` em `Proto+0x28`, `p[]` em `Proto+0x60`,
    /// `sizecode` em `+0x88` e `sizep` em `+0xa8`.
    ///
    /// As filhas vêm na ordem do dump e o `p[]` guarda só as filhas diretas; o
    /// índice global de cada uma está no próprio chunk (`Proto::filhos`), então
    /// a descida usa esse índice — usar a posição dentro do `p[]` entregaria o
    /// código de outra proto assim que houvesse mais de uma filha.
    pub(crate) fn patchar_codigo(&self, f: u64, chunk: &[u8], caps_obj: Option<u64>, log: &mut Vec<String>) -> Result<(), ErroMotor> {
        let protos = crate::opcodes::percorrer(chunk)
            .map_err(|e| ErroMotor::Io(format!("chunk: {e}")))?;
        let principal = crate::opcodes::proto_principal(chunk).unwrap_or(0);
        let cl = self.alvo.u64_em(f).map_err(|e| ErroMotor::Io(e.to_string()))?;
        let raiz = self.alvo.u64_em(cl + 0x18).map_err(|e| ErroMotor::Io(e.to_string()))?;
        if !self.alvo.dentro_do_processo(raiz) {
            return Err(ErroMotor::Io(format!("main proto at {raiz:#x} is not a valid pointer")));
        }
        let mut pilha = vec![(principal, raiz, 0usize)];
        let mut feitos = vec![false; protos.len()];
        let mut total = 0usize;
        while let Some((indice, proto, profundidade)) = pilha.pop() {
            if indice >= protos.len() || feitos[indice] {
                continue;
            }
            if profundidade > MAX_PROFUNDIDADE {
                return Err(ErroMotor::Io(format!(
                    "proto {indice} na profundidade {profundidade}: limite {MAX_PROFUNDIDADE}"
                )));
            }
            if !self.alvo.dentro_do_processo(proto) {
                return Err(ErroMotor::Io(format!(
                    "proto {indice} at {proto:#x} outside the process ranges"
                )));
            }
            let Some(code) = chunk.get(protos[indice].codigo.clone()).map(|b| b.to_vec()) else { continue };
            for campo in [0x38u64, 0x28] {
                let destino = self.alvo.u64_em(proto + campo).unwrap_or(0);
                if self.alvo.gravavel(destino) {
                    self.alvo.escrever(destino, &code).map_err(|e| ErroMotor::Io(e.to_string()))?;
                }
            }
            feitos[indice] = true;
            total += 1;
            let sizep = self.alvo.u32_em(proto + 0xa8).unwrap_or(0) as usize;
            let p = self.alvo.u64_em(proto + 0x60).unwrap_or(0);
            // `sizecode`/`sizep` são u32 nesse Proto; ler 8 bytes pega o campo
            // vizinho e transforma o laço das filhas em lixo.
            let sizecode = self.alvo.u32_em(proto + 0x88).unwrap_or(0) as usize;
            if sizecode != protos[indice].sizecode {
                return Err(ErroMotor::Io(format!(
                    "proto {indice}: sizecode={sizecode} in process, {} in chunk (divergent layout)",
                    protos[indice].sizecode
                )));
            }
            // O sizecode conferir é a prova de que o layout do Proto bate; só
            // depois dela apontamos o campo de capability (proto+0x20) para o
            // objeto de máscara cheia. Com o teto do contexto cheio, isso tira o
            // sandbox do nosso chunk. `proto+0x20` é PONTEIRO, não máscara.
            if let Some(obj) = caps_obj {
                let campo = proto + OFF_PROTO_CAPS;
                if self.alvo.gravavel(campo) {
                    self.alvo.escrever(campo, &obj.to_le_bytes()).map_err(|e| ErroMotor::Io(e.to_string()))?;
                } else {
                    return Err(ErroMotor::Io(format!(
                        "proto {indice}: capability field at {campo:#x} is not writable (diverging layout?)"
                    )));
                }
            }
            if sizep != protos[indice].filhos.len() {
                return Err(ErroMotor::Io(format!(
                    "proto {indice}: sizep={sizep} in process, {} in chunk (divergent layout)",
                    protos[indice].filhos.len()
                )));
            }
            if std::env::var("LUARIA_DIAG").is_ok() {
                for off in (0x80..0xc0u64).step_by(8) {
                    log.push(format!(
                        "  proto{indice}+{off:#04x} = {:#018x}",
                        self.alvo.u64_em(proto + off).unwrap_or(0)
                    ));
                }
            }
            if sizep == 0 {
                continue;
            }
            if !self.alvo.dentro_do_processo(p) {
                return Err(ErroMotor::Io(format!(
                    "proto {indice}: p[] at {p:#x} outside the process ranges"
                )));
            }
            // o `p[]` do processo segue a mesma ordem das filhas no chunk
            for (j, filha) in protos[indice].filhos.iter().enumerate() {
                if j >= sizep {
                    break;
                }
                let ponteiro = self.alvo.u64_em(p + j as u64 * 8).unwrap_or(0);
                if !self.alvo.dentro_do_processo(ponteiro) {
                    return Err(ErroMotor::Io(format!(
                        "proto {indice}: child {filha} (p[{j}]) at {ponteiro:#x} outside the process"
                    )));
                }
                pilha.push((*filha, ponteiro, profundidade + 1));
            }
        }
        if std::env::var("LUARIA_DIAG").is_ok() {
            log.push(format!("patch: {total} of {} protos (main {principal})", protos.len()));
        }
        if total != protos.len() {
            return Err(ErroMotor::Io(format!(
                "only {total} of {} protos were fixed; not running half-patched code",
                protos.len()
            )));
        }
        Ok(())
    }

    /// Empilha uma closure C por nativa logo acima da closure do chunk; viram args do chunk.
    /// A folga física foi checada em fase_remota (SLOTS_CARGA + QTD), então o
    /// lua_checkstack interno do pushcclosurek nunca realoca. Se mesmo assim a
    /// pilha mudar de endereço, o deslocamento garante o `func` certo.
    fn empilhar_nativas(
        &self,
        sessao: &Sessao,
        l: u64,
        func: u64,
        fns: &[u64],
        trap: u64,
        log: &mut Vec<String>,
    ) -> Result<u64, ErroMotor> {
        let stack = self.alvo.u64_em(l + OFF_STACK).unwrap_or(0);
        let top = self.alvo.u64_em(l + OFF_TOP).unwrap_or(0);
        let (stack_last, nc) = self.ler_folga(l);
        checar_folga(stack, top, stack_last, nc, fns.len() as u64 + 1)
            .map_err(|m| ErroMotor::Io(format!("no stack headroom for the natives: {m}")))?;
        let (off_func, off_top) = (func - stack, top - stack);
        for f in fns {
            // pushcclosurek(L, fn, debugname=0, nup=0, cont=0)
            sessao
                .chamar(&self.alvo, self.alvo.base + nativas::LUA_PUSHCCLOSUREK.va, [l, *f, 0, 0, 0, 0], trap, 0)
                .map_err(|e| ErroMotor::Io(e.to_string()))?;
        }
        let stack_novo = self.alvo.u64_em(l + OFF_STACK).unwrap_or(0);
        let top_novo = self.alvo.u64_em(l + OFF_TOP).unwrap_or(0);
        if stack_novo != stack {
            log.push(format!("stack reallocated during natives: {stack:#x} -> {stack_novo:#x}"));
        }
        if top_novo.wrapping_sub(stack_novo) != off_top + 16 * fns.len() as u64 {
            return Err(ErroMotor::Io(format!("unexpected top after natives: {top_novo:#x}")));
        }
        log.push(format!("nativas: {} closures C empilhadas", fns.len()));
        Ok(stack_novo + off_func)
    }

    /// Chama a closure carregada e registra o que ela devolveu.
    fn chamar_closure(
        &self,
        sessao: &Sessao,
        tid: i32,
        l: u64,
        func: u64,
        top_antes: u64,
        assinatura: &[u8],
        nativas: Option<[u64; nativas::QTD as usize]>,
        trap: u64,
        log: &mut Vec<String>,
    ) -> Result<(), ErroMotor> {
        let stack_antes = self.alvo.u64_em(l + OFF_STACK).unwrap_or(0);
        log.push(format!("closure at {func:#x}, value {:#x}", self.alvo.u64_em(func).unwrap_or(0)));
        // O campo `code` do Proto muda de lugar entre builds da Roblox. Em vez de
        // chutar offset, procura na closure e no Proto um ponteiro cujo conteúdo
        // seja o começo do chunk compilado. Custa 0x200 leituras: só com LUARIA_DIAG.
        if std::env::var("LUARIA_DIAG").is_ok() && !assinatura.is_empty() {
            if let Some(cl) = self.alvo.u64_em(func).ok().filter(|v| *v != 0) {
                let proto = self.alvo.u64_em(cl + 0x18).unwrap_or(0);
                for (rotulo, obj) in [("closure", cl), ("proto", proto)] {
                    if obj < 0x1000 {
                        continue;
                    }
                    for off in (0..0x100u64).step_by(8) {
                        let Some(p) = self.alvo.u64_em(obj + off).ok().filter(|v| *v > 0x1000) else { continue };
                        let igual = self.alvo.bytes(p, assinatura.len()).map(|b| b == assinatura).unwrap_or(false);
                        if igual {
                            log.push(format!("chunk loaded at {p:#x} ({rotulo}+{off:#x})"));
                        }
                    }
                }
            }
        }
        if let Ok(cl) = self.alvo.u64_em(func) {
            let proto = self.alvo.u64_em(cl + 0x18).unwrap_or(0);
            let hdr = self.alvo.bytes(cl, 0x20).unwrap_or_default();
            log.push(format!("closure hdr: {}", hex(&hdr)));
            if proto != 0 {
                let code = self.alvo.u64_em(proto + 0x38).unwrap_or(0);
                log.push(format!("proto.code={code:#x} stacksize={} isC={}",
                    self.alvo.bytes(cl + 5, 1).map(|b| b[0]).unwrap_or(0xff),
                    self.alvo.bytes(cl + 6, 1).map(|b| b[0]).unwrap_or(0xff)));
                if code != 0 {
                    let ins = self.alvo.bytes(code, 32).unwrap_or_default();
                    log.push(format!("code[0..32]: {}", hex(&ins)));
                }
            }
        }
        if std::env::var("LUARIA_SEM_EXEC").is_ok() {
            return Ok(());
        }
        let func = match nativas {
            Some(fns) => match self.empilhar_nativas(sessao, l, func, &fns, trap, log) {
                Ok(f) => f,
                Err(e) => {
                    self.restaurar_topo(l, top_antes, stack_antes, log);
                    return Err(e);
                }
            },
            None => func,
        };
        // Estratégias de chamada, escolhidas por LUARIA_EST (default 1):
        //   1 = corpo de luaD_call (precall + luaV_execute) — o caminho completo
        //   2 = luaD_call (entrada hookada, transparente com o freio desarmado)
        //   3 = luaD_precall + luaV_execute separados
        // Script ainda não começou: se faltar folga para o proto inteiro, devolve o
        // topo e erra aqui, sem entrar no luaD_precall que cresceria a pilha.
        let maxstack = self
            .alvo
            .u64_em(func)
            .ok()
            .and_then(|cl| self.alvo.bytes(cl + 5, 1).ok())
            .map_or(0, |b| u64::from(b[0]));
        let stack = self.alvo.u64_em(l + OFF_STACK).unwrap_or(0);
        let top = self.alvo.u64_em(l + OFF_TOP).unwrap_or(0);
        let (stack_last, nc) = self.ler_folga(l);
        let necessarios = maxstack + MARGEM_EXEC_SLOTS;
        match checar_folga(stack, top, stack_last, nc, necessarios) {
            Ok(livres) => log.push(format!("maxstack={maxstack} livres={livres} necessarios={necessarios}")),
            Err(m) => {
                self.restaurar_topo(l, top_antes, stack_antes, log);
                return Err(ErroMotor::Io(format!("no stack headroom to run (maxstack={maxstack}): {m}")));
            }
        }
        let estrategia = std::env::var("LUARIA_EST").ok().and_then(|v| v.parse::<u32>().ok()).unwrap_or(1);
        log.push(format!("strategy {estrategia}"));
        let ret = match self.executar_estrategia(sessao, estrategia, 0, l, func, trap, log) {
            Ok(v) => v,
            Err(e) => {
                self.registrar_parada(tid, log);
                self.diagnostico_vm(l, log);
                self.restaurar_topo(l, top_antes, stack_antes, log);
                return Err(e);
            }
        };
        let top = self.alvo.u64_em(l + OFF_TOP).unwrap_or(0);
        log.push(format!("run ret={ret:#x}; top {top_antes:#x} -> {top:#x}"));
        self.restaurar_topo(l, top_antes, stack_antes, log);
        for (i, p) in (func..func + 16 * 3).step_by(16).enumerate() {
            let val = self.alvo.u64_em(p).unwrap_or(0);
            let tt = self.alvo.u32_em(p + OFF_TT).unwrap_or(0xff);
            match tt {
                1 => log.push(format!("  retorno[{i}]: booleano {}", if val != 0 { "true" } else { "false" })),
                6 => {
                    if let Some(txt) = self.ler_tstring(val) {
                        log.push(format!("  retorno[{i}]: string {txt:?}"));
                    }
                }
                _ => {}
            }
        }
        Ok(())
    }

    /// Devolve `L->top` ao valor de antes da carga.
    ///
    /// O `luaD_call` remoto deixa no topo a closure e os temporários do script.
    /// O `luaD_call` original do jogo (que o hook libera em seguida) empilharia
    /// por cima disso; a pilha cresce a cada execução e o cliente acaba
    /// morrendo no caminho de script dele (medido: SIGSEGV após ~5 execuções).
    /// O topo é a única coisa a restaurar: `ci`/`base` o próprio `luaD_call`
    /// devolve, e o valor da closure fica na memória abaixo do topo restaurado.
    pub(crate) fn restaurar_topo(&self, l: u64, top_antes: u64, stack_antes: u64, log: &mut Vec<String>) {
        let atual = self.alvo.u64_em(l + OFF_TOP).unwrap_or(0);
        // A pilha do Lua pode ser REALOCADA durante a chamada. Guardar o
        // endereço absoluto e devolvê-lo depois aponta para memória liberada —
        // era isso que derrubava o cliente logo após a primeira execução.
        // O que se preserva é o deslocamento em relação a `L->stack`.
        let stack = self.alvo.u64_em(l + OFF_STACK).unwrap_or(0);
        let novo = if stack_antes != 0 && stack != 0 {
            stack + top_antes.saturating_sub(stack_antes)
        } else {
            top_antes
        };
        if atual == novo {
            return;
        }
        if let Err(e) = self.alvo.escrever(l + OFF_TOP, &novo.to_le_bytes()) {
            log.push(format!("failed to restore the top: {e}"));
        } else {
            log.push(format!("top restored: {atual:#x} -> {novo:#x}"));
        }
    }

    pub(crate) fn registrar_parada(&self, tid: i32, log: &mut Vec<String>) {
        if let Ok((rip, rsp)) = regs_de(tid) {
            log.push(format!(
                "stopped at rip={rip:#x} (+{:#x} of payload) rsp={rsp:#x}",
                rip.saturating_sub(self.alvo.base)
            ));
        }
    }

    /// Estado da VM no momento da falha: ci, savedpc, primeiro opcode e o handler
    /// que a tabela de dispatch aponta para ele.
    fn diagnostico_vm(&self, l: u64, log: &mut Vec<String>) {
        let ci = self.alvo.u64_em(l + 0x10).unwrap_or(0);
        let top = self.alvo.u64_em(l + OFF_TOP).unwrap_or(0);
        let base = self.alvo.u64_em(l + OFF_BASE).unwrap_or(0);
        log.push(format!("  vm: ci={ci:#x} base={base:#x} top={top:#x}"));
        if ci == 0 {
            return;
        }
        let pc = self.alvo.u64_em(ci + 0x20).unwrap_or(0);
        let func = self.alvo.u64_em(ci + 0x8).unwrap_or(0);
        log.push(format!("  ci: func={func:#x} savedpc={pc:#x}"));
        if pc == 0 {
            return;
        }
        let insn = self.alvo.u32_em(pc).unwrap_or(0xffff_ffff);
        let op = (insn & 0xff) as u64;
        let tabela = self.alvo.u64_em(self.alvo.base + 0x70b8e20 + op * 8).unwrap_or(0);
        log.push(format!("  first instruction {insn:#010x} (op {op}) handler={tabela:#x}"));
        // a tabela de despacho do interpretador tem buracos: opcode fora do mapa
        // deste build cai em handler nulo e o despacho vai para o endereço 0
        let nulos = (0..110u64)
            .filter(|i| self.alvo.u64_em(self.alvo.base + 0x70b8e20 + i * 8).unwrap_or(0) == 0)
            .count();
        log.push(format!("  dispatch table: {nulos} of 110 indices without handler"));
    }

    pub(crate) fn executar_estrategia(
        &self,
        sessao: &Sessao,
        estrategia: u32,
        nresultados: u64,
        l: u64,
        func: u64,
        trap: u64,
        log: &mut Vec<String>,
    ) -> Result<u64, ErroMotor> {
        match estrategia {
            2 => sessao
                .chamar(&self.alvo, self.alvo.base + self.anchors.lua_d_call.unwrap_or(0), [l, func, nresultados, 0, 0, 0], trap, 0)
                .map_err(|e| ErroMotor::Io(e.to_string())),
            3 => {
                self.conferir_prologo(FN_PRECALL, &[0x55, 0x48, 0x89, 0xe5, 0x41, 0x57])?;
                let r = sessao
                    .chamar(&self.alvo, self.alvo.base + FN_PRECALL, [l, func, 0, 0, 0, 0], trap, 0)
                    .map_err(|e| ErroMotor::Io(e.to_string()))?;
                log.push(format!("precall ret={r}"));
                self.conferir_prologo(FN_EXECUTAR, &[0x80, 0x7f, 0x06, 0x00])?;
                sessao
                    .chamar(&self.alvo, self.alvo.base + FN_EXECUTAR, [l, 0, 0, 0, 0, 0], trap, 0)
                    .map_err(|e| ErroMotor::Io(e.to_string()))
            }
            _ => {
                self.conferir_prologo(FN_CORPO_CALL, &[0x55, 0x48, 0x89, 0xe5, 0x41, 0x57])?;
                sessao
                    .chamar(&self.alvo, self.alvo.base + FN_CORPO_CALL, [l, func, nresultados, 0, 0, 0], trap, 0)
                    .map_err(|e| ErroMotor::Io(e.to_string()))
            }
        }
    }

    /// Desarma a recusa de assinatura do `loadsafe` para chunks grandes.
    ///
    /// O site é a comparação com 1 logo depois do verificador.
    /// Com a constante trocada por 0x7f o desvio é sempre tomado, então o
    /// undump roda sem que o status "corrupted" derrube a carga. Só o compare
    /// muda: o verificador continua executando e nada mais no cliente depende
    /// do retorno dele (xref único, medido por payload).
    fn dispensar_verificacao(&self, log: &mut Vec<String>) -> Result<(), ErroMotor> {
        let site = self.alvo.base + SITE_VERIFICACAO;
        let atual = self.alvo.bytes(site, 4).map_err(|e| ErroMotor::Io(e.to_string()))?;
        if atual == BYTES_VERIFICACAO_PATCH {
            return Ok(());
        }
        if atual != BYTES_VERIFICACAO {
            return Err(ErroMotor::Io(format!(
                "verification site at +{SITE_VERIFICACAO:#x} does not match ({}); different payload",
                hex(&atual)
            )));
        }
        // uma escrita de 1 byte é atômica, mas parar as threads evita que uma
        // thread já dentro do compares veja o valor novo no meio da instrução
        let paradas = self.alvo.parar_com_guarda();
        let r = self.alvo.escrever(site + 2, &[0x7f]);
        drop(paradas);
        r.map_err(|e| ErroMotor::Io(e.to_string()))?;
        log.push("signature check skipped (chunk >= 280 bytes)".into());
        Ok(())
    }

    /// Confere o prólogo antes de chamar um endereço medido por payload: se o
    /// payload mudou, falha alto em vez de matar o cliente.
    fn conferir_prologo(&self, offset: u64, esperado: &[u8]) -> Result<(), ErroMotor> {
        let b = self
            .alvo
            .bytes(self.alvo.base + offset, esperado.len())
            .map_err(|e| ErroMotor::Io(e.to_string()))?;
        if b != esperado {
            return Err(ErroMotor::Io(format!(
                "prologue at +{offset:#x} does not match ({}), address measured in another payload",
                hex(&b)
            )));
        }
        Ok(())
    }

    /// A closure que o load empurrou: exatamente uma TValue acima do topo antigo,
    /// com tt de função (8, medido em runtime — fica em +0xc).
    fn closure_no_topo(&self, l: u64, top_antes: u64, log: &mut Vec<String>) -> Option<u64> {
        let top = self.alvo.u64_em(l + OFF_TOP).ok()?;
        log.push(format!("top after load: {top:#x} (before {top_antes:#x})"));
        if top != top_antes + 16 {
            log.push(format!("delta inesperado: {} slots", (top as i64 - top_antes as i64) / 16));
            return None;
        }
        let cand = top - 16;
        let tt = self.alvo.u32_em(cand + OFF_TT).ok()?;
        log.push(format!("TValue at {cand:#x}: value {:#x} tt={tt}", self.alvo.u64_em(cand).unwrap_or(0)));
        (tt == TT_FUNCAO).then_some(cand)
    }

    /// Lê uma `TString` de Lua (tt 6 no build medido): len em +0x14, dados em +0x18.
    fn ler_tstring(&self, ptr: u64) -> Option<String> {
        let len = self.alvo.u32_em(ptr + 0x14).ok()? as usize;
        if len == 0 || len > 4096 {
            return None;
        }
        let bytes = self.alvo.bytes(ptr + 0x18, len).ok()?;
        Some(String::from_utf8_lossy(&bytes).to_string())
    }

    /// A thread parada *dentro do laço de pausa* do stub. É a única que executa
    /// o bloco de spin (as demais saem na primeira checagem e pulam para `SPIN_FIM`), então
    /// é ela que segura os registradores da chamada freada.
    fn thread_em_pausa(&self, tids: &[i32], stub: u64) -> Option<i32> {
        for tid in tids {
            let Some(r) = regs_completos(*tid) else { continue };
            let Some(off) = r[16].checked_sub(stub) else { continue };
            if (crate::hook::SPIN_INICIO..crate::hook::SPIN_FIM).contains(&off) {
                return Some(*tid);
            }
        }
        None
    }
}

fn regs_completos(tid: i32) -> Option<[u64; 27]> {
    #[repr(C)]
    struct R {
        v: [u64; 27],
    }
    extern "C" {
        fn ptrace(req: i32, pid: i32, addr: *mut u8, data: *mut u8) -> i64;
    }
    let mut r = R { v: [0; 27] };
    let rc = unsafe { ptrace(12, tid, std::ptr::null_mut(), &mut r as *mut R as *mut u8) };
    (rc == 0).then_some(r.v)
}

fn regs_de(tid: i32) -> Result<(u64, u64), ()> {
    #[repr(C)]
    struct R {
        v: [u64; 27],
    }
    extern "C" {
        fn ptrace(req: i32, pid: i32, addr: *mut u8, data: *mut u8) -> i64;
    }
    let mut r = R { v: [0; 27] };
    unsafe {
        if ptrace(12, tid, std::ptr::null_mut(), &mut r as *mut R as *mut u8) != 0 {
            return Err(());
        }
    }
    Ok((r.v[16], r.v[19]))
}

const PROT_RW: u64 = 3;
const PROT_RWX: u64 = 7;

/// Cabeçalho do anel (MAGIC + VERSAO), o que o jogo confere antes de usar o anel.
fn cabecalho_anel() -> [u8; 12] {
    let mut c = [0u8; 12];
    c[..8].copy_from_slice(ponte_layout::MAGIC);
    c[8..].copy_from_slice(&ponte_layout::VERSAO.to_le_bytes());
    c
}

/// Os 8 parâmetros nativos ficam na MESMA linha do `function(` e o `, ...)` na do `end`,
/// senão `linha_do_usuario()` desloca. Args vêm de chamar_closure (empilhar_nativas).
fn embrulhar(corpo: &str) -> String {
    format!(
        "local __ok, __err = coroutine.resume(coroutine.create(function(__n_grm, __n_srm, __n_sro, __n_gk, __n_gu, __n_gg, __n_pe, __n_pr, __n_sk, __n_su, ...)\n{corpo}\nend), ...)\n\
         if not __ok then print(\"LUARIA_ERRO \" .. tostring(__err)) end\n"
    )
}

/// Compila o chunk para bytecode Luau (o caminho que o motor do jogo usa).
///
/// O binário vem de `LUARIA_LUAU_COMPILE`, de `~/.cache/luaria/luau-compile`, de
/// `tools/luau-compile` ou do PATH — nesta ordem. Precisa ser da **mesma release
/// do Luau** que a VM do cliente (release/735): o enum de opcodes muda entre
/// releases e só o mapa por nome sobrevive, então o compilador tem que falar a
/// mesma língua. `tools/luau.sh` monta esse binário.
pub fn compilar(fonte: &str) -> Result<Vec<u8>, String> {
    let exe = match std::env::var("LUARIA_LUAU_COMPILE") {
        Ok(p) => PathBuf::from(p),
        Err(_) => {
            let cache = std::env::var("HOME")
                .map(|h| PathBuf::from(h).join(".cache/luaria/luau-compile"))
                .unwrap_or_default();
            let local = PathBuf::from("tools/luau-compile");
            if cache.exists() {
                cache
            } else if local.exists() {
                local
            } else {
                PathBuf::from("luau-compile")
            }
        }
    };
    let arq = std::env::temp_dir().join(format!("luaria-{}.lua", std::process::id()));
    std::fs::write(&arq, fonte).map_err(|e| format!("{exe:?}: {e}"))?;
    // -O0 de propósito: com -O1+ o compilador emite FASTCALL1 para builtins
    // (`type`, etc.) e a VM do cliente executa o builtin errado (medido:
    // `type(x) == "table"` vira "attempt to index nil with 'table'").
    // Os FFlags abaixo sobem a versão do bytecode (fastpcall=14, costmodel=12,
    // callfeedback=11). A VM do cliente é a release/735 mas o *formato* que ela
    // aceita é o do release base: emitir 9 evita LOP_FASTPCALL/LOP_CALLFB e o
    // caminho de tipos mais novo. Ajustável por LUARIA_FFLAGS.
    let fflags = std::env::var("LUARIA_FFLAGS").unwrap_or_else(|_| {
        "LuauCompileFastpcall=false,LuauEmitCallFeedback=false,LuauBytecodeCostModel=false,LuauCompileEmitVectorDouble=false"
            .to_string()
    });
    let saida = std::process::Command::new(&exe)
        .args([
            "--binary",
            &format!("-O{}", std::env::var("LUARIA_LUAU_O").unwrap_or_else(|_| "0".into())),
            &format!("--fflags={fflags}"),
        ])
        .arg(&arq)
        .output()
        .map_err(|e| format!("{exe:?}: {e}"))?;
    let _ = std::fs::remove_file(&arq);
    if !saida.status.success() {
        return Err(String::from_utf8_lossy(&saida.stderr).trim().to_string());
    }
    if saida.stdout.is_empty() {
        return Err("compilador devolveu bytecode vazio".into());
    }
    Ok(saida.stdout)
}

#[cfg(test)]
mod testes {
    use super::*;

    #[test]
    fn identidade_padrao_e_lixo_desligam() {
        assert_eq!(identidade_de(None), 8);
        assert_eq!(identidade_de(Some("abc")), 0);
        assert_eq!(identidade_de(Some("-1")), 0);
        assert_eq!(identidade_de(Some(" 8 ")), 8);
        assert_eq!(identidade_de(Some("0")), 0);
    }

    /// O slot do objeto de capability não pode pisar nos slots que o stub usa.
    #[test]
    fn embrulho_nao_desloca_linha_do_usuario() {
        let corpo = format!(
            "local __luaria_arquivos = {{}}\n{}\n{}__USUARIO__",
            crate::prelude::completo(),
            crate::prelude::ponte_globais("x")
        );
        let fonte = embrulhar(&corpo);
        assert!(fonte.contains("function(__n_grm, __n_srm, __n_sro, __n_gk, __n_gu, __n_gg, __n_pe, __n_pr, __n_sk, __n_su, ...)"));
        let linha = fonte.lines().position(|l| l.contains("__USUARIO__")).unwrap() + 1;
        assert_eq!(linha, linha_do_usuario());
    }

    #[test]
    fn embrulho_compila() {
        match compilar(&embrulhar("print(__n_grm, ...)")) {
            Err(e) if e.contains("os error 2") => eprintln!("luau-compile indisponível"),
            r => assert!(r.is_ok(), "{r:?}"),
        }
    }

    #[test]
    fn slot_de_caps_livre_no_stub() {
        use crate::hook::{OFF_ARGS, OFF_RETVAL};
        assert!(OFF_CAPS_OBJ >= OFF_RETVAL + 8 && OFF_CAPS_OBJ + 8 <= OFF_ARGS);
        assert!(OFF_REGIAO >= OFF_CAPS_OBJ + 8 && OFF_REGIAO + 8 <= OFF_ARGS);
        assert!(OFF_PONTE_PAGINA >= OFF_REGIAO + 8 && OFF_PONTE_ANEL + 8 <= OFF_NOME);
    }

    /// O prelude entra em todo chunk: se ele não compilar ou o parser de protos
    /// não der conta das funções aninhadas dele, nada roda.
    #[test]
    fn prelude_compila_e_parseia() {
        let fonte = format!(
            "local __luaria_arquivos = {{}}\n{}\n{}print(\"ok\")",
            crate::prelude::completo(),
            crate::prelude::ponte_globais("Settings = 1 print(Key)")
        );
        let b = match compilar(&fonte) {
            Ok(b) => b,
            // só pula sem o binário; erro de sintaxe no prelude tem que falhar
            Err(e) if e.contains("os error 2") => {
                eprintln!("luau-compile indisponível neste ambiente: {e}");
                return;
            }
            Err(e) => panic!("prelude não compila: {e}"),
        };
        // folga para o script do usuário e o instantâneo de arquivos na região de 2 MB
        assert!(b.len() < 400 * 1024, "bytecode do prelude = {} bytes", b.len());
        let code = crate::opcodes::code_section(&b, 0).expect("parser não achou o proto principal");
        assert!(!code.is_empty());
    }
}
