//! Servidor HTTP mínimo em `127.0.0.1:0` — só o necessário para o WebView.
//!
//! Sem framework: um accept, uma thread por conexão e respostas curtas. Os
//! estáticos da UI entram no binário por `include_bytes!`, então o app roda de
//! qualquer diretório.

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::mpsc::{channel, Sender};
use std::sync::Arc;
use std::time::Duration;

use serde_json::{json, Value};

use super::clientes_api::{self, AlvoClientes};
use super::config;
use super::pastas;
use super::estado::{self, Comando, Estado};

/// Pedidos de janela vindos da UI; quem executa é a thread GTK (`ui_web.rs`).
pub enum AcaoJanela {
    Minimizar,
    Maximizar,
    Fechar,
    Arrastar { x: f64, y: f64, botao: i32 },
    /// Opacidade nativa da janela (0.2 a 1.0); o clampe fica na thread GTK.
    Opacidade { valor: f64 },
    /// `borda` é a sigla da alça (n, s, l, o, no, ne, so, se); a thread GTK valida.
    Redimensionar { borda: String, x: f64, y: f64, botao: i32 },
}

/// Corpo máximo aceito: o fundo é o maior envio da UI.
const LIMITE_CORPO: usize = config::LIMITE_FUNDO + (1 << 20);

/// Rota, tipo MIME, caminho relativo a `assets/ui/` e bytes embutidos. Com
/// `LUARIA_DEV` o arquivo é lido do disco, assim o CSS muda sem recompilar.
const ESTATICOS: &[(&str, &str, &str, &[u8])] = &[
    (
        "/index.html",
        "text/html; charset=utf-8",
        "index.html",
        include_bytes!("../../../../../assets/ui/index.html"),
    ),
    (
        "/app.js",
        "application/javascript; charset=utf-8",
        "app.js",
        include_bytes!("../../../../../assets/ui/app.js"),
    ),
    (
        "/css/base.css",
        "text/css; charset=utf-8",
        "css/base.css",
        include_bytes!("../../../../../assets/ui/css/base.css"),
    ),
    (
        "/css/chrome.css",
        "text/css; charset=utf-8",
        "css/chrome.css",
        include_bytes!("../../../../../assets/ui/css/chrome.css"),
    ),
    (
        "/css/home.css",
        "text/css; charset=utf-8",
        "css/home.css",
        include_bytes!("../../../../../assets/ui/css/home.css"),
    ),
    (
        "/css/editor.css",
        "text/css; charset=utf-8",
        "css/editor.css",
        include_bytes!("../../../../../assets/ui/css/editor.css"),
    ),
    (
        "/css/scripts.css",
        "text/css; charset=utf-8",
        "css/scripts.css",
        include_bytes!("../../../../../assets/ui/css/scripts.css"),
    ),
    (
        "/css/settings.css",
        "text/css; charset=utf-8",
        "css/settings.css",
        include_bytes!("../../../../../assets/ui/css/settings.css"),
    ),
    (
        "/css/modais.css",
        "text/css; charset=utf-8",
        "css/modais.css",
        include_bytes!("../../../../../assets/ui/css/modais.css"),
    ),
    (
        "/css/skin-aero.css",
        "text/css; charset=utf-8",
        "css/skin-aero.css",
        include_bytes!("../../../../../assets/ui/css/skin-aero.css"),
    ),
    (
        "/fundo-padrao.svg",
        "image/svg+xml",
        "fundo-padrao.svg",
        include_bytes!("../../../../../assets/ui/fundo-padrao.svg"),
    ),
    (
        "/logo.svg",
        "image/svg+xml",
        "../luaria.svg",
        include_bytes!("../../../../../assets/luaria.svg"),
    ),
    (
        "/fontes/ibm-plex-sans-400.woff2",
        "font/woff2",
        "fontes/ibm-plex-sans-400.woff2",
        include_bytes!("../../../../../assets/ui/fontes/ibm-plex-sans-400.woff2"),
    ),
    (
        "/fontes/ibm-plex-sans-600.woff2",
        "font/woff2",
        "fontes/ibm-plex-sans-600.woff2",
        include_bytes!("../../../../../assets/ui/fontes/ibm-plex-sans-600.woff2"),
    ),
    (
        "/fontes/space-grotesk-600.woff2",
        "font/woff2",
        "fontes/space-grotesk-600.woff2",
        include_bytes!("../../../../../assets/ui/fontes/space-grotesk-600.woff2"),
    ),
    (
        "/fontes/ibm-plex-mono-400.woff2",
        "font/woff2",
        "fontes/ibm-plex-mono-400.woff2",
        include_bytes!("../../../../../assets/ui/fontes/ibm-plex-mono-400.woff2"),
    ),
];

/// Bytes e tipo de um estático; `None` se a rota não existe.
fn estatico(rota: &str) -> Option<(&'static str, Vec<u8>)> {
    let rota = if rota == "/" { "/index.html" } else { rota };
    let (_, tipo, disco, embutido) = ESTATICOS.iter().find(|(r, ..)| *r == rota)?;
    if std::env::var("LUARIA_DEV").is_ok() {
        if let Ok(bytes) = std::fs::read(std::path::Path::new("assets/ui").join(disco)) {
            return Some((*tipo, bytes));
        }
    }
    Some((*tipo, embutido.to_vec()))
}

pub fn iniciar(estado: Arc<Estado>, janela: Sender<AcaoJanela>) -> std::io::Result<u16> {
    let listener = TcpListener::bind("127.0.0.1:0")?;
    let porta = listener.local_addr()?.port();
    std::thread::spawn(move || {
        for conexao in listener.incoming().flatten() {
            let estado = estado.clone();
            let janela = janela.clone();
            std::thread::spawn(move || atender(conexao, estado, janela, porta));
        }
    });
    Ok(porta)
}

struct Pedido {
    metodo: String,
    rota: String,
    query: Vec<(String, String)>,
    tipo: String,
    host: String,
    origem: Option<String>,
    corpo: Vec<u8>,
}

/// Qualquer página aberta no navegador alcança 127.0.0.1. Sem isto um POST
/// "simples" (text/plain, sem preflight) de outro site rodava `/api/exec` no
/// cliente, e um domínio com DNS apontando para 127.0.0.1 lia tudo.
fn pedido_local(pedido: &Pedido, porta: u16) -> bool {
    let hosts = [format!("127.0.0.1:{porta}"), format!("localhost:{porta}")];
    if !hosts.iter().any(|h| pedido.host.eq_ignore_ascii_case(h)) {
        return false;
    }
    match &pedido.origem {
        // curl/CLI locais não mandam Origin; o WebView manda a própria origem
        None => true,
        Some(o) => hosts.iter().any(|h| o.eq_ignore_ascii_case(&format!("http://{h}"))),
    }
}

fn atender(mut stream: TcpStream, estado: Arc<Estado>, janela: Sender<AcaoJanela>, porta: u16) {
    let Some(pedido) = ler_pedido(&mut stream) else { return };
    if !pedido_local(&pedido, porta) {
        let _ = stream.write_all(&json_de(403, &json!({"ok": false, "erro": "origin not allowed"})));
        return;
    }
    let resposta = despachar(&pedido, &estado, &janela);
    let _ = stream.write_all(&resposta);
    let _ = stream.flush();
}

fn despachar(pedido: &Pedido, estado: &Arc<Estado>, janela: &Sender<AcaoJanela>) -> Vec<u8> {
    let rota = pedido.rota.as_str();
    match (pedido.metodo.as_str(), rota) {
        ("OPTIONS", _) => return montar("200 OK", "text/plain", b""),
        ("GET", _) if !rota.starts_with("/api/") => {
            return match estatico(rota) {
                Some((tipo, bytes)) => montar("200 OK", tipo, &bytes),
                None => json_de(404, &json!({"ok": false, "erro": "route not found"})),
            };
        }
        _ => {}
    }

    match (pedido.metodo.as_str(), rota) {
        ("GET", "/api/status") => status(estado),
        ("GET", "/api/log") => {
            let desde = parametro(&pedido.query, "desde")
                .and_then(|v| v.parse::<u64>().ok())
                .unwrap_or(0);
            json_de(200, &estado.desde(desde))
        }
        ("GET", "/api/scripts") => json_de(200, &estado::listar_scripts(&estado.scripts)),
        ("GET", "/api/script") => ler_script(estado, pedido),
        ("GET", "/api/fs/arvore") => pastas::rota_arvore(),
        ("GET", "/api/fs/ler") => pastas::rota_ler(&pedido.query),
        ("POST", r) if r.starts_with("/api/fs/") => pastas::rota_operar(&r["/api/fs/".len()..], &pedido.corpo),
        ("GET", "/api/autoexec") => pastas::rota_autoexec_ler(),
        ("POST", "/api/autoexec") => pastas::rota_autoexec_gravar(&pedido.corpo),
        ("POST", "/api/console") => escrever_console(estado, pedido),
        ("GET", "/api/tema") => json_de(200, &config::ler_tema()),
        ("GET", "/api/atividade") => json_de(200, &config::ler_atividade()),
        ("GET", "/api/changelog") => json_de(200, &config::ler_changelog(&caminho_changelog())),
        ("GET", "/api/fundo") => match config::ler_fundo() {
            Some((tipo, bytes)) => montar("200 OK", tipo, &bytes),
            None => json_de(404, &json!({"ok": false, "erro": "no saved background"})),
        },
        ("POST", "/api/fundo") => enviar_fundo(pedido),
        ("POST", "/api/fundo/remover") => {
            config::remover_fundo();
            json_de(200, &json!({"ok": true}))
        }
        ("POST", "/api/exec") => executar(estado, pedido),
        ("POST", "/api/script") => salvar_script(estado, pedido),
        ("POST", "/api/novo") => novo_script(estado, pedido),
        ("POST", "/api/renomear") => renomear_script(estado, pedido),
        ("POST", "/api/excluir") => excluir_script(estado, pedido),
        ("POST", "/api/anexar") => anexar(estado),
        ("GET", "/api/clientes") => clientes_api::listar(estado),
        ("POST", "/api/clientes/anexar") => clientes_api::anexar(estado, &pedido.corpo),
        ("POST", "/api/clientes/desanexar") => clientes_api::desanexar(estado, &pedido.corpo),
        ("POST", "/api/clientes/alvo") => clientes_api::definir_alvo(estado, &pedido.corpo),
        ("GET", "/api/headshot") => clientes_api::headshot(&pedido.query),
        ("POST", "/api/tema") => salvar_tema(pedido),
        ("POST", "/api/janela") => acao_janela(pedido, janela),
        _ => json_de(404, &json!({"ok": false, "erro": "route not found"})),
    }
}

/// `CHANGELOG.md` da Home: `LUARIA_CHANGELOG`, ou o do diretório de trabalho
/// (rodando a partir do repositório). Sem arquivo, a Home mostra o estado vazio.
fn caminho_changelog() -> std::path::PathBuf {
    match std::env::var("LUARIA_CHANGELOG") {
        Ok(caminho) => std::path::PathBuf::from(caminho),
        Err(_) => std::path::PathBuf::from("CHANGELOG.md"),
    }
}

fn status(estado: &Arc<Estado>) -> Vec<u8> {
    let s = match estado.comando() {
        Some(tx) => {
            let (rtx, rrx) = channel();
            if tx.send(Comando::Status { resposta: rtx }).is_ok() {
                rrx.recv_timeout(Duration::from_secs(2))
                    .unwrap_or_else(|_| estado.status())
            } else {
                estado.status()
            }
        }
        None => estado.status(),
    };
    let mut valor = serde_json::to_value(&s).unwrap_or_else(|_| json!({}));
    valor["versao"] = json!(env!("CARGO_PKG_VERSION"));
    json_de(200, &valor)
}

fn anexar(estado: &Arc<Estado>) -> Vec<u8> {
    let s = match estado.comando() {
        Some(tx) => {
            let (rtx, rrx) = channel();
            if tx.send(Comando::Anexar { resposta: rtx }).is_ok() {
                rrx.recv_timeout(Duration::from_secs(20))
                    .unwrap_or_else(|_| estado.status())
            } else {
                estado.status()
            }
        }
        None => estado.status(),
    };
    json_de(200, &s)
}

/// Deixa a UI escrever no console (AutoExec avisa qual script está rodando).
fn escrever_console(estado: &Arc<Estado>, pedido: &Pedido) -> Vec<u8> {
    let dados = serde_json::from_slice::<Value>(&pedido.corpo).unwrap_or(Value::Null);
    let Some(texto) = dados["texto"].as_str() else {
        return json_de(400, &json!({"ok": false, "erro": "campo 'texto' ausente"}));
    };
    let texto: String = texto.chars().take(500).collect();
    if dados["tipo"] == "erro" {
        estado.erro(texto);
    } else {
        estado.info(texto);
    }
    json_de(200, &json!({"ok": true}))
}

fn executar(estado: &Arc<Estado>, pedido: &Pedido) -> Vec<u8> {
    let Ok(corpo) = serde_json::from_slice::<Value>(&pedido.corpo) else {
        return json_de(400, &json!({"ok": false, "erro": "invalid JSON"}));
    };
    let Some(codigo) = corpo.get("codigo").and_then(|v| v.as_str()) else {
        return json_de(400, &json!({"ok": false, "erro": "campo 'codigo' ausente"}));
    };
    let dia = corpo.get("dia").and_then(|v| v.as_str()).unwrap_or("");
    let Some(tx) = estado.comando() else {
        return json_de(503, &json!({"ok": false, "erro": "engine not started"}));
    };
    let alvo = estado.alvo.lock().unwrap().clone();
    // um pid por alvo: cada execução pode levar até 180 s
    let alvos = match &alvo {
        AlvoClientes::Todos => 4,
        AlvoClientes::Pids(pids) => pids.len(),
    };
    let (rtx, rrx) = channel();
    if tx
        .send(Comando::Executar {
            codigo: codigo.to_string(),
            alvo,
            resposta: rtx,
        })
        .is_err()
    {
        return json_de(503, &json!({"ok": false, "erro": "engine thread died"}));
    }
    match rrx.recv_timeout(Duration::from_secs(180 * alvos.max(1) as u64)) {
        Ok(r) => {
            // Só conta o que deu certo: uma execução recusada não entra no mapa.
            if r.ok {
                let _ = config::registrar_execucao(dia);
            }
            json_de(200, &r)
        }
        Err(_) => json_de(
            504,
            &json!({"ok": false, "erro": "engine did not respond in 180 s"}),
        ),
    }
}

fn enviar_fundo(pedido: &Pedido) -> Vec<u8> {
    match config::salvar_fundo(&pedido.corpo, &pedido.tipo) {
        Ok(()) => json_de(200, &json!({"ok": true})),
        Err(e) => json_de(400, &json!({"ok": false, "erro": e})),
    }
}

fn acao_janela(pedido: &Pedido, janela: &Sender<AcaoJanela>) -> Vec<u8> {
    let Ok(corpo) = serde_json::from_slice::<Value>(&pedido.corpo) else {
        return json_de(400, &json!({"ok": false, "erro": "invalid JSON"}));
    };
    let acao = match corpo.get("acao").and_then(|v| v.as_str()) {
        Some("minimizar") => AcaoJanela::Minimizar,
        Some("maximizar") => AcaoJanela::Maximizar,
        Some("fechar") => AcaoJanela::Fechar,
        Some("opacidade") => AcaoJanela::Opacidade {
            valor: corpo.get("valor").and_then(|v| v.as_f64()).unwrap_or(1.0),
        },
        Some("arrastar") => AcaoJanela::Arrastar {
            x: corpo.get("x").and_then(|v| v.as_f64()).unwrap_or(0.0),
            y: corpo.get("y").and_then(|v| v.as_f64()).unwrap_or(0.0),
            botao: corpo.get("botao").and_then(|v| v.as_i64()).unwrap_or(1) as i32,
        },
        Some("redimensionar") => AcaoJanela::Redimensionar {
            borda: corpo.get("borda").and_then(|v| v.as_str()).unwrap_or("").to_string(),
            x: corpo.get("x").and_then(|v| v.as_f64()).unwrap_or(0.0),
            y: corpo.get("y").and_then(|v| v.as_f64()).unwrap_or(0.0),
            botao: corpo.get("botao").and_then(|v| v.as_i64()).unwrap_or(1) as i32,
        },
        _ => return json_de(400, &json!({"ok": false, "erro": "invalid window action"})),
    };
    let _ = janela.send(acao);
    json_de(200, &json!({"ok": true}))
}

fn ler_script(estado: &Arc<Estado>, pedido: &Pedido) -> Vec<u8> {
    let Some(nome) = parametro(&pedido.query, "nome").and_then(|n| estado::nome_seguro(&n)) else {
        return json_de(400, &json!({"ok": false, "erro": "invalid name"}));
    };
    let caminho = estado.scripts.join(&nome);
    match std::fs::read_to_string(&caminho) {
        Ok(conteudo) => json_de(200, &json!({"nome": nome, "conteudo": conteudo})),
        Err(e) => json_de(404, &json!({"ok": false, "erro": format!("{e}")})),
    }
}

fn salvar_script(estado: &Arc<Estado>, pedido: &Pedido) -> Vec<u8> {
    let Ok(corpo) = serde_json::from_slice::<Value>(&pedido.corpo) else {
        return json_de(400, &json!({"ok": false, "erro": "invalid JSON"}));
    };
    let Some(nome) = corpo.get("nome").and_then(|v| v.as_str()).and_then(estado::nome_seguro) else {
        return json_de(400, &json!({"ok": false, "erro": "invalid name"}));
    };
    let conteudo = corpo.get("conteudo").and_then(|v| v.as_str()).unwrap_or("");
    let caminho = estado.scripts.join(&nome);
    if let Err(e) = std::fs::write(&caminho, conteudo) {
        return json_de(500, &json!({"ok": false, "erro": format!("{e}")}));
    }
    json_de(200, &json!({"ok": true, "nome": nome}))
}

fn novo_script(estado: &Arc<Estado>, pedido: &Pedido) -> Vec<u8> {
    let Ok(corpo) = serde_json::from_slice::<Value>(&pedido.corpo) else {
        return json_de(400, &json!({"ok": false, "erro": "invalid JSON"}));
    };
    let Some(base) = corpo.get("nome").and_then(|v| v.as_str()).and_then(estado::nome_seguro) else {
        return json_de(400, &json!({"ok": false, "erro": "invalid name"}));
    };
    let conteudo = corpo
        .get("conteudo")
        .and_then(|v| v.as_str())
        .unwrap_or("-- new script\n-- Ctrl+Enter runs, Ctrl+S saves\n\n");
    let (tronco, extensao) = base
        .rsplit_once('.')
        .map(|(t, e)| (t.to_string(), format!(".{e}")))
        .unwrap_or((base.clone(), ".lua".into()));
    let mut nome = base.clone();
    let mut contador = 2;
    while estado.scripts.join(&nome).exists() {
        nome = format!("{tronco}-{contador}{extensao}");
        contador += 1;
    }
    if let Err(e) = std::fs::write(estado.scripts.join(&nome), conteudo) {
        return json_de(500, &json!({"ok": false, "erro": format!("{e}")}));
    }
    json_de(200, &json!({"ok": true, "nome": nome}))
}

fn renomear_script(estado: &Arc<Estado>, pedido: &Pedido) -> Vec<u8> {
    let Ok(corpo) = serde_json::from_slice::<Value>(&pedido.corpo) else {
        return json_de(400, &json!({"ok": false, "erro": "invalid JSON"}));
    };
    let de = corpo.get("de").and_then(|v| v.as_str()).and_then(estado::nome_seguro);
    let para = corpo.get("para").and_then(|v| v.as_str()).and_then(estado::nome_seguro);
    let (Some(de), Some(para)) = (de, para) else {
        return json_de(400, &json!({"ok": false, "erro": "invalid names"}));
    };
    if estado.scripts.join(&para).exists() {
        return json_de(409, &json!({"ok": false, "erro": "a script with this name already exists"}));
    }
    match std::fs::rename(estado.scripts.join(&de), estado.scripts.join(&para)) {
        Ok(()) => json_de(200, &json!({"ok": true, "nome": para})),
        Err(e) => json_de(500, &json!({"ok": false, "erro": format!("{e}")})),
    }
}

fn excluir_script(estado: &Arc<Estado>, pedido: &Pedido) -> Vec<u8> {
    let Ok(corpo) = serde_json::from_slice::<Value>(&pedido.corpo) else {
        return json_de(400, &json!({"ok": false, "erro": "invalid JSON"}));
    };
    let Some(nome) = corpo.get("nome").and_then(|v| v.as_str()).and_then(estado::nome_seguro) else {
        return json_de(400, &json!({"ok": false, "erro": "invalid name"}));
    };
    match std::fs::remove_file(estado.scripts.join(&nome)) {
        Ok(()) => json_de(200, &json!({"ok": true})),
        Err(e) => json_de(500, &json!({"ok": false, "erro": format!("{e}")})),
    }
}

fn salvar_tema(pedido: &Pedido) -> Vec<u8> {
    let Ok(json) = serde_json::from_slice::<Value>(&pedido.corpo) else {
        return json_de(400, &json!({"ok": false, "erro": "invalid JSON"}));
    };
    match config::salvar_tema(&json) {
        Ok(tema) => json_de(200, &json!({"ok": true, "tema": tema})),
        Err(e) => json_de(500, &json!({"ok": false, "erro": e})),
    }
}

// ============================ HTTP cru =====================================

fn ler_pedido(stream: &mut TcpStream) -> Option<Pedido> {
    stream.set_read_timeout(Some(Duration::from_secs(15))).ok();
    let mut buf = Vec::new();
    let mut tmp = [0u8; 8192];

    let fim = loop {
        if let Some(p) = procurar(&buf, b"\r\n\r\n") {
            break p;
        }
        if buf.len() > 4 << 20 {
            return None;
        }
        let n = stream.read(&mut tmp).ok()?;
        if n == 0 {
            return None;
        }
        buf.extend_from_slice(&tmp[..n]);
    };

    let cabecalhos = String::from_utf8_lossy(&buf[..fim]).to_string();
    let mut linhas = cabecalhos.split("\r\n");
    let primeira = linhas.next()?;
    let mut partes = primeira.split_whitespace();
    let metodo = partes.next()?.to_string();
    let alvo = partes.next()?.to_string();

    let mut tamanho = 0usize;
    let mut tipo = String::new();
    let mut host = String::new();
    let mut origem = None;
    for linha in linhas {
        if let Some((chave, valor)) = linha.split_once(':') {
            if chave.eq_ignore_ascii_case("content-length") {
                tamanho = valor.trim().parse().unwrap_or(0);
            } else if chave.eq_ignore_ascii_case("content-type") {
                tipo = valor.trim().to_string();
            } else if chave.eq_ignore_ascii_case("host") {
                host = valor.trim().to_string();
            } else if chave.eq_ignore_ascii_case("origin") {
                // "null" vem de iframe sandbox/arquivo: nunca é a UI
                origem = Some(valor.trim().to_string());
            }
        }
    }

    if tamanho > LIMITE_CORPO {
        return None;
    }
    let mut corpo = buf[fim + 4..].to_vec();
    while corpo.len() < tamanho {
        let n = stream.read(&mut tmp).ok()?;
        if n == 0 {
            break;
        }
        corpo.extend_from_slice(&tmp[..n]);
    }
    corpo.truncate(tamanho);

    let (rota, consulta) = alvo.split_once('?').unwrap_or((alvo.as_str(), ""));
    let query = consulta
        .split('&')
        .filter(|p| !p.is_empty())
        .filter_map(|p| {
            let (k, v) = p.split_once('=')?;
            Some((decodificar(k), decodificar(v)))
        })
        .collect();

    Some(Pedido {
        metodo,
        rota: rota.to_string(),
        query,
        tipo,
        host,
        origem,
        corpo,
    })
}

fn procurar(hay: &[u8], needle: &[u8]) -> Option<usize> {
    hay.windows(needle.len()).position(|w| w == needle)
}

pub(super) fn parametro(query: &[(String, String)], chave: &str) -> Option<String> {
    query
        .iter()
        .find(|(k, _)| k == chave)
        .map(|(_, v)| v.clone())
}

fn decodificar(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut saida = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'%' if i + 2 < bytes.len() => {
                let hex = std::str::from_utf8(&bytes[i + 1..i + 3]).ok();
                if let Some(v) = hex.and_then(|h| u8::from_str_radix(h, 16).ok()) {
                    saida.push(v);
                    i += 3;
                    continue;
                }
                saida.push(b'%');
                i += 1;
            }
            b'+' => {
                saida.push(b' ');
                i += 1;
            }
            b => {
                saida.push(b);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&saida).to_string()
}

pub(super) fn json_de(status: u16, valor: &impl serde::Serialize) -> Vec<u8> {
    let corpo = serde_json::to_vec(valor).unwrap_or_else(|_| b"{}".to_vec());
    let motivo = match status {
        400 => "400 Bad Request",
        403 => "403 Forbidden",
        404 => "404 Not Found",
        409 => "409 Conflict",
        500 => "500 Internal Server Error",
        503 => "503 Service Unavailable",
        504 => "504 Gateway Timeout",
        _ => "200 OK",
    };
    montar(motivo, "application/json; charset=utf-8", &corpo)
}

pub(super) fn montar(status: &str, tipo: &str, corpo: &[u8]) -> Vec<u8> {
    let mut resposta = Vec::with_capacity(corpo.len() + 160);
    resposta.extend_from_slice(format!("HTTP/1.1 {status}\r\n").as_bytes());
    resposta.extend_from_slice(format!("Content-Type: {tipo}\r\n").as_bytes());
    resposta.extend_from_slice(format!("Content-Length: {}\r\n", corpo.len()).as_bytes());
    resposta.extend_from_slice(b"Cache-Control: no-store\r\n");
    resposta.extend_from_slice(b"Connection: close\r\n\r\n");
    resposta.extend_from_slice(corpo);
    resposta
}

#[cfg(test)]
mod testes {
    use super::*;

    fn pedido(host: &str, origem: Option<&str>) -> Pedido {
        Pedido {
            metodo: "POST".into(),
            rota: "/api/exec".into(),
            query: vec![],
            tipo: "text/plain".into(),
            host: host.into(),
            origem: origem.map(str::to_string),
            corpo: vec![],
        }
    }

    #[test]
    fn so_aceita_a_propria_origem() {
        assert!(pedido_local(&pedido("127.0.0.1:4000", None), 4000));
        assert!(pedido_local(&pedido("127.0.0.1:4000", Some("http://127.0.0.1:4000")), 4000));
        assert!(!pedido_local(&pedido("127.0.0.1:4000", Some("https://site.mau")), 4000));
        assert!(!pedido_local(&pedido("127.0.0.1:4000", Some("null")), 4000));
        // DNS rebinding: o navegador manda o nome do domínio no Host
        assert!(!pedido_local(&pedido("mau.exemplo:4000", None), 4000));
        assert!(!pedido_local(&pedido("127.0.0.1:4001", None), 4000));
    }
}
