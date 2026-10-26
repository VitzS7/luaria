//! Poller do host para o canal jogo→host (anel descrito em `ponte_layout`).
//!
//! Protocolo de escritor único por palavra: o jogo escreve dono, g_req, g_cons,
//! tipo, len_req e flags; o host escreve h_ack, h_resp, status, len_resp e h_pulso.
//! Por isso não há atômicos: cada escrita é um writev separado, e a ordem
//! (corpo, status/len, h_resp) é o que faz o jogo ler algo completo.

use std::fs::{File, OpenOptions};
use std::io::{self, ErrorKind};
use std::os::unix::io::AsRawFd;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::sync::Arc;
use std::thread::{self, JoinHandle, Scope};
use std::time::{Duration, Instant};

use crate::ponte_codec::PedidoHttp;
use crate::ponte_layout as anel;
use crate::remoto;

pub const STATUS_TIPO_DESCONHECIDO: u32 = 0xFFFF_0001;
pub const STATUS_REQ_INVALIDA: u32 = 0xFFFF_0002;
pub const STATUS_RESP_TRUNCADA: u32 = 0xFFFF_0003;
pub const STATUS_ATENDIMENTO_FALHOU: u32 = 0xFFFF_0004;

/// Cabeçalho global mais os 8 cabeçalhos de slot: o que o poller lê por passada.
const TAM_CABECALHO: usize = anel::OFF_SLOTS + anel::N_SLOTS * anel::TAM_CAB_SLOT;
const TIPOS_CONHECIDOS: [u32; 6] = [
    anel::TIPO_HTTP,
    anel::TIPO_WS,
    anel::TIPO_CLIP_ESCREVER,
    anel::TIPO_CLIP_LER,
    anel::TIPO_CONSOLE,
    anel::TIPO_FILA_TELEPORT,
];
/// Por que 100 ms: o jogo só usa h_pulso para ver que o host está vivo.
const INTERVALO_PULSO: Duration = Duration::from_millis(100);
const INTERVALO_ATIVO: Duration = Duration::from_millis(8);
const INTERVALO_OCIOSO: Duration = Duration::from_millis(40);
const JANELA_ATIVA: Duration = Duration::from_secs(2);
/// Por que 60 s: o jogo consome a resposta logo ao retomar a coroutine; slot parado por esse tempo
/// foi abandonado (ex.: script que errou no meio). Um script lento que demora mais que isso perde a resposta.
const PRAZO_CONSUMO: Duration = Duration::from_secs(60);

extern "C" {
    fn process_vm_readv(pid: i32, local: *const Iovec, liovcnt: usize, remote: *const Iovec, riovcnt: usize, flags: u64) -> isize;
    fn process_vm_writev(pid: i32, local: *const Iovec, liovcnt: usize, remote: *const Iovec, riovcnt: usize, flags: u64) -> isize;
    fn flock(fd: i32, operacao: i32) -> i32;
}

#[repr(C)]
struct Iovec {
    base: *mut u8,
    len: usize,
}

const LOCK_EX: i32 = 2;
const LOCK_NB: i32 = 4;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RespostaPonte {
    pub status: u32,
    pub corpo: Vec<u8>,
}

impl RespostaPonte {
    fn erro(status: u32) -> Self {
        RespostaPonte { status, corpo: Vec::new() }
    }
}

#[derive(Debug)]
pub enum ErroPonte {
    Io(io::Error),
    CabecalhoInvalido,
    JaAtendido,
}

impl From<io::Error> for ErroPonte {
    fn from(e: io::Error) -> Self {
        ErroPonte::Io(e)
    }
}

struct Concluido {
    slot: usize,
    seq: u32,
    sem_resposta: bool,
    resposta: RespostaPonte,
}

pub struct Ponte {
    pid: i32,
    anel: u64,
    pulso: u32,
    ultimo_pulso: Option<Instant>,
    /// Pedidos despachados e ainda não concluídos; cada slot tem no máximo um, então <= N_SLOTS.
    em_voo: usize,
    /// Pedidos aceitos desde o início; o `Vigia` lê para saber se o jogo está ocioso.
    atendidos: Arc<AtomicUsize>,
    /// Por slot: (seq, instante em que o host viu Pronta sem consumo) para achar slot abandonado.
    presos: [Option<(u32, Instant)>; anel::N_SLOTS],
    prazo: Duration,
    envio: Sender<Concluido>,
    chegada: Receiver<Concluido>,
}

impl Ponte {
    pub fn nova(pid: i32, anel: u64) -> Self {
        let (envio, chegada) = mpsc::channel();
        Ponte {
            pid,
            anel,
            pulso: 0,
            ultimo_pulso: None,
            em_voo: 0,
            atendidos: Arc::default(),
            presos: [None; anel::N_SLOTS],
            prazo: PRAZO_CONSUMO,
            envio,
            chegada,
        }
    }

    /// Uma passada: devolve quantos pedidos foram aceitos e só retorna depois de gravar as respostas deles.
    pub fn passo(&mut self, atender: &(dyn Fn(u32, &[u8]) -> RespostaPonte + Sync)) -> Result<usize, ErroPonte> {
        thread::scope(|s| {
            let aceitos = self.passo_em(s, atender)?;
            self.esperar_voo()?;
            Ok(aceitos)
        })
    }

    /// Zera os cabeçalhos dos slots. Só chamar sem `Vigia` ativo no mesmo pid.
    pub fn reiniciar_slots(&mut self) -> io::Result<()> {
        self.presos = [None; anel::N_SLOTS];
        self.escrever(anel::OFF_SLOTS, &[0u8; anel::N_SLOTS * anel::TAM_CAB_SLOT])
    }

    /// Os workers saem do scope, então o despacho fica dentro dele e a conclusão é feita pelo poller.
    fn passo_em<'s, 'env: 's>(
        &mut self,
        s: &'s Scope<'s, 'env>,
        atender: &'env (dyn Fn(u32, &[u8]) -> RespostaPonte + Sync + 'env),
    ) -> Result<usize, ErroPonte> {
        self.drenar()?;
        let mut aceitos = 0;
        let cab = self.ler(0, TAM_CABECALHO)?;
        if cab[..8] != anel::MAGIC[..] || u32_em(&cab, anel::OFF_VERSAO) != anel::VERSAO {
            return Err(ErroPonte::CabecalhoInvalido);
        }
        self.pulsar()?;
        for slot in 0..anel::N_SLOTS {
            let c = anel::off_cab_slot(slot);
            let g_req = u32_em(&cab, c + anel::SLOT_G_REQ);
            let situacao = anel::estado(
                g_req,
                u32_em(&cab, c + anel::SLOT_G_CONS),
                u32_em(&cab, c + anel::SLOT_H_ACK),
                u32_em(&cab, c + anel::SLOT_H_RESP),
            );
            self.soltar_se_abandonado(slot, situacao, g_req)?;
            if situacao != anel::EstadoSlot::Pendente {
                continue;
            }
            aceitos += 1;
            self.atendidos.fetch_add(1, Ordering::Relaxed);
            self.escrever(c + anel::SLOT_H_ACK, &g_req.to_le_bytes())?;

            let tipo = u32_em(&cab, c + anel::SLOT_TIPO);
            let len = u32_em(&cab, c + anel::SLOT_LEN_REQ) as usize;
            if len > anel::MAX_REQ {
                self.concluir(slot, g_req, false, RespostaPonte::erro(STATUS_REQ_INVALIDA))?;
                continue;
            }
            if !TIPOS_CONHECIDOS.contains(&tipo) {
                self.concluir(slot, g_req, false, RespostaPonte::erro(STATUS_TIPO_DESCONHECIDO))?;
                continue;
            }

            let req = self.ler(anel::off_req(slot), len)?;
            let sem_resposta = u32_em(&cab, c + anel::SLOT_FLAGS) & anel::FLAG_SEM_RESPOSTA != 0;
            self.em_voo += 1;
            let envio = self.envio.clone();
            s.spawn(move || {
                // Sem catch_unwind um pânico no atendimento deixaria o slot para sempre em voo.
                let resposta = catch_unwind(AssertUnwindSafe(|| atender(tipo, &req)))
                    .unwrap_or_else(|_| RespostaPonte::erro(STATUS_ATENDIMENTO_FALHOU));
                envio
                    .send(Concluido { slot, seq: g_req, sem_resposta, resposta })
                    .expect("poller vive enquanto o scope roda");
            });
        }
        Ok(aceitos)
    }

    /// Por que escrever g_cons aqui é seguro: o valor é o mesmo que o jogo grava ao consumir, e o
    /// jogo não reusa o slot enquanto g_cons != g_req. Resíduo: se o jogo lê a resposta exatamente
    /// nesse instante, vê o slot como cancelado e perde a resposta.
    fn soltar_se_abandonado(&mut self, slot: usize, situacao: anel::EstadoSlot, g_req: u32) -> io::Result<()> {
        if situacao != anel::EstadoSlot::Pronta {
            self.presos[slot] = None;
            return Ok(());
        }
        let agora = Instant::now();
        match self.presos[slot] {
            Some((seq, desde)) if seq == g_req => {
                if agora.duration_since(desde) >= self.prazo {
                    self.presos[slot] = None;
                    self.escrever(anel::off_cab_slot(slot) + anel::SLOT_G_CONS, &g_req.to_le_bytes())?;
                }
            }
            _ => self.presos[slot] = Some((g_req, agora)),
        }
        Ok(())
    }

    fn drenar(&mut self) -> Result<(), ErroPonte> {
        while let Ok(concluido) = self.chegada.try_recv() {
            self.finalizar(concluido)?;
        }
        Ok(())
    }

    /// Pulsa enquanto espera: sem isso um atendimento lento (HTTP > 1,5 s) faz o jogo achar que o host morreu.
    fn esperar_voo(&mut self) -> Result<(), ErroPonte> {
        while self.em_voo > 0 {
            self.pulsar()?;
            match self.chegada.recv_timeout(INTERVALO_PULSO) {
                Ok(concluido) => self.finalizar(concluido)?,
                Err(RecvTimeoutError::Timeout) => {}
                Err(RecvTimeoutError::Disconnected) => panic!("poller vive enquanto o scope roda"),
            }
        }
        Ok(())
    }

    fn finalizar(&mut self, concluido: Concluido) -> Result<(), ErroPonte> {
        self.em_voo -= 1;
        self.concluir(concluido.slot, concluido.seq, concluido.sem_resposta, concluido.resposta)
    }

    /// Por que checar `g_cons` aqui: se o jogo cancelou durante o atendimento, ele não lê mais
    /// corpo nem status, então só o h_resp precisa sair.
    fn concluir(&self, slot: usize, seq: u32, sem_resposta: bool, resposta: RespostaPonte) -> Result<(), ErroPonte> {
        let c = anel::off_cab_slot(slot);
        let cancelado = u32_em(&self.ler(c + anel::SLOT_G_CONS, 4)?, 0) == seq;
        if !sem_resposta && !cancelado {
            self.gravar_resposta(slot, resposta)?;
        }
        self.escrever(c + anel::SLOT_H_RESP, &seq.to_le_bytes())?;
        Ok(())
    }

    fn gravar_resposta(&self, slot: usize, resposta: RespostaPonte) -> io::Result<()> {
        let (status, corpo) = if resposta.corpo.len() > anel::MAX_RESP {
            (STATUS_RESP_TRUNCADA, &resposta.corpo[..anel::MAX_RESP])
        } else {
            (resposta.status, &resposta.corpo[..])
        };
        if !corpo.is_empty() {
            self.escrever(anel::off_resp(slot), corpo)?;
        }
        // status e len_resp são 8 bytes contíguos: um writev só.
        let mut cab = [0u8; 8];
        cab[..4].copy_from_slice(&status.to_le_bytes());
        cab[4..].copy_from_slice(&(corpo.len() as u32).to_le_bytes());
        self.escrever(anel::off_cab_slot(slot) + anel::SLOT_STATUS, &cab)
    }

    fn pulsar(&mut self) -> io::Result<()> {
        let agora = Instant::now();
        if let Some(ultimo) = self.ultimo_pulso {
            if agora.duration_since(ultimo) < INTERVALO_PULSO {
                return Ok(());
            }
        }
        self.pulso = self.pulso.wrapping_add(1);
        self.ultimo_pulso = Some(agora);
        self.escrever(anel::OFF_H_PULSO, &self.pulso.to_le_bytes())
    }

    fn ler(&self, off: usize, len: usize) -> io::Result<Vec<u8>> {
        let mut buf = vec![0u8; len];
        let local = Iovec { base: buf.as_mut_ptr(), len };
        let remoto = Iovec { base: (self.anel + off as u64) as *mut u8, len };
        let n = unsafe { process_vm_readv(self.pid, &local, 1, &remoto, 1, 0) };
        checar_transferencia(n, len)?;
        Ok(buf)
    }

    fn escrever(&self, off: usize, bytes: &[u8]) -> io::Result<()> {
        let local = Iovec { base: bytes.as_ptr() as *mut u8, len: bytes.len() };
        let remoto = Iovec { base: (self.anel + off as u64) as *mut u8, len: bytes.len() };
        let n = unsafe { process_vm_writev(self.pid, &local, 1, &remoto, 1, 0) };
        checar_transferencia(n, bytes.len())
    }
}

fn checar_transferencia(n: isize, esperado: usize) -> io::Result<()> {
    if n < 0 {
        return Err(io::Error::last_os_error());
    }
    if n as usize != esperado {
        return Err(io::Error::new(ErrorKind::UnexpectedEof, "process_vm_* transferiu menos que o pedido"));
    }
    Ok(())
}

fn u32_em(bytes: &[u8], off: usize) -> u32 {
    u32::from_le_bytes([bytes[off], bytes[off + 1], bytes[off + 2], bytes[off + 3]])
}

/// Atende os tipos ligados ao host. Por ora só HTTP; os demais respondem `STATUS_TIPO_DESCONHECIDO`.
/// Roda no worker do poller, então o curl pode bloquear sem travar a passada.
pub fn atender_padrao(tipo: u32, req: &[u8]) -> RespostaPonte {
    if tipo != anel::TIPO_HTTP {
        return RespostaPonte::erro(STATUS_TIPO_DESCONHECIDO);
    }
    let pedido = match PedidoHttp::decodificar(req) {
        Ok(pedido) => pedido,
        Err(e) => return erro_texto(e.to_string()),
    };
    match remoto::pedir_http(&pedido) {
        Ok(resposta) => {
            let corpo = resposta.codificar();
            if corpo.len() > anel::MAX_RESP {
                return erro_texto("response too large".into());
            }
            RespostaPonte { status: resposta.status, corpo }
        }
        Err(mensagem) => erro_texto(mensagem),
    }
}

/// Status 0 com a mensagem em texto puro: o Lua trata o erro sem decodificar o corpo.
fn erro_texto(mensagem: String) -> RespostaPonte {
    RespostaPonte { status: 0, corpo: mensagem.into_bytes() }
}

/// Thread de atendimento contínuo. Um poller por pid: o lock é o arquivo `/tmp`, que sai com o fd.
pub struct Vigia {
    parada: Option<Sender<()>>,
    thread: Option<JoinHandle<Result<(), ErroPonte>>>,
    atendidos: Arc<AtomicUsize>,
    _trava: File,
}

impl Vigia {
    pub fn iniciar<F>(pid: i32, anel: u64, atender: F) -> Result<Vigia, ErroPonte>
    where
        F: Fn(u32, &[u8]) -> RespostaPonte + Send + Sync + 'static,
    {
        let trava = travar_pid(pid)?;
        let (parada, ouvir_parada) = mpsc::channel();
        let ponte = Ponte::nova(pid, anel);
        let atendidos = ponte.atendidos.clone();
        let thread = thread::spawn(move || ponte.laco(&atender, ouvir_parada));
        Ok(Vigia { parada: Some(parada), thread: Some(thread), atendidos, _trava: trava })
    }

    pub fn terminou(&self) -> bool {
        self.thread.as_ref().map_or(true, |h| h.is_finished())
    }

    pub fn pedidos_atendidos(&self) -> usize {
        self.atendidos.load(Ordering::Relaxed)
    }

    /// Erro devolvido aqui é o que fez o poller parar sozinho (ex.: pid morreu).
    pub fn parar(mut self) -> Result<(), ErroPonte> {
        self.parada.take();
        self.thread.take().expect("parar consome o Vigia, a thread ainda está lá").join().expect("poller entrou em pânico")
    }
}

impl Drop for Vigia {
    fn drop(&mut self) {
        // Por que join aqui: o lock do pid só pode sair depois que o poller parou de escrever.
        self.parada.take();
        if let Some(h) = self.thread.take() {
            // O pânico já foi impresso pelo hook padrão; Drop não tem como propagá-lo.
            let _ = h.join();
        }
    }
}

impl Ponte {
    fn laco(mut self, atender: &(dyn Fn(u32, &[u8]) -> RespostaPonte + Sync), parada: Receiver<()>) -> Result<(), ErroPonte> {
        let mut ultima_atividade: Option<Instant> = None;
        thread::scope(|s| {
            let resultado = loop {
                let agora = Instant::now();
                match self.passo_em(s, atender) {
                    Ok(n) if n > 0 => ultima_atividade = Some(agora),
                    Ok(_) => {}
                    Err(e) => break Err(e),
                }
                let ativo = ultima_atividade.map_or(false, |t| agora.duration_since(t) < JANELA_ATIVA);
                let intervalo = if ativo { INTERVALO_ATIVO } else { INTERVALO_OCIOSO };
                match parada.recv_timeout(intervalo) {
                    Err(RecvTimeoutError::Timeout) => {}
                    _ => break Ok(()),
                }
            };
            // Workers em voo ainda precisam do h_resp, senão o slot fica preso.
            resultado.and(self.esperar_voo())
        })
    }
}

fn travar_pid(pid: i32) -> Result<File, ErroPonte> {
    let arquivo = OpenOptions::new()
        .create(true)
        .write(true)
        .open(format!("/tmp/luaria-ponte-{pid}.lock"))?;
    if unsafe { flock(arquivo.as_raw_fd(), LOCK_EX | LOCK_NB) } == 0 {
        return Ok(arquivo);
    }
    let e = io::Error::last_os_error();
    if e.kind() == ErrorKind::WouldBlock {
        Err(ErroPonte::JaAtendido)
    } else {
        Err(ErroPonte::Io(e))
    }
}

#[cfg(test)]
mod testes {
    use super::*;
    use std::os::raw::c_void;
    use std::ptr::{copy_nonoverlapping, null_mut};
    use std::sync::atomic::{AtomicUsize, Ordering};

    extern "C" {
        fn mmap(end: *mut c_void, len: usize, prot: i32, flags: i32, fd: i32, off: i64) -> *mut c_void;
        fn munmap(end: *mut c_void, len: usize) -> i32;
    }
    const PROT_RW: i32 = 3;
    const MAP_PRIVADO_ANON: i32 = 0x22;
    const MAP_FALHOU: *mut c_void = usize::MAX as *mut c_void;

    /// Região de teste no próprio processo: o poller lê e escreve via process_vm_* no próprio pid.
    struct Anel {
        base: *mut u8,
    }

    // Só o teste acessa o ponteiro; o poller de teste usa o mesmo mmap de forma coordenada.
    unsafe impl Send for Anel {}
    unsafe impl Sync for Anel {}

    impl Anel {
        fn novo() -> Anel {
            let p = unsafe { mmap(null_mut(), anel::TAM_ANEL, PROT_RW, MAP_PRIVADO_ANON, -1, 0) };
            assert!(p != MAP_FALHOU, "mmap falhou");
            let a = Anel { base: p as *mut u8 };
            a.escrever_bruto(anel::OFF_MAGIC, anel::MAGIC);
            a.escrever_bruto(anel::OFF_VERSAO, &anel::VERSAO.to_le_bytes());
            a
        }

        fn addr(&self) -> u64 {
            self.base as u64
        }

        fn escrever_bruto(&self, off: usize, bytes: &[u8]) {
            unsafe { copy_nonoverlapping(bytes.as_ptr(), self.base.add(off), bytes.len()) }
        }

        fn u32_em(&self, off: usize) -> u32 {
            unsafe { (self.base.add(off) as *const u32).read_unaligned() }
        }

        fn set_u32(&self, off: usize, valor: u32) {
            self.escrever_bruto(off, &valor.to_le_bytes());
        }

        fn bytes(&self, off: usize, len: usize) -> Vec<u8> {
            unsafe { std::slice::from_raw_parts(self.base.add(off), len).to_vec() }
        }

        fn cab(&self, slot: usize, campo: usize) -> u32 {
            self.u32_em(anel::off_cab_slot(slot) + campo)
        }

        /// Lado do jogo: payload primeiro, g_req por último (publica o pedido).
        fn pedir(&self, slot: usize, seq: u32, tipo: u32, flags: u32, corpo: &[u8]) {
            let c = anel::off_cab_slot(slot);
            self.escrever_bruto(anel::off_req(slot), corpo);
            self.set_u32(c + anel::SLOT_TIPO, tipo);
            self.set_u32(c + anel::SLOT_LEN_REQ, corpo.len() as u32);
            self.set_u32(c + anel::SLOT_FLAGS, flags);
            self.set_u32(c + anel::SLOT_G_REQ, seq);
        }

        /// Lado do jogo: reaproveita o slot depois de consumir a resposta anterior.
        fn consumir(&self, slot: usize, seq: u32) {
            self.set_u32(anel::off_cab_slot(slot) + anel::SLOT_G_CONS, seq);
        }
    }

    impl Drop for Anel {
        fn drop(&mut self) {
            unsafe { munmap(self.base as *mut c_void, anel::TAM_ANEL) };
        }
    }

    fn ponte_de(a: &Anel) -> Ponte {
        Ponte::nova(std::process::id() as i32, a.addr())
    }

    fn eco(_tipo: u32, req: &[u8]) -> RespostaPonte {
        RespostaPonte { status: 200, corpo: req.to_vec() }
    }

    #[test]
    fn pedido_ok_grava_resposta_e_pulso() {
        let a = Anel::novo();
        let mut p = ponte_de(&a);
        a.pedir(0, 1, anel::TIPO_HTTP, 0, b"ola");
        assert_eq!(p.passo(&eco).unwrap(), 1);
        assert_eq!(a.cab(0, anel::SLOT_H_ACK), 1);
        assert_eq!(a.cab(0, anel::SLOT_H_RESP), 1);
        assert_eq!(a.cab(0, anel::SLOT_STATUS), 200);
        assert_eq!(a.cab(0, anel::SLOT_LEN_RESP), 3);
        assert_eq!(a.bytes(anel::off_resp(0), 3), b"ola");
        assert_eq!(a.u32_em(anel::OFF_H_PULSO), 1);
    }

    #[test]
    fn cancelado_so_recebe_h_resp() {
        let a = Anel::novo();
        let mut p = ponte_de(&a);
        a.pedir(0, 1, anel::TIPO_HTTP, 0, b"ola");
        // O jogo cancela enquanto o atendimento roda.
        p.passo(&|_, _| {
            a.consumir(0, 1);
            RespostaPonte { status: 200, corpo: b"xxx".to_vec() }
        })
        .unwrap();
        assert_eq!(a.cab(0, anel::SLOT_H_RESP), 1);
        assert_eq!(a.cab(0, anel::SLOT_STATUS), 0);
        assert_eq!(a.bytes(anel::off_resp(0), 3), [0, 0, 0]);
    }

    #[test]
    fn wrap_do_u32_no_seq() {
        let a = Anel::novo();
        let mut p = ponte_de(&a);
        a.pedir(0, u32::MAX - 1, anel::TIPO_HTTP, 0, b"a");
        p.passo(&eco).unwrap();
        a.consumir(0, u32::MAX - 1);
        a.pedir(0, u32::MAX, anel::TIPO_HTTP, 0, b"b");
        p.passo(&eco).unwrap();
        assert_eq!(a.cab(0, anel::SLOT_H_RESP), u32::MAX);
        a.consumir(0, u32::MAX);
        a.pedir(0, 0, anel::TIPO_HTTP, 0, b"c");
        p.passo(&eco).unwrap();
        assert_eq!(a.cab(0, anel::SLOT_H_RESP), 0);
        assert_eq!(a.bytes(anel::off_resp(0), 1), b"c");
    }

    #[test]
    fn len_gigante_recusada_sem_executar() {
        let a = Anel::novo();
        let mut p = ponte_de(&a);
        let chamadas = AtomicUsize::new(0);
        a.pedir(0, 1, anel::TIPO_HTTP, 0, b"x");
        a.set_u32(anel::off_cab_slot(0) + anel::SLOT_LEN_REQ, anel::MAX_REQ as u32 + 1);
        p.passo(&|_, _| {
            chamadas.fetch_add(1, Ordering::SeqCst);
            eco(0, &[])
        })
        .unwrap();
        assert_eq!(chamadas.load(Ordering::SeqCst), 0);
        assert_eq!(a.cab(0, anel::SLOT_STATUS), STATUS_REQ_INVALIDA);
        assert_eq!(a.cab(0, anel::SLOT_H_RESP), 1);
    }

    #[test]
    fn tipo_desconhecido_responde_erro() {
        let a = Anel::novo();
        let mut p = ponte_de(&a);
        let chamadas = AtomicUsize::new(0);
        a.pedir(0, 1, 99, 0, b"x");
        p.passo(&|_, _| {
            chamadas.fetch_add(1, Ordering::SeqCst);
            eco(0, &[])
        })
        .unwrap();
        assert_eq!(chamadas.load(Ordering::SeqCst), 0);
        assert_eq!(a.cab(0, anel::SLOT_STATUS), STATUS_TIPO_DESCONHECIDO);
        assert_eq!(a.cab(0, anel::SLOT_H_RESP), 1);
    }

    #[test]
    fn oito_pedidos_rodam_em_paralelo() {
        let a = Anel::novo();
        let mut p = ponte_de(&a);
        for i in 0..anel::N_SLOTS {
            a.pedir(i, 1, anel::TIPO_HTTP, 0, &[i as u8]);
        }
        let ativos = AtomicUsize::new(0);
        let pico = AtomicUsize::new(0);
        p.passo(&|_, req| {
            let agora = ativos.fetch_add(1, Ordering::SeqCst) + 1;
            pico.fetch_max(agora, Ordering::SeqCst);
            thread::sleep(Duration::from_millis(30));
            ativos.fetch_sub(1, Ordering::SeqCst);
            eco(0, req)
        })
        .unwrap();
        for i in 0..anel::N_SLOTS {
            assert_eq!(a.cab(i, anel::SLOT_H_RESP), 1);
            assert_eq!(a.bytes(anel::off_resp(i), 1), [i as u8]);
        }
        let pico = pico.load(Ordering::SeqCst);
        assert!(pico > 1 && pico <= anel::N_SLOTS, "pico de concorrência {pico}");
    }

    #[test]
    fn sem_resposta_so_recebe_h_resp() {
        let a = Anel::novo();
        let mut p = ponte_de(&a);
        let chamadas = AtomicUsize::new(0);
        a.pedir(0, 1, anel::TIPO_CONSOLE, anel::FLAG_SEM_RESPOSTA, b"x");
        p.passo(&|_, _| {
            chamadas.fetch_add(1, Ordering::SeqCst);
            RespostaPonte { status: 500, corpo: b"y".to_vec() }
        })
        .unwrap();
        assert_eq!(chamadas.load(Ordering::SeqCst), 1);
        assert_eq!(a.cab(0, anel::SLOT_H_RESP), 1);
        assert_eq!(a.cab(0, anel::SLOT_STATUS), 0);
        assert_eq!(a.bytes(anel::off_resp(0), 1), [0]);
    }

    #[test]
    fn corpo_maior_que_max_resp_trunca_com_status_de_erro() {
        let a = Anel::novo();
        let mut p = ponte_de(&a);
        a.pedir(0, 1, anel::TIPO_HTTP, 0, b"x");
        let grande = vec![7u8; anel::MAX_RESP + 1];
        p.passo(&|_, _| RespostaPonte { status: 200, corpo: grande.clone() }).unwrap();
        assert_eq!(a.cab(0, anel::SLOT_STATUS), STATUS_RESP_TRUNCADA);
        assert_eq!(a.cab(0, anel::SLOT_LEN_RESP) as usize, anel::MAX_RESP);
    }

    #[test]
    fn panico_no_atendimento_vira_erro_sem_travar_slot() {
        let a = Anel::novo();
        let mut p = ponte_de(&a);
        a.pedir(0, 1, anel::TIPO_HTTP, 0, b"x");
        p.passo(&|_, _| panic!("falha de teste")).unwrap();
        assert_eq!(a.cab(0, anel::SLOT_STATUS), STATUS_ATENDIMENTO_FALHOU);
        assert_eq!(a.cab(0, anel::SLOT_H_RESP), 1);
    }

    #[test]
    fn magic_errado_recusa_passada() {
        let a = Anel::novo();
        let mut p = ponte_de(&a);
        a.escrever_bruto(anel::OFF_MAGIC, b"XXXXXXXX");
        assert!(matches!(p.passo(&eco), Err(ErroPonte::CabecalhoInvalido)));
    }

    #[test]
    fn reiniciar_zera_todos_os_slots() {
        let a = Anel::novo();
        let mut p = ponte_de(&a);
        a.pedir(3, 9, anel::TIPO_HTTP, 0, b"x");
        p.passo(&eco).unwrap();
        p.reiniciar_slots().unwrap();
        for i in 0..anel::N_SLOTS {
            assert_eq!(a.cab(i, anel::SLOT_G_REQ), 0);
            assert_eq!(a.cab(i, anel::SLOT_H_RESP), 0);
            assert_eq!(a.cab(i, anel::SLOT_STATUS), 0);
        }
    }

    #[test]
    fn vigia_atende_trava_pid_e_solta_no_parar() {
        let a = Anel::novo();
        let pid = std::process::id() as i32;
        let vigia = Vigia::iniciar(pid, a.addr(), eco).unwrap();
        assert!(matches!(Vigia::iniciar(pid, a.addr(), eco), Err(ErroPonte::JaAtendido)));

        a.pedir(0, 1, anel::TIPO_HTTP, 0, b"ola");
        let inicio = Instant::now();
        while a.cab(0, anel::SLOT_H_RESP) != 1 {
            assert!(inicio.elapsed() < Duration::from_secs(2), "vigia não respondeu");
            thread::sleep(Duration::from_millis(5));
        }
        assert_eq!(a.bytes(anel::off_resp(0), 3), b"ola");

        vigia.parar().unwrap();
        Vigia::iniciar(pid, a.addr(), eco).expect("lock deveria estar solto após parar").parar().unwrap();
    }

    #[test]
    fn pulso_avanca_durante_atendimento_lento() {
        let a = Anel::novo();
        let mut p = ponte_de(&a);
        a.pedir(0, 1, anel::TIPO_HTTP, 0, b"x");
        let lento = |_: u32, req: &[u8]| {
            thread::sleep(Duration::from_millis(1600));
            eco(0, req)
        };
        let mut valores = Vec::new();
        thread::scope(|s| {
            let trabalho = s.spawn(|| p.passo(&lento));
            while !trabalho.is_finished() {
                valores.push(a.u32_em(anel::OFF_H_PULSO));
                thread::sleep(Duration::from_millis(10));
            }
            trabalho.join().unwrap().unwrap();
        });
        valores.dedup();
        assert!(valores.len() >= 10, "pulso parou durante o atendimento: {valores:?}");
        assert_eq!(a.cab(0, anel::SLOT_H_RESP), 1);
    }

    #[test]
    fn slot_pronto_sem_consumo_e_liberado_apos_prazo() {
        let a = Anel::novo();
        let mut p = ponte_de(&a);
        p.prazo = Duration::from_millis(50);
        a.pedir(0, 1, anel::TIPO_HTTP, 0, b"x");
        p.passo(&eco).unwrap();
        p.passo(&eco).unwrap();
        assert_eq!(a.cab(0, anel::SLOT_G_CONS), 0, "não libera antes do prazo");
        thread::sleep(Duration::from_millis(60));
        p.passo(&eco).unwrap();
        assert_eq!(a.cab(0, anel::SLOT_G_CONS), 1);
        assert!(anel::pode_reusar(1, a.cab(0, anel::SLOT_G_CONS), a.cab(0, anel::SLOT_H_RESP)));
    }

    #[test]
    fn slot_consumido_a_tempo_nao_e_tocado() {
        let a = Anel::novo();
        let mut p = ponte_de(&a);
        p.prazo = Duration::from_millis(50);
        a.pedir(0, 1, anel::TIPO_HTTP, 0, b"x");
        p.passo(&eco).unwrap();
        p.passo(&eco).unwrap();
        a.consumir(0, 1);
        thread::sleep(Duration::from_millis(60));
        p.passo(&eco).unwrap();
        assert!(p.presos[0].is_none());
        assert_eq!(a.cab(0, anel::SLOT_G_CONS), 1);
    }

    #[test]
    fn http_mal_codificado_vira_status_zero_com_texto() {
        let r = atender_padrao(anel::TIPO_HTTP, b"lixo");
        assert_eq!(r.status, 0);
        assert_eq!(r.corpo, "invalid field size".as_bytes());
    }

    #[test]
    fn http_para_ip_privado_e_recusado_sem_rede() {
        let req = crate::ponte_codec::codificar(&[b"GET", b"http://127.0.0.1/", b"0", b""]);
        let r = atender_padrao(anel::TIPO_HTTP, &req);
        assert_eq!(r.status, 0);
        assert!(String::from_utf8_lossy(&r.corpo).contains("destination refused"));
    }

    #[test]
    fn tipo_sem_atendimento_responde_desconhecido() {
        let r = atender_padrao(anel::TIPO_WS, b"");
        assert_eq!(r.status, STATUS_TIPO_DESCONHECIDO);
        assert!(r.corpo.is_empty());
    }
}
