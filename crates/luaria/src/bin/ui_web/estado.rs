//! Estado compartilhado entre o servidor HTTP, o motor e o vigia do log.
//!
//! O `Motor` não é `Sync` de verdade para uso concorrente: por isso ele mora
//! numa thread dedicada e todo mundo fala com ele por canal. O servidor HTTP só
//! lê o retrato em `status` e o anel de console — nunca segura o motor.

use std::collections::{BTreeMap, HashMap, HashSet, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

use serde::Serialize;
use luaria::engine::Motor;
use luaria::ponte::{self, ErroPonte, Ponte, Vigia};

use super::clientes_api::{self, AlvoClientes, Perfil};

/// Ruído que a UI sabe esconder/mostrar com o filtro.
const RUIDO: [&str; 3] = ["[perf]", "Wrap-deformer", "[compat]"];

/// Barulho que não interessa nem com o filtro desligado.
const DESCARTA: [&str; 3] = ["[FLog::Network]", "OpenSSL", "Vulkan"];

/// Quanto do script aparece no cartão da biblioteca.
const TRECHO_LINHAS: usize = 8;
const TRECHO_COLUNAS: usize = 60;

/// Teto do console em memória.
const MAX_LINHAS: usize = 4000;

/// Intervalo entre tentativas automáticas de anexar ao cliente.
const INTERVALO_ANEXO: Duration = Duration::from_secs(2);

#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum TipoLog {
    Script,
    Warn,
    Erro,
    Info,
}

#[derive(Clone, Serialize)]
pub struct LinhaLog {
    pub seq: u64,
    pub tipo: TipoLog,
    pub texto: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub linha: Option<usize>,
}

#[derive(Clone, Serialize)]
pub struct Status {
    pub conectado: bool,
    pub pid: Option<i32>,
    pub luau_load: Option<u64>,
    pub lua_d_call: Option<u64>,
    /// Linhas do chunk que vêm antes do script do usuário. A linha do editor é
    /// `linha_do_chunk - linhas_prelude`.
    pub linhas_prelude: usize,
    pub cliente_vivo: bool,
    pub anexando: bool,
    pub executando: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub erro: Option<String>,
}

impl Default for Status {
    fn default() -> Self {
        Self {
            conectado: false,
            pid: None,
            luau_load: None,
            lua_d_call: None,
            linhas_prelude: 0,
            cliente_vivo: false,
            anexando: false,
            executando: false,
            erro: None,
        }
    }
}

#[derive(Clone, Serialize)]
pub struct ResultadoExec {
    pub ok: bool,
    pub log: Vec<String>,
    pub ms: u128,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub erro: Option<String>,
}

pub enum Comando {
    Executar { codigo: String, alvo: AlvoClientes, resposta: Sender<ResultadoExec> },
    Anexar { resposta: Sender<Status> },
    Status { resposta: Sender<Status> },
    AnexarPid { pid: i32, resposta: Sender<Result<(), String>> },
    Desanexar { pid: i32, resposta: Sender<()> },
    /// (pid, (só viu LuaApp, place)) de cada motor anexado.
    Clientes { resposta: Sender<Vec<(i32, (bool, Option<u64>))>> },
}

struct Console {
    linhas: VecDeque<LinhaLog>,
    proximo: u64,
}

pub struct Estado {
    pub status: Mutex<Status>,
    console: Mutex<Console>,
    motor: OnceLock<Sender<Comando>>,
    /// Trava de execução: dois Ctrl+Enter seguidos não empilham chamadas.
    pub ocupado: AtomicBool,
    pub scripts: PathBuf,
    /// Nome e userid lidos do log, por pid. Escrito pelo vigia do log.
    pub perfis: Mutex<HashMap<i32, Perfil>>,
    pub alvo: Mutex<AlvoClientes>,
}

impl Estado {
    pub fn novo() -> Arc<Self> {
        let estado = Arc::new(Self {
            status: Mutex::new(Status::default()),
            console: Mutex::new(Console {
                linhas: VecDeque::new(),
                // seq começa em 1: `desde=0` (primeira carga) pega tudo.
                proximo: 1,
            }),
            motor: OnceLock::new(),
            ocupado: AtomicBool::new(false),
            scripts: pasta_scripts(),
            perfis: Mutex::new(HashMap::new()),
            alvo: Mutex::new(AlvoClientes::Pids(Vec::new())),
        });
        let _ = std::fs::create_dir_all(&estado.scripts);
        iniciar_tail(estado.clone());
        iniciar_motor(estado.clone());
        estado
    }

    pub fn comando(&self) -> Option<Sender<Comando>> {
        self.motor.get().cloned()
    }

    pub fn registrar(&self, tipo: TipoLog, texto: String, linha: Option<usize>) {
        let mut console = self.console.lock().unwrap();
        let seq = console.proximo;
        console.proximo += 1;
        console.linhas.push_back(LinhaLog { seq, tipo, texto, linha });
        while console.linhas.len() > MAX_LINHAS {
            console.linhas.pop_front();
        }
    }

    pub fn info(&self, texto: impl Into<String>) {
        self.registrar(TipoLog::Info, texto.into(), None);
    }

    pub fn erro(&self, texto: impl Into<String>) {
        self.registrar(TipoLog::Erro, texto.into(), None);
    }

    /// Linhas com `seq > desde`; o front guarda o último `seq` que viu.
    pub fn desde(&self, desde: u64) -> Vec<LinhaLog> {
        let console = self.console.lock().unwrap();
        console
            .linhas
            .iter()
            .filter(|l| l.seq > desde)
            .cloned()
            .collect()
    }

    pub fn status(&self) -> Status {
        let mut s = self.status.lock().unwrap().clone();
        s.cliente_vivo = s.pid.map(cliente_vivo).unwrap_or(false);
        if s.pid.is_some() && !s.cliente_vivo {
            s.conectado = false;
        }
        s
    }
}

pub fn pasta_scripts() -> PathBuf {
    if let Ok(dir) = std::env::var("LUARIA_SCRIPTS") {
        return PathBuf::from(dir);
    }
    let home = std::env::var("HOME").unwrap_or_else(|_| ".".into());
    PathBuf::from(home).join(".local/share/luaria/scripts")
}

pub fn pasta_config() -> PathBuf {
    let home = std::env::var("HOME").unwrap_or_else(|_| ".".into());
    PathBuf::from(home).join(".config/luaria")
}

/// Log que a UI acompanha: `LUARIA_LOG`, ou o mais recente entre o stdout do
/// `tools/launch.sh` e o log interno do cliente. Aberto pelo próprio Mocktail
/// não existe `/tmp/luaria-client.log`, mas o cliente grava o mesmo `[FLog::...]`
/// em `appData/logs/*_last.log`.
fn caminho_log() -> PathBuf {
    if let Ok(p) = std::env::var("LUARIA_LOG") {
        return PathBuf::from(p);
    }
    let mut candidatos = vec![PathBuf::from("/tmp/luaria-client.log")];
    let home = std::env::var("HOME").unwrap_or_default();
    let pasta = PathBuf::from(home)
        .join(".var/app/space.bigrat.mocktail/data/mocktail/android/data/files/appData/logs");
    if let Ok(lista) = std::fs::read_dir(pasta) {
        candidatos.extend(lista.flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|x| x == "log")));
    }
    candidatos
        .into_iter()
        .filter_map(|p| Some((std::fs::metadata(&p).ok()?.modified().ok()?, p)))
        .max_by_key(|(t, _)| *t)
        .map(|(_, p)| p)
        .unwrap_or_else(|| PathBuf::from("/tmp/luaria-client.log"))
}

fn cliente_vivo(pid: i32) -> bool {
    if pid <= 0 {
        return false;
    }
    let maps = PathBuf::from(format!("/proc/{pid}/maps"));
    match std::fs::read_to_string(maps) {
        Ok(txt) => txt.contains("libroblox"),
        Err(_) => false,
    }
}

// ===================== motor numa thread dedicada ==========================

fn iniciar_motor(estado: Arc<Estado>) {
    let (tx, rx) = channel::<Comando>();
    let _ = estado.motor.set(tx);
    std::thread::spawn(move || laco_motor(estado, rx));
}

/// Motores anexados e o que a thread já pediu a cada um. Só esta thread mexe aqui.
#[derive(Default)]
struct Sessao {
    motores: BTreeMap<i32, Motor>,
    /// (pid, place) em que o perfil já foi pedido; place 0 = jogo ainda não visto.
    perfis_pedidos: HashSet<(i32, u64)>,
    /// Pids que o usuário desanexou: o anexo automático não os escolhe de novo.
    recusados: HashSet<i32>,
    /// Um Vigia da ponte por pid. Sem entrada = o host ainda não atende esse cliente.
    vigias: BTreeMap<i32, VigiaAtivo>,
    /// Pids com Vigia parando em segundo plano para zerar os slots. Só o worker remove.
    reiniciando: Arc<Mutex<HashSet<i32>>>,
    /// Pids em que a ponte falhou com erro de I/O: não repete o erro a cada passada.
    falha_ponte: HashSet<i32>,
}

struct VigiaAtivo {
    vigia: Vigia,
    anel: u64,
    /// PlaceId em que o Vigia começou; mudança de place reinicia a ponte.
    place: Option<u64>,
}

impl Sessao {
    /// `None` escolhe um cliente sozinho (anexo automático). Erro vai para `status.erro`.
    fn anexar(&mut self, estado: &Estado, pid: Option<i32>) -> Result<(), String> {
        if pid.is_some_and(|p| self.motores.contains_key(&p)) {
            return Ok(());
        }
        estado.status.lock().unwrap().anexando = true;
        let resultado = match pid {
            Some(p) => Motor::conectar_pid(p),
            None => {
                let recusados: Vec<i32> = self.recusados.iter().copied().collect();
                Motor::conectar_excluindo(&recusados)
            }
        };
        estado.status.lock().unwrap().anexando = false;
        match resultado {
            Ok(motor) => {
                let pid_novo = motor.pid;
                self.recusados.remove(&pid_novo);
                self.motores.insert(pid_novo, motor);
                estado.info(format!("client attached · pid {pid_novo}"));
                self.publicar(estado);
                Ok(())
            }
            Err(e) => {
                let texto = e.to_string();
                estado.status.lock().unwrap().erro = Some(texto.clone());
                Err(texto)
            }
        }
    }

    fn desanexar(&mut self, estado: &Estado, pid: i32) {
        // o Drop do Motor solta o freio do hook
        self.motores.remove(&pid);
        self.perfis_pedidos.retain(|(p, _)| *p != pid);
        self.falha_ponte.remove(&pid);
        self.recusados.insert(pid);
        self.publicar(estado);
    }

    /// Tira os motores cujo cliente fechou. Devolve `true` se saiu algum.
    fn remover_mortos(&mut self, estado: &Estado) -> bool {
        let mortos: Vec<i32> = self.motores.keys().copied().filter(|p| !cliente_vivo(*p)).collect();
        for pid in &mortos {
            self.motores.remove(pid);
            self.perfis_pedidos.retain(|(p, _)| p != pid);
            self.falha_ponte.remove(pid);
            estado.erro(format!("client {pid} exited · waiting for reconnection"));
        }
        if !mortos.is_empty() {
            self.publicar(estado);
        }
        !mortos.is_empty()
    }

    /// Um Vigia por cliente com a ponte instalada. Roda a cada passada: quem não ganhou
    /// a trava (outro processo atende) ou esperava o slot ser zerado é tentado de novo.
    fn atender_ponte(&mut self, estado: &Estado) {
        self.soltar_vigias_trocados(estado);
        for (pid, motor) in &self.motores {
            let reiniciando = self.reiniciando.lock().unwrap().contains(pid);
            if self.vigias.contains_key(pid) || reiniciando || self.falha_ponte.contains(pid) {
                continue;
            }
            let Some(anel) = motor.anel_ponte() else { continue };
            match Vigia::iniciar(*pid, anel, ponte::atender_padrao) {
                Ok(vigia) => {
                    self.vigias.insert(*pid, VigiaAtivo { vigia, anel, place: motor.estado_cliente().1 });
                }
                Err(ErroPonte::JaAtendido) => {}
                Err(e) => {
                    estado.erro(format!("bridge for client {pid} did not start: {e:?}"));
                    self.falha_ponte.insert(*pid);
                }
            }
        }
    }

    /// Solta os Vigias de cliente que sumiu, trocou de PlaceId ou cujo poller já parou.
    fn soltar_vigias_trocados(&mut self, estado: &Estado) {
        let soltar: Vec<i32> = self
            .vigias
            .iter()
            .filter(|(pid, ativo)| {
                let trocou = match self.motores.get(pid) {
                    None => true,
                    Some(motor) => matches!(motor.estado_cliente().1, Some(novo) if Some(novo) != ativo.place),
                };
                trocou || ativo.vigia.terminou()
            })
            .map(|(pid, _)| *pid)
            .collect();
        for pid in soltar {
            let Some(ativo) = self.vigias.remove(&pid) else { continue };
            if ativo.vigia.terminou() {
                if let Err(e) = ativo.vigia.parar() {
                    estado.erro(format!("bridge for client {pid} stopped: {e:?}"));
                }
            } else {
                self.parar_e_zerar_em_segundo_plano(pid, ativo);
            }
        }
    }

    /// `parar` espera o worker terminar um curl de até 30 s, então sai do laço do motor.
    /// Zerar os slots só depois da parada: o Vigia segura o lock enquanto roda.
    fn parar_e_zerar_em_segundo_plano(&self, pid: i32, ativo: VigiaAtivo) {
        self.reiniciando.lock().unwrap().insert(pid);
        let reiniciando = self.reiniciando.clone();
        let VigiaAtivo { vigia, anel, .. } = ativo;
        std::thread::spawn(move || {
            let _ = vigia.parar();
            // Falha aqui é cliente que sumiu entre a parada e a escrita; não há o que fazer.
            let _ = Ponte::nova(pid, anel).reiniciar_slots();
            reiniciando.lock().unwrap().remove(&pid);
        });
    }

    /// Espelha o motor principal (menor pid) no `Status` que a UI lê.
    fn publicar(&self, estado: &Estado) {
        let mut s = estado.status.lock().unwrap();
        if let Some(motor) = self.motores.values().next() {
            s.conectado = true;
            s.pid = Some(motor.pid);
            s.luau_load = motor.anchors.luau_load;
            s.lua_d_call = motor.anchors.lua_d_call;
            s.linhas_prelude = luaria::engine::linha_do_usuario().saturating_sub(1);
            s.cliente_vivo = true;
            s.erro = None;
        } else {
            s.conectado = false;
            s.pid = None;
            s.luau_load = None;
            s.lua_d_call = None;
            s.cliente_vivo = false;
        }
    }

    /// Pede o perfil uma vez por (pid, place). Falha não é repetida: senão vira
    /// loop no console a cada 300 ms.
    fn pedir_perfis(&mut self, estado: &Estado) {
        for (pid, motor) in self.motores.iter_mut() {
            let place = motor.estado_cliente().1.unwrap_or(0);
            if !self.perfis_pedidos.insert((*pid, place)) {
                continue;
            }
            let mut linhas = Vec::new();
            if let Err(e) = motor.executar_com_log(&script_perfil(*pid), &mut linhas) {
                estado.registrar(TipoLog::Warn, format!("profile of client {pid} not obtained: {e}"), None);
            }
        }
    }
}

/// `print` vai para o log do cliente, que `processar` lê como `LUARIA_PLAYER`.
/// O pid é inteiro vindo do próprio host, então entra sem escape.
fn script_perfil(pid: i32) -> String {
    format!(
        "local jogador = game:GetService(\"Players\").LocalPlayer\n\
         print(\"LUARIA_PLAYER\", {pid}, jogador.UserId, jogador.Name, jogador.DisplayName)"
    )
}

/// Pids que recebem a execução. `Pids` sem nenhum anexado cai no primeiro
/// anexado, como o alvo padrão vazio sempre fez.
fn pids_alvo(alvo: &AlvoClientes, anexados: &[i32]) -> Vec<i32> {
    match alvo {
        AlvoClientes::Todos => anexados.to_vec(),
        AlvoClientes::Pids(pedidos) => {
            let escolhidos: Vec<i32> = anexados.iter().copied().filter(|p| pedidos.contains(p)).collect();
            if escolhidos.is_empty() {
                anexados.iter().copied().take(1).collect()
            } else {
                escolhidos
            }
        }
    }
}

/// Roda `codigo` em cada alvo, em sequência. Uma falha não para os demais.
fn executar_nos_alvos(
    sessao: &mut Sessao,
    alvo: &AlvoClientes,
    codigo: &str,
    linhas: &mut Vec<String>,
) -> Result<(), String> {
    let anexados: Vec<i32> = sessao.motores.keys().copied().collect();
    let pids = pids_alvo(alvo, &anexados);
    if pids.is_empty() {
        return Err(luaria::engine::ErroMotor::SemCliente.to_string());
    }
    let nao_anexados: Vec<i32> = match alvo {
        AlvoClientes::Todos => vec![],
        AlvoClientes::Pids(pedidos) => pedidos.iter().copied().filter(|p| !anexados.contains(p)).collect(),
    };
    let multi = pids.len() + nao_anexados.len() > 1;
    let mut erros: Vec<String> = nao_anexados.iter().map(|p| format!("pid {p}: not attached")).collect();
    for pid in pids {
        let Some(motor) = sessao.motores.get_mut(&pid) else { continue };
        let mut do_pid = Vec::new();
        let resultado = motor.executar_com_log(codigo, &mut do_pid);
        let prefixo = if multi { format!("[pid {pid}] ") } else { String::new() };
        linhas.extend(do_pid.into_iter().map(|l| format!("{prefixo}{l}")));
        if let Err(e) = resultado {
            erros.push(if multi { format!("pid {pid}: {e}") } else { e.to_string() });
        }
    }
    if erros.is_empty() {
        Ok(())
    } else {
        Err(erros.join("; "))
    }
}

fn laco_motor(estado: Arc<Estado>, rx: Receiver<Comando>) {
    let mut sessao = Sessao::default();
    let mut ultima_tentativa: Option<Instant> = None;

    loop {
        if sessao.motores.is_empty() && ultima_tentativa.map_or(true, |t| t.elapsed() >= INTERVALO_ANEXO) {
            ultima_tentativa = Some(Instant::now());
            let _ = sessao.anexar(&estado, None);
        }
        if sessao.remover_mortos(&estado) {
            ultima_tentativa = None;
        }
        sessao.atender_ponte(&estado);
        sessao.pedir_perfis(&estado);

        match rx.recv_timeout(Duration::from_millis(300)) {
            Ok(Comando::Status { resposta }) => {
                let _ = resposta.send(estado.status());
            }
            Ok(Comando::Anexar { resposta }) => {
                if sessao.motores.is_empty() {
                    ultima_tentativa = Some(Instant::now());
                    let _ = sessao.anexar(&estado, None);
                }
                let _ = resposta.send(estado.status());
            }
            Ok(Comando::AnexarPid { pid, resposta }) => {
                let _ = resposta.send(sessao.anexar(&estado, Some(pid)));
            }
            Ok(Comando::Desanexar { pid, resposta }) => {
                sessao.desanexar(&estado, pid);
                let _ = resposta.send(());
            }
            Ok(Comando::Clientes { resposta }) => {
                let anexos = sessao.motores.iter().map(|(p, m)| (*p, m.estado_cliente())).collect();
                let _ = resposta.send(anexos);
            }
            Ok(Comando::Executar { codigo, alvo, resposta }) => {
                if estado.ocupado.swap(true, Ordering::SeqCst) {
                    let _ = resposta.send(ResultadoExec {
                        ok: false,
                        log: vec![],
                        ms: 0,
                        erro: Some("an execution is already running".into()),
                    });
                    continue;
                }
                estado.status.lock().unwrap().executando = true;
                let inicio = Instant::now();
                let mut linhas = Vec::new();
                let resultado = executar_nos_alvos(&mut sessao, &alvo, &codigo, &mut linhas);
                let ms = inicio.elapsed().as_millis();
                for l in &linhas {
                    estado.info(l.clone());
                }
                let (ok, erro) = match resultado {
                    Ok(()) => {
                        estado.info(format!("execution finished in {ms} ms"));
                        (true, None)
                    }
                    Err(texto) => {
                        estado.erro(texto.clone());
                        (false, Some(texto))
                    }
                };
                estado.status.lock().unwrap().executando = false;
                estado.ocupado.store(false, Ordering::SeqCst);
                let _ = resposta.send(ResultadoExec { ok, log: linhas, ms, erro });
            }
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => break,
        }
    }
}

// ============================ log do cliente ===============================

fn iniciar_tail(estado: Arc<Estado>) {
    std::thread::spawn(move || {
        let mut caminho = caminho_log();
        let mut deslocamento = std::fs::metadata(&caminho).map(|m| m.len()).unwrap_or(0);
        let mut parcial = String::new();
        let mut voltas = 0u32;

        loop {
            std::thread::sleep(Duration::from_millis(250));
            voltas += 1;
            // cliente reaberto cria log novo: troca e lê desde o começo. Só larga o
            // atual parado há 30 s, senão dois logs vivos alternariam e o mesmo
            // LUARIA_FS (appendfile) seria aplicado duas vezes.
            if voltas % 8 == 0 {
                let parado = std::fs::metadata(&caminho)
                    .and_then(|m| m.modified())
                    .ok()
                    .and_then(|t| t.elapsed().ok())
                    .is_none_or(|idade| idade > Duration::from_secs(30));
                let novo = if parado { caminho_log() } else { caminho.clone() };
                if novo != caminho {
                    caminho = novo;
                    deslocamento = 0;
                    parcial.clear();
                }
            }
            let Ok(meta) = std::fs::metadata(&caminho) else { continue };
            if meta.len() < deslocamento {
                deslocamento = 0;
                parcial.clear();
            }
            if meta.len() == deslocamento {
                continue;
            }
            let Ok(mut arquivo) = std::fs::File::open(&caminho) else { continue };
            use std::io::{Read, Seek, SeekFrom};
            if arquivo.seek(SeekFrom::Start(deslocamento)).is_err() {
                continue;
            }
            let mut bytes = Vec::new();
            if arquivo.read_to_end(&mut bytes).is_err() {
                continue;
            }
            deslocamento += bytes.len() as u64;
            parcial.push_str(&String::from_utf8_lossy(&bytes));
            while let Some(pos) = parcial.find('\n') {
                let linha: String = parcial.drain(..=pos).collect();
                processar(&estado, linha.trim_end_matches(['\r', '\n']));
            }
        }
    });
}

fn processar(estado: &Arc<Estado>, bruta: &str) {
    // linha da ponte de arquivos não é saída do script: aplica e some do console
    if let Some(resultado) = luaria::arquivos::aplicar(bruta) {
        if let Err(e) = resultado {
            estado.registrar(TipoLog::Erro, format!("arquivos: {e}"), None);
        }
        return;
    }
    if let Some(resultado) = luaria::arquivos::texto_clipboard(bruta, luaria::arquivos::token()) {
        match resultado.and_then(|texto| copiar(&texto)) {
            Ok(()) => estado.registrar(TipoLog::Info, "setclipboard: texto copiado".into(), None),
            Err(e) => estado.registrar(TipoLog::Erro, format!("setclipboard: {e}"), None),
        }
        return;
    }
    // linha de perfil é do host, não saída do script: guarda e some do console
    if let Some((pid, perfil)) = clientes_api::ler_perfil(bruta) {
        estado.perfis.lock().unwrap().insert(pid, perfil);
        return;
    }
    let prelude = estado.status.lock().unwrap().linhas_prelude;
    if let Some((tipo, texto, linha)) = classificar(bruta, prelude) {
        estado.registrar(tipo, texto, linha);
    }
}

/// Clipboard do sistema pelo utilitário da sessão (Wayland ou X11).
fn copiar(texto: &str) -> Result<(), String> {
    use std::io::Write;
    use std::process::{Command, Stdio};
    let candidatos: [(&str, &[&str]); 2] = [("wl-copy", &[]), ("xclip", &["-selection", "clipboard"])];
    for (programa, args) in candidatos {
        let Ok(mut filho) = Command::new(programa).args(args).stdin(Stdio::piped()).spawn() else { continue };
        if let Some(mut entrada) = filho.stdin.take() {
            entrada.write_all(texto.as_bytes()).map_err(|e| format!("{programa}: {e}"))?;
        }
        // wl-copy fica em segundo plano servindo o clipboard; não espera
        std::thread::spawn(move || filho.wait());
        return Ok(());
    }
    Err("neither wl-copy nor xclip installed".into())
}

/// Traduz uma linha do log do cliente para o console. `None` = ruído.
pub fn classificar(linha: &str, prelude: usize) -> Option<(TipoLog, String, Option<usize>)> {
    let linha = linha.trim();
    if linha.is_empty() || DESCARTA.iter().any(|r| linha.contains(r)) {
        return None;
    }
    // Ruído vira `info`: o filtro é decisão da UI, que pode desligá-lo.
    if RUIDO.iter().any(|r| linha.contains(r)) {
        return Some((TipoLog::Info, linha.to_string(), None));
    }

    if let Some(pos) = linha.find("LUARIA_ERRO") {
        let resto = linha[pos + "LUARIA_ERRO".len()..].trim();
        let (numero, texto) = separar_erro(resto, prelude);
        return Some((TipoLog::Erro, texto, numero));
    }

    if let Some(texto) = apos(linha, "[FLog::CreatorOutput]") {
        return Some((TipoLog::Script, texto.to_string(), None));
    }

    if let Some(texto) = apos(linha, "[FLog::Warning]") {
        let (numero, texto) = separar_erro(texto, prelude);
        return Some((TipoLog::Warn, texto, numero));
    }
    if let Some(texto) = apos(linha, "[FLog::Error]") {
        let (numero, texto) = separar_erro(texto, prelude);
        return Some((TipoLog::Erro, texto, numero));
    }

    if let Some(texto) = apos(linha, "[FLog::Output]") {
        let baixo = texto.to_ascii_lowercase();
        if baixo.contains("warning") || baixo.contains("warn:") {
            let (numero, limpo) = separar_erro(texto, prelude);
            return Some((TipoLog::Warn, limpo, numero));
        }
        return None;
    }

    if linha.starts_with("[luaria") || linha.starts_with("luaria:") {
        return Some((TipoLog::Info, linha.to_string(), None));
    }

    None
}

fn apos<'a>(linha: &'a str, tag: &str) -> Option<&'a str> {
    linha.find(tag).map(|p| linha[p + tag.len()..].trim())
}

/// Separa `luaria:12: mensagem` em (linha do editor, mensagem).
fn separar_erro(texto: &str, prelude: usize) -> (Option<usize>, String) {
    if let Some((fonte, resto)) = texto.split_once(':') {
        let fonte = fonte.trim();
        if fonte.starts_with("luaria") || fonte.starts_with("chunk") || fonte.starts_with("script") {
            if let Some((numero, mensagem)) = resto.split_once(':') {
                if let Ok(n) = numero.trim().parse::<usize>() {
                    // Sem conexão não há prelude conhecido: melhor sem gutter do
                    // que apontar para a linha errada.
                    let linha = if prelude == 0 {
                        None
                    } else {
                        n.checked_sub(prelude).filter(|l| *l > 0)
                    };
                    return (linha, mensagem.trim().to_string());
                }
            }
        }
    }
    (None, texto.trim().to_string())
}

// ================================ scripts =================================

#[derive(Serialize)]
pub struct ScriptInfo {
    pub nome: String,
    pub tamanho: u64,
    pub modificado: u64,
    pub linhas: usize,
    /// Primeira linha "de verdade" do script, para o cartão da biblioteca.
    pub previa: String,
    /// Primeiras linhas do código, já com corte, para a prévia do cartão.
    pub trecho: String,
}

pub fn nome_seguro(nome: &str) -> Option<String> {
    let base = Path::new(nome).file_name()?.to_string_lossy().to_string();
    if base.is_empty() || base.starts_with('.') {
        return None;
    }
    if !base.ends_with(".lua") && !base.ends_with(".luau") {
        return Some(format!("{base}.lua"));
    }
    Some(base)
}

pub fn listar_scripts(dir: &Path) -> Vec<ScriptInfo> {
    let Ok(entradas) = std::fs::read_dir(dir) else {
        return vec![];
    };
    let mut lista: Vec<ScriptInfo> = entradas
        .flatten()
        .filter_map(|e| {
            let caminho = e.path();
            if !caminho.is_file() {
                return None;
            }
            let nome = caminho.file_name()?.to_string_lossy().to_string();
            if !nome.ends_with(".lua") && !nome.ends_with(".luau") {
                return None;
            }
            let meta = e.metadata().ok()?;
            let conteudo = std::fs::read_to_string(&caminho).unwrap_or_default();
            let previa = conteudo
                .lines()
                .map(str::trim)
                .find(|l| !l.is_empty() && !l.starts_with("--"))
                .unwrap_or("")
                .chars()
                .take(80)
                .collect();
            let trecho = conteudo
                .lines()
                .take(TRECHO_LINHAS)
                .map(|l| l.chars().take(TRECHO_COLUNAS).collect::<String>())
                .collect::<Vec<_>>()
                .join("\n");
            Some(ScriptInfo {
                nome,
                tamanho: meta.len(),
                modificado: meta
                    .modified()
                    .ok()
                    .and_then(|m| m.duration_since(std::time::UNIX_EPOCH).ok())
                    .map(|d| d.as_secs())
                    .unwrap_or(0),
                linhas: conteudo.lines().count(),
                previa,
                trecho,
            })
        })
        .collect();
    lista.sort_by(|a, b| a.nome.to_lowercase().cmp(&b.nome.to_lowercase()));
    lista
}
#[cfg(test)]
mod testes {
    use super::*;

    #[test]
    fn erro_do_script_volta_para_a_linha_do_editor() {
        let prelude = 40;
        let (_, texto, linha) = classificar(
            "[FLog::CreatorOutput] LUARIA_ERRO luaria:52: attempt to index nil with 'x'",
            prelude,
        )
        .unwrap();
        assert_eq!(linha, Some(12));
        assert_eq!(texto, "attempt to index nil with 'x'");
    }

    #[test]
    fn ruido_vira_info_filtravel() {
        for linha in ["[perf] frame 12ms", "Wrap-deformer lento", "[compat] aviso"] {
            let (tipo, _, _) = classificar(linha, 10).expect("ruído deve chegar como info");
            assert_eq!(tipo, TipoLog::Info);
        }
        assert!(classificar("[FLog::Network] sending", 10).is_none());
    }

    #[test]
    fn warning_sem_linha_nao_inventa_gutter() {
        let (_, texto, linha) = classificar(
            "[FLog::Output] Warning: algo estranho no jogo",
            10,
        )
        .unwrap();
        assert_eq!(linha, None);
        assert!(texto.contains("Warning"));
    }

    #[test]
    fn alvo_todos_vai_para_todos_os_anexados() {
        assert_eq!(pids_alvo(&AlvoClientes::Todos, &[7, 3]), vec![7, 3]);
    }

    #[test]
    fn alvo_pids_mantem_ordem_dos_anexados() {
        let alvo = AlvoClientes::Pids(vec![3, 9]);
        assert_eq!(pids_alvo(&alvo, &[7, 3, 5]), vec![3]);
        let alvo = AlvoClientes::Pids(vec![5, 7]);
        assert_eq!(pids_alvo(&alvo, &[7, 3, 5]), vec![7, 5]);
    }

    #[test]
    fn alvo_pids_vazio_cai_no_primeiro_anexado() {
        assert_eq!(pids_alvo(&AlvoClientes::Pids(vec![]), &[7, 3]), vec![7]);
        assert_eq!(pids_alvo(&AlvoClientes::Pids(vec![99]), &[7, 3]), vec![7]);
    }

    #[test]
    fn alvo_pid_desconhecido_e_ignorado_na_interseccao() {
        let alvo = AlvoClientes::Pids(vec![99, 3]);
        assert_eq!(pids_alvo(&alvo, &[7, 3]), vec![3]);
    }

    #[test]
    fn sem_anexados_nao_ha_alvo() {
        assert!(pids_alvo(&AlvoClientes::Todos, &[]).is_empty());
        assert!(pids_alvo(&AlvoClientes::Pids(vec![3]), &[]).is_empty());
    }
}
