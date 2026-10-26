//! Rotas de clientes: lista, anexar/desanexar, alvo e headshot do jogador.
//!
//! A sonda nunca chama função do jogo aqui. O nome vem de uma linha `LUARIA_PLAYER`
//! que o próprio script de perfil imprime no log do cliente (ver `estado.rs`).

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::mpsc::Sender;
use std::sync::Arc;
use std::time::{Duration, SystemTime};

use serde_json::{json, Value};
use luaria::clientes::{classificar as classificar_cliente, listar_clientes, StatusCliente};

use super::estado::{Comando, Estado};
use super::servidor::{json_de, montar, parametro};

/// Headshot em cache vale 24 h; depois disso baixa de novo.
const TTL_HEADSHOT: Duration = Duration::from_secs(24 * 60 * 60);
/// Teto do PNG baixado: headshot 150x150 fica bem abaixo disso.
const LIMITE_PNG: &str = "1048576";

#[derive(Clone)]
pub struct Perfil {
    pub nome: String,
    pub display: Option<String>,
    pub user_id: u64,
}

/// Quais clientes a UI marcou como alvo. `Todos` vale para qualquer pid.
#[derive(Clone, PartialEq, Debug)]
pub enum AlvoClientes {
    Todos,
    Pids(Vec<i32>),
}

/// `LUARIA_PLAYER <pid> <userid> <name> <display>`. O display pode ter espaço,
/// por isso o resto da linha inteira vira display; vazio = sem display.
pub fn ler_perfil(linha: &str) -> Option<(i32, Perfil)> {
    let resto = linha.split_once("LUARIA_PLAYER")?.1;
    let mut campos = resto.split_whitespace();
    let pid = campos.next()?.parse().ok()?;
    let user_id = campos.next()?.parse().ok()?;
    let nome = campos.next()?.to_string();
    let display = campos.collect::<Vec<_>>().join(" ");
    let perfil = Perfil {
        nome,
        display: (!display.is_empty()).then_some(display),
        user_id,
    };
    Some((pid, perfil))
}

pub fn listar(estado: &Arc<Estado>) -> Vec<u8> {
    let anexos = match falar_com_motor(estado, Duration::from_secs(2), |resposta| Comando::Clientes { resposta }) {
        Ok(anexos) => anexos,
        Err(resposta) => return resposta,
    };
    let vivos = listar_clientes();
    // motor anexado a um cliente que já não aparece na lista é um cliente morto
    let mut pids: Vec<i32> = vivos.iter().map(|c| c.pid).collect();
    for (pid, _) in &anexos {
        if !pids.contains(pid) {
            pids.push(*pid);
        }
    }
    let principal = anexos.first().map(|(p, _)| *p);
    let alvo = estado.alvo.lock().unwrap().clone();
    let perfis = estado.perfis.lock().unwrap().clone();

    let itens: Vec<Value> = pids
        .into_iter()
        .map(|pid| {
            let vivo = vivos.iter().any(|c| c.pid == pid);
            let anexo = anexos.iter().find(|(p, _)| *p == pid).map(|(_, e)| *e);
            let status = classificar_cliente(vivo, anexo);
            let perfil = perfis.get(&pid);
            json!({
                "pid": pid,
                "nome": perfil.map(|p| &p.nome),
                "display": perfil.and_then(|p| p.display.clone()),
                "user_id": perfil.map(|p| p.user_id),
                "place_id": match status {
                    StatusCliente::Jogo(p) => Some(p),
                    _ => None,
                },
                "status": texto_status(status),
                "anexado": anexo.is_some(),
                "alvo": no_alvo(&alvo, pid),
                "ativo": principal == Some(pid),
            })
        })
        .collect();
    json_de(200, &itens)
}

fn texto_status(status: StatusCliente) -> &'static str {
    match status {
        StatusCliente::Morto => "morto",
        StatusCliente::NaoAnexado => "nao_anexado",
        StatusCliente::Anexado => "anexado",
        StatusCliente::Menu => "menu",
        StatusCliente::Jogo(_) => "jogo",
    }
}

fn no_alvo(alvo: &AlvoClientes, pid: i32) -> bool {
    match alvo {
        AlvoClientes::Todos => true,
        AlvoClientes::Pids(pids) => pids.contains(&pid),
    }
}

pub fn anexar(estado: &Arc<Estado>, corpo: &[u8]) -> Vec<u8> {
    let pid = match pid_listado(corpo) {
        Ok(pid) => pid,
        Err(resposta) => return resposta,
    };
    match falar_com_motor(estado, Duration::from_secs(20), |resposta| Comando::AnexarPid { pid, resposta }) {
        Ok(Ok(())) => json_de(200, &json!({"ok": true, "pid": pid})),
        Ok(Err(erro)) => json_de(500, &json!({"ok": false, "erro": erro})),
        Err(resposta) => resposta,
    }
}

pub fn desanexar(estado: &Arc<Estado>, corpo: &[u8]) -> Vec<u8> {
    let pid = match pid_listado(corpo) {
        Ok(pid) => pid,
        Err(resposta) => return resposta,
    };
    match falar_com_motor(estado, Duration::from_secs(5), |resposta| Comando::Desanexar { pid, resposta }) {
        Ok(()) => json_de(200, &json!({"ok": true})),
        Err(resposta) => resposta,
    }
}

pub fn definir_alvo(estado: &Estado, corpo: &[u8]) -> Vec<u8> {
    let Ok(dados) = serde_json::from_slice::<Value>(corpo) else {
        return json_de(400, &json!({"ok": false, "erro": "invalid JSON"}));
    };
    let listados: Vec<i32> = listar_clientes().iter().map(|c| c.pid).collect();
    let novo = match &dados["pids"] {
        Value::String(texto) if texto == "todos" => AlvoClientes::Todos,
        Value::Array(itens) => {
            let pids: Option<Vec<i32>> = itens
                .iter()
                .map(|v| v.as_i64().and_then(|p| i32::try_from(p).ok()))
                .collect();
            let Some(pids) = pids else {
                return json_de(400, &json!({"ok": false, "erro": "invalid pid"}));
            };
            if pids.iter().any(|p| !listados.contains(p)) {
                return json_de(404, &json!({"ok": false, "erro": "client not found"}));
            }
            AlvoClientes::Pids(pids)
        }
        _ => {
            return json_de(400, &json!({"ok": false, "erro": "field 'pids' must be a list or \"todos\""}));
        }
    };
    *estado.alvo.lock().unwrap() = novo;
    json_de(200, &json!({"ok": true}))
}

/// Pid do corpo, aceito só se for um cliente listado agora: a UI não manda pid
/// que o host não tenha visto.
fn pid_listado(corpo: &[u8]) -> Result<i32, Vec<u8>> {
    let Ok(dados) = serde_json::from_slice::<Value>(corpo) else {
        return Err(json_de(400, &json!({"ok": false, "erro": "invalid JSON"})));
    };
    let Some(pid) = dados["pid"].as_i64().and_then(|p| i32::try_from(p).ok()) else {
        return Err(json_de(400, &json!({"ok": false, "erro": "invalid pid"})));
    };
    if !listar_clientes().iter().any(|c| c.pid == pid) {
        return Err(json_de(404, &json!({"ok": false, "erro": "client not found"})));
    }
    Ok(pid)
}

/// Manda um comando para a thread do motor e espera a resposta. Falha já sai como HTTP.
fn falar_com_motor<T>(
    estado: &Estado,
    limite: Duration,
    comando: impl FnOnce(Sender<T>) -> Comando,
) -> Result<T, Vec<u8>> {
    let Some(tx) = estado.comando() else {
        return Err(json_de(503, &json!({"ok": false, "erro": "engine not started"})));
    };
    let (rtx, rrx) = std::sync::mpsc::channel();
    if tx.send(comando(rtx)).is_err() {
        return Err(json_de(503, &json!({"ok": false, "erro": "engine thread died"})));
    }
    rrx.recv_timeout(limite)
        .map_err(|_| json_de(504, &json!({"ok": false, "erro": "engine did not respond"})))
}

/// `GET /api/headshot?uid=`: PNG do avatar, do cache ou da CDN da Roblox.
pub fn headshot(query: &[(String, String)]) -> Vec<u8> {
    let Some(uid) = parametro(query, "uid").as_deref().and_then(uid_valido) else {
        return json_de(400, &json!({"ok": false, "erro": "invalid uid"}));
    };
    let caminho = pasta_headshots().join(format!("{uid}.png"));
    if let Some(png) = ler_cache(&caminho, SystemTime::now()) {
        return montar("200 OK", "image/png", &png);
    }
    let Some(png) = buscar_headshot(uid) else {
        return json_de(404, &json!({"ok": false, "erro": "headshot unavailable"}));
    };
    // cache é opcional: se não gravar, o próximo pedido só baixa de novo
    let _ = fs::create_dir_all(pasta_headshots()).and_then(|()| fs::write(&caminho, &png));
    montar("200 OK", "image/png", &png)
}

/// Só dígitos, e cabe em u64. Sinal, espaço ou sufixo viram 400 antes de virar URL.
fn uid_valido(texto: &str) -> Option<u64> {
    if texto.is_empty() || !texto.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    texto.parse().ok()
}

fn pasta_headshots() -> PathBuf {
    let home = std::env::var("HOME").unwrap_or_else(|_| ".".into());
    PathBuf::from(home).join(".cache/luaria/headshots")
}

fn ler_cache(caminho: &Path, agora: SystemTime) -> Option<Vec<u8>> {
    let gravado = fs::metadata(caminho).ok()?.modified().ok()?;
    if agora.duration_since(gravado).unwrap_or_default() > TTL_HEADSHOT {
        return None;
    }
    fs::read(caminho).ok()
}

fn buscar_headshot(uid: u64) -> Option<Vec<u8>> {
    let url = format!(
        "https://thumbnails.roblox.com/v1/users/avatar-headshot?userIds={uid}&size=150x150&format=Png"
    );
    let resposta = baixar(&url)?;
    let imagem = imagem_url(&resposta)?;
    let png = baixar(&imagem)?;
    png.starts_with(b"\x89PNG").then_some(png)
}

/// `curl` sem shell, só https, sem seguir redirecionamento (o host já foi validado).
fn baixar(url: &str) -> Option<Vec<u8>> {
    let saida = Command::new("curl")
        .args(["-fsS", "--max-time", "5", "--proto", "=https", "--max-filesize", LIMITE_PNG, url])
        .output()
        .ok()?;
    saida.status.success().then_some(saida.stdout)
}

/// `data[0].imageUrl` da API de thumbnails. A URL vem de rede: só a CDN https passa.
fn imagem_url(resposta: &[u8]) -> Option<String> {
    let dados: Value = serde_json::from_slice(resposta).ok()?;
    let url = dados["data"][0]["imageUrl"].as_str()?;
    host_permitido(url).then(|| url.to_string())
}

fn host_permitido(url: &str) -> bool {
    let Some(resto) = url.strip_prefix("https://") else {
        return false;
    };
    // `@` e `:` ficam no host e reprovam: não há host confiável com eles
    resto.split(['/', '?', '#']).next() == Some("tr.rbxcdn.com")
}

#[cfg(test)]
mod testes {
    use super::*;

    #[test]
    fn uid_so_com_digitos() {
        assert_eq!(uid_valido("12"), Some(12));
        assert_eq!(uid_valido("1;x"), None);
        assert_eq!(uid_valido(""), None);
        assert_eq!(uid_valido("+1"), None);
        assert_eq!(uid_valido("99999999999999999999999"), None);
    }

    #[test]
    fn cache_expira_depois_de_24_horas() {
        let dir = std::env::temp_dir().join(format!("luaria-headshot-teste-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let caminho = dir.join("1.png");
        fs::write(&caminho, b"\x89PNG").unwrap();
        let agora = SystemTime::now();
        assert!(ler_cache(&caminho, agora).is_some());
        assert!(ler_cache(&caminho, agora + TTL_HEADSHOT + Duration::from_secs(1)).is_none());
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn le_linha_do_perfil_com_display_com_espaco() {
        let (pid, perfil) =
            ler_perfil("[FLog::CreatorOutput] LUARIA_PLAYER\t123\t456\tNome_x\tNome Exibido").unwrap();
        assert_eq!(pid, 123);
        assert_eq!(perfil.nome, "Nome_x");
        assert_eq!(perfil.display.as_deref(), Some("Nome Exibido"));
        assert_eq!(perfil.user_id, 456);
    }

    #[test]
    fn perfil_sem_display_fica_none() {
        let (_, perfil) = ler_perfil("LUARIA_PLAYER 7 8 Nome ").unwrap();
        assert_eq!(perfil.display, None);
    }

    #[test]
    fn linha_de_perfil_malformada_e_ignorada() {
        assert!(ler_perfil("LUARIA_PLAYER abc 1 nome").is_none());
        assert!(ler_perfil("LUARIA_PLAYER 1 2").is_none());
        assert!(ler_perfil("sem a marca 1 2 nome").is_none());
    }

    #[test]
    fn so_aceita_host_da_cdn_roblox_por_https() {
        assert!(host_permitido("https://tr.rbxcdn.com/abc.png"));
        assert!(!host_permitido("http://tr.rbxcdn.com/abc.png"));
        assert!(!host_permitido("https://tr.rbxcdn.com.evil.com/x.png"));
        assert!(!host_permitido("https://tr.rbxcdn.com@evil.com/x.png"));
        assert!(!host_permitido("https://evil.com/?tr.rbxcdn.com"));
    }
}
