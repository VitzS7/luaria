//! Resolve no host, antes de compilar, `game:HttpGet("url")` e `loadstring(<literal>)`.
//!
//! O cliente não tem compilador Luau e o jogo fica parado durante a execução, então
//! não dá para buscar nada em tempo de execução: o que é literal vira texto aqui.
//! Conteúdo baixado é não confiável: só é tratado como texto, nunca executado no host.
//!
//! shortcut: scanner léxico simples (strings/comentários), sem parser Lua completo;
//! o número de linha dos erros só é exato antes da 1ª reescrita, porque o conteúdo
//! inlinado desloca as linhas seguintes. Trocar por parser se isso incomodar.

use std::collections::HashMap;
use std::io::Write;
use std::net::{IpAddr, ToSocketAddrs};
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant, SystemTime};

use crate::ponte_codec::{PedidoHttp, RespostaHttp};
use crate::ponte_layout::MAX_RESP;

const PROF_MAX: usize = 4;
const MAX_URLS: usize = 24;
const ORCAMENTO: usize = 3 * 1024 * 1024;
const VALIDADE_CACHE: Duration = Duration::from_secs(600);
/// `HttpGet` do host, sem o limite do anel: o conteúdo inlinado pode passar de 448 KB.
const LIMITE_HTTPGET: usize = 2_000_000;
const TEMPO_HTTPGET: Duration = Duration::from_secs(20);
const LIMITE_PONTE: usize = MAX_RESP;
const TEMPO_PONTE: Duration = Duration::from_secs(30);
const MAX_REDIRECTS: u32 = 5;

struct Estado<'a> {
    baixar: &'a dyn Fn(&str) -> Option<String>,
    compila: &'a dyn Fn(&str) -> bool,
    memo: HashMap<String, Option<String>>,
    urls: usize,
    bytes: usize,
    log: &'a mut Vec<String>,
    houve: bool,
}

impl Estado<'_> {
    fn obter(&mut self, url: &str) -> Option<String> {
        if let Some(r) = self.memo.get(url) {
            return r.clone();
        }
        if self.urls >= MAX_URLS {
            self.log.push(format!("luaria: remote: limit of {MAX_URLS} URLs, skipping {url}"));
            return None;
        }
        self.urls += 1;
        let r = match (self.baixar)(url) {
            None => {
                self.log.push(format!("luaria: remote: failed to download {url} (network, cache or blocked host)"));
                None
            }
            Some(t) if self.bytes + t.len() > ORCAMENTO => {
                self.log.push(format!("luaria: remote: {} MB budget exceeded at {url}", ORCAMENTO >> 20));
                None
            }
            Some(t) => {
                self.bytes += t.len();
                self.log.push(format!("luaria: remoto: inlinado {url} ({} bytes)", t.len()));
                Some(t)
            }
        };
        self.memo.insert(url.to_string(), r.clone());
        r
    }
}

/// Devolve o código reescrito e se houve alguma reescrita.
pub fn resolver(
    src: &str,
    baixar: &dyn Fn(&str) -> Option<String>,
    compila: &dyn Fn(&str) -> bool,
    log: &mut Vec<String>,
) -> (String, bool) {
    let mut st = Estado { baixar, compila, memo: HashMap::new(), urls: 0, bytes: 0, log, houve: false };
    let saida = reescrever(src, 0, &mut st);
    (saida, st.houve)
}

pub fn resolver_remoto(src: &str, log: &mut Vec<String>) -> (String, bool) {
    resolver(src, &baixar_url, &|c| crate::engine::compilar(c).is_ok(), log)
}

fn eh_ident(c: u8) -> bool {
    c.is_ascii_alphanumeric() || c == b'_'
}

fn ident(b: &[u8], i: usize) -> (&str, usize) {
    let mut f = i;
    while f < b.len() && eh_ident(b[f]) {
        f += 1;
    }
    (std::str::from_utf8(&b[i..f]).unwrap_or(""), f)
}

fn pular_espaco(b: &[u8], mut i: usize) -> usize {
    while i < b.len() && b[i].is_ascii_whitespace() {
        i += 1;
    }
    i
}

/// `[=*[` em `i`: devolve (nível, início do conteúdo).
fn abre_longo(b: &[u8], i: usize) -> Option<(usize, usize)> {
    if b.get(i) != Some(&b'[') {
        return None;
    }
    let mut j = i + 1;
    while b.get(j) == Some(&b'=') {
        j += 1;
    }
    (b.get(j) == Some(&b'[')).then_some((j - i - 1, j + 1))
}

/// Posição logo depois do fechamento do bracket longo (ou fim do texto).
fn fim_longo(b: &[u8], nivel: usize, ini: usize) -> usize {
    let mut fecha = vec![b'='; nivel + 2];
    fecha[0] = b']';
    fecha[nivel + 1] = b']';
    b[ini..].windows(fecha.len()).position(|w| w == fecha.as_slice()).map_or(b.len(), |p| ini + p + fecha.len())
}

fn desescapar(cru: &str) -> Option<String> {
    let mut s = String::new();
    let mut it = cru.chars().peekable();
    while let Some(c) = it.next() {
        if c != '\\' {
            s.push(c);
            continue;
        }
        match it.next()? {
            'n' => s.push('\n'),
            't' => s.push('\t'),
            'r' => s.push('\r'),
            '\\' => s.push('\\'),
            '"' => s.push('"'),
            '\'' => s.push('\''),
            d if d.is_ascii_digit() => {
                let mut n = d.to_digit(10)?;
                for _ in 0..2 {
                    match it.peek().and_then(|c| c.to_digit(10)) {
                        Some(x) => {
                            n = n * 10 + x;
                            it.next();
                        }
                        None => break,
                    }
                }
                if n >= 128 {
                    return None;
                }
                s.push(n as u8 as char);
            }
            _ => return None,
        }
    }
    Some(s)
}

/// Literal de string (aspas ou longo) em `i`: (conteúdo, posição depois).
fn literal(src: &str, i: usize) -> Option<(String, usize)> {
    let b = src.as_bytes();
    let c = *b.get(i)?;
    if c == b'[' {
        let (nivel, ini) = abre_longo(b, i)?;
        let fim = fim_longo(b, nivel, ini);
        let corpo_fim = fim.checked_sub(nivel + 2).filter(|f| *f >= ini)?;
        let mut corpo = &src[ini..corpo_fim];
        // o Lua descarta a primeira quebra de linha logo após `[[`
        corpo = corpo.strip_prefix("\r\n").or_else(|| corpo.strip_prefix('\n')).unwrap_or(corpo);
        return Some((corpo.to_string(), fim));
    }
    if c != b'"' && c != b'\'' {
        return None;
    }
    let mut j = i + 1;
    while j < b.len() && b[j] != c {
        if b[j] == b'\n' {
            return None;
        }
        j += if b[j] == b'\\' { 2 } else { 1 };
    }
    if j >= b.len() {
        return None;
    }
    Some((desescapar(&src[i + 1..j])?, j + 1))
}

/// `game:HttpGet("u"[, bool])` com `i` logo depois do identificador `game`.
fn tentar_httpget(src: &str, i: usize, st: &mut Estado) -> Option<(String, usize)> {
    let b = src.as_bytes();
    let mut j = pular_espaco(b, i);
    if b.get(j) != Some(&b':') {
        return None;
    }
    j = pular_espaco(b, j + 1);
    let (nome, f) = ident(b, j);
    if nome != "HttpGet" && nome != "HttpGetAsync" {
        return None;
    }
    j = pular_espaco(b, f);
    if b.get(j) != Some(&b'(') {
        return None;
    }
    j = pular_espaco(b, j + 1);
    let (url, f) = literal(src, j)?;
    j = pular_espaco(b, f);
    if b.get(j) == Some(&b',') {
        j = pular_espaco(b, j + 1);
        let (flag, f) = ident(b, j);
        if flag != "true" && flag != "false" {
            return None;
        }
        j = pular_espaco(b, f);
    }
    if b.get(j) != Some(&b')') {
        return None;
    }
    let texto = st.obter(&url)?;
    Some((texto, j + 1))
}

/// `loadstring(<arg>[, "nome"])` com `i` logo depois do identificador.
fn tentar_loadstring(src: &str, i: usize, st: &mut Estado) -> Option<(String, usize)> {
    let b = src.as_bytes();
    let mut j = pular_espaco(b, i);
    if b.get(j) != Some(&b'(') {
        return None;
    }
    j = pular_espaco(b, j + 1);
    let (id, f) = ident(b, j);
    let (fonte, f) = if id == "game" { tentar_httpget(src, f, st)? } else { literal(src, j)? };
    j = pular_espaco(b, f);
    if b.get(j) == Some(&b',') {
        j = pular_espaco(b, j + 1);
        let (_, f) = literal(src, j)?;
        j = pular_espaco(b, f);
    }
    (b.get(j) == Some(&b')')).then_some((fonte, j + 1))
}

fn literal_longo(texto: &str) -> String {
    let mut nivel = 0;
    loop {
        let eq = "=".repeat(nivel);
        let fecha = format!("]{eq}]");
        if format!("{texto}{fecha}").find(&fecha) == Some(texto.len()) {
            // a primeira quebra após a abertura é descartada, então dobra se preciso
            let quebra = if texto.starts_with('\n') || texto.starts_with('\r') { "\n" } else { "" };
            return format!("[{eq}[{quebra}{texto}{fecha}");
        }
        nivel += 1;
    }
}

fn limpar_fonte(t: &str) -> String {
    let t = t.strip_prefix('\u{feff}').unwrap_or(t);
    if t.starts_with("#!") {
        // mantém a quebra para não deslocar linhas do conteúdo
        return t[t.find('\n').unwrap_or(t.len())..].to_string();
    }
    t.to_string()
}

/// Sem `;`, `x = y` + quebra + `(function...` vira chamada `y(function...)`.
fn reescrever(src: &str, prof: usize, st: &mut Estado) -> String {
    let b = src.as_bytes();
    let mut out = String::with_capacity(src.len());
    let (mut i, mut ultimo) = (0, 0);
    while i < b.len() {
        let c = b[i];
        if c == b'-' && b.get(i + 1) == Some(&b'-') {
            i += 2;
            if let Some((n, ini)) = abre_longo(b, i) {
                i = fim_longo(b, n, ini);
            } else {
                while i < b.len() && b[i] != b'\n' {
                    i += 1;
                }
            }
        } else if c == b'"' || c == b'\'' {
            i += 1;
            while i < b.len() && b[i] != c && b[i] != b'\n' {
                i += if b[i] == b'\\' { 2 } else { 1 };
            }
            i += 1;
        } else if c == b'[' {
            i = abre_longo(b, i).map_or(i + 1, |(n, ini)| fim_longo(b, n, ini));
        } else if c.is_ascii_alphabetic() || c == b'_' {
            let (id, fim) = ident(b, i);
            let atras_ponto = i > 0 && matches!(b[i - 1], b'.' | b':');
            if atras_ponto || (id != "loadstring" && id != "game") {
                i = fim;
                continue;
            }
            let nome = id;
            let achou = if prof >= PROF_MAX {
                None
            } else if nome == "game" {
                tentar_httpget(src, fim, st).map(|(t, f)| (literal_longo(&t), f))
            } else {
                // código que não compila fica para o loadstring do prelude, que
                // devolve (nil, erro) como o real em vez de quebrar o chunk inteiro
                tentar_loadstring(src, fim, st).map(|(t, f)| (limpar_fonte(&t), f)).and_then(|(t, f)| {
                    if !(st.compila)(&t) {
                        st.log.push("luaria: remote: loadstring source does not compile, left to the interpreter".into());
                        return None;
                    }
                    let corpo = reescrever(&t, prof + 1, st);
                    Some((format!("__luaria_fn(function(...) {corpo}\nend)"), f))
                })
            };
            match achou {
                Some((novo, f)) => {
                    out.push_str(&src[ultimo..i]);
                    out.push_str(&novo);
                    st.houve = true;
                    i = f;
                    ultimo = f;
                }
                None => i = fim,
            }
        } else {
            i += 1;
        }
    }
    if prof >= PROF_MAX && src.contains("loadstring") {
        st.log.push("luaria: remote: max nested loadstring depth, left as is".into());
    }
    out.push_str(&src[ultimo.min(src.len())..]);
    out
}

fn hash_fnv(s: &str) -> u64 {
    s.bytes().fold(0xcbf29ce484222325, |h, b| (h ^ b as u64).wrapping_mul(0x100000001b3))
}

/// (host, porta) de uma URL http(s); `None` para outro esquema ou host vazio.
fn alvo(url: &str) -> Option<(String, u16)> {
    let (resto, porta) = match url.strip_prefix("https://") {
        Some(r) => (r, 443),
        None => (url.strip_prefix("http://")?, 80),
    };
    let autoridade = resto.split(['/', '?', '#']).next().unwrap_or("");
    let autoridade = autoridade.rsplit('@').next().unwrap_or("");
    let (host, porta) = if let Some(v6) = autoridade.strip_prefix('[') {
        let (h, depois) = v6.split_once(']')?;
        (h, depois.strip_prefix(':').map_or(Some(porta), |p| p.parse().ok())?)
    } else {
        match autoridade.split_once(':') {
            Some((h, p)) => (h, p.parse().ok()?),
            None => (autoridade, porta),
        }
    };
    (!host.is_empty()).then(|| (host.to_ascii_lowercase(), porta))
}

fn ip_privado(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => {
            let o = v4.octets();
            v4.is_private() || v4.is_loopback() || v4.is_link_local() || v4.is_unspecified()
                || v4.is_broadcast() || o[0] == 0 || (o[0] == 100 && (64..128).contains(&o[1]))
        }
        IpAddr::V6(v6) => {
            let s = v6.segments();
            v6.is_loopback() || v6.is_unspecified() || (s[0] & 0xfe00) == 0xfc00 || (s[0] & 0xffc0) == 0xfe80
                || v6.to_ipv4_mapped().is_some_and(|v4| ip_privado(IpAddr::V4(v4)))
        }
    }
}

fn host_bloqueado(url: &str) -> bool {
    let Some((host, _)) = alvo(url) else { return true };
    if host == "localhost" || host.ends_with(".localhost") {
        return true;
    }
    host.trim_start_matches('[').trim_end_matches(']').parse::<IpAddr>().is_ok_and(ip_privado)
}

/// Resolve o host e devolve um IP público para fixar no curl. Sem fixar, um DNS
/// que responde 127.0.0.1 (ou muda entre a checagem e a conexão) passa reto.
fn ip_publico(url: &str, resolver: &dyn Fn(&str, u16) -> Vec<IpAddr>) -> Option<(String, u16, IpAddr)> {
    if host_bloqueado(url) {
        return None;
    }
    let (host, porta) = alvo(url)?;
    let mut ips = resolver(&host, porta);
    if ips.is_empty() || ips.iter().any(|ip| ip_privado(*ip)) {
        return None;
    }
    // IPv4 primeiro: com o IP fixado no curl não há fallback, e IPv6 quebrado
    // fazia cada download esperar os 20 s do timeout antes de cair no cache velho.
    ips.sort_by_key(|ip| ip.is_ipv6());
    Some((host, porta, ips[0]))
}

fn dir_cache() -> Option<PathBuf> {
    Some(PathBuf::from(std::env::var("HOME").ok()?).join(".cache/luaria/http"))
}

fn resolver_sistema(host: &str, porta: u16) -> Vec<IpAddr> {
    (host, porta).to_socket_addrs().map(|it| it.map(|a| a.ip()).collect()).unwrap_or_default()
}

/// `bruta` são cabeçalhos + corpo (curl `-i`). `truncada` marca que o curl parou
/// no `--max-filesize` (código 63): o que chegou até ali ainda vai junto.
struct Saida {
    bruta: Vec<u8>,
    truncada: bool,
}

/// Status 0 na resposta significa truncada; o Lua não pode tratar isso como sucesso.
pub fn pedir_http(p: &PedidoHttp) -> Result<RespostaHttp, String> {
    p.validar().map_err(|e| e.to_string())?;
    pedir_com(p, LIMITE_PONTE, TEMPO_PONTE, &resolver_sistema, &executar_curl)
}

/// Segue redirect na mão: o `-L` do curl iria para qualquer host, inclusive
/// 127.0.0.1, sem passar pela checagem. Cada salto revalida host e IP.
fn pedir_com(
    p: &PedidoHttp,
    limite: usize,
    tempo: Duration,
    resolver: &dyn Fn(&str, u16) -> Vec<IpAddr>,
    executar: &dyn Fn(&[String], &[u8]) -> Option<Saida>,
) -> Result<RespostaHttp, String> {
    let inicio = Instant::now();
    let mut url = p.url.clone();
    let mut metodo = p.metodo.clone();
    let mut corpo: &[u8] = &p.corpo;
    let mut saltos = 0;
    loop {
        let restante = tempo.checked_sub(inicio.elapsed()).ok_or("tempo total estourado")?;
        let (host, porta, ip) = ip_publico(&url, resolver)
            .ok_or_else(|| format!("destination refused (host blocked or no public IP): {url}"))?;
        let ip_txt = if ip.is_ipv6() { format!("[{ip}]") } else { ip.to_string() };
        let fixo = format!("{host}:{porta}:{ip_txt}");
        let args = montar_args(&metodo, &url, &fixo, restante.as_secs().max(1), limite, &p.headers, !corpo.is_empty());
        let saida = executar(&args, corpo).ok_or_else(|| format!("curl failed on {url}"))?;
        let mut resp = parsear_resposta(&saida.bruta)?;
        if saida.truncada || resp.corpo.len() > limite {
            resp.corpo.truncate(limite);
            resp.status = 0;
            resp.mensagem = format!("response larger than {} KB, truncated", limite / 1024);
            return Ok(resp);
        }
        let destino = match resp.status {
            301 | 302 | 303 | 307 | 308 => location(&resp).map(str::to_owned),
            _ => None,
        };
        let Some(loc) = destino else { return Ok(resp) };
        if saltos == MAX_REDIRECTS {
            return Err(format!("more than {MAX_REDIRECTS} redirects"));
        }
        saltos += 1;
        url = destino_redirect(&url, &loc).ok_or_else(|| format!("redirect to unsupported destination: {loc}"))?;
        if matches!(resp.status, 301 | 302 | 303) {
            metodo = "GET".into();
            corpo = &[];
        }
    }
}

fn montar_args(
    metodo: &str,
    url: &str,
    fixo: &str,
    tempo_s: u64,
    limite: usize,
    headers: &[(String, String)],
    com_corpo: bool,
) -> Vec<String> {
    let mut a: Vec<String> = vec![
        "-sS".into(),
        "-i".into(),
        "--connect-timeout".into(),
        "6".into(),
        "--max-time".into(),
        tempo_s.to_string(),
        "--max-filesize".into(),
        limite.to_string(),
        "--proto".into(),
        "=http,https".into(),
        "--proto-redir".into(),
        "=http,https".into(),
        "--resolve".into(),
        fixo.into(),
        "-X".into(),
        metodo.into(),
        // sem isso o POST ganha um `100 Continue` antes da resposta e o parse vê dois cabeçalhos
        "-H".into(),
        "Expect:".into(),
    ];
    if !headers.iter().any(|(n, _)| n.eq_ignore_ascii_case("user-agent")) {
        a.extend(["-H".into(), "User-Agent: Roblox/WinInet".into()]);
    }
    for (nome, valor) in headers {
        // `Nome:` sem valor faz o curl tirar o header; `Nome;` manda o vazio de verdade
        let linha = if valor.is_empty() { format!("{nome};") } else { format!("{nome}: {valor}") };
        a.extend(["-H".into(), linha]);
    }
    if com_corpo {
        a.extend(["--data-binary".into(), "@-".into()]);
    }
    a.extend(["--".into(), url.into()]);
    a
}

fn executar_curl(args: &[String], corpo: &[u8]) -> Option<Saida> {
    let mut filho = Command::new("curl")
        .args(args)
        .stdin(if corpo.is_empty() { Stdio::null() } else { Stdio::piped() })
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;
    if let Some(mut entrada) = filho.stdin.take() {
        entrada.write_all(corpo).ok()?;
    }
    let o = filho.wait_with_output().ok()?;
    match o.status.code() {
        Some(0) => Some(Saida { bruta: o.stdout, truncada: false }),
        Some(63) => Some(Saida { bruta: o.stdout, truncada: true }),
        _ => None,
    }
}

fn parsear_resposta(bruta: &[u8]) -> Result<RespostaHttp, String> {
    let corte = bruta.windows(4).position(|w| w == b"\r\n\r\n").ok_or("response without headers")?;
    let cabecalho = String::from_utf8_lossy(&bruta[..corte]);
    let mut linhas = cabecalho.split("\r\n");
    let mut partes = linhas.next().unwrap_or("").splitn(3, ' ');
    if !partes.next().unwrap_or("").starts_with("HTTP/") {
        return Err("response without HTTP status line".into());
    }
    let status = partes.next().and_then(|s| s.parse::<u32>().ok()).ok_or("invalid HTTP status")?;
    let mensagem = partes.next().unwrap_or("").trim().to_string();
    let headers = linhas
        .filter_map(|l| l.split_once(':'))
        .map(|(n, v)| (n.trim().to_string(), v.trim().to_string()))
        .filter(|(n, _)| !n.is_empty())
        .collect();
    Ok(RespostaHttp { status, mensagem, headers, corpo: bruta[corte + 4..].to_vec() })
}

fn location(resp: &RespostaHttp) -> Option<&str> {
    resp.headers.iter().find(|(n, _)| n.eq_ignore_ascii_case("location")).map(|(_, v)| v.as_str())
}

/// Destino do `Location`; relativo é resolvido contra `atual`. Outros formatos são recusados.
fn destino_redirect(atual: &str, loc: &str) -> Option<String> {
    let minusc = loc.to_ascii_lowercase();
    if minusc.starts_with("http://") || minusc.starts_with("https://") {
        return Some(loc.to_string());
    }
    let esquema = if atual.starts_with("https://") { "https:" } else { "http:" };
    if let Some(resto) = loc.strip_prefix("//") {
        return Some(format!("{esquema}//{resto}"));
    }
    if loc.starts_with('/') {
        let inicio_host = atual.find("://")? + 3;
        let fim_origem = atual[inicio_host..].find(['/', '?', '#']).map_or(atual.len(), |p| inicio_host + p);
        return Some(format!("{}{loc}", &atual[..fim_origem]));
    }
    None
}

fn baixar_url(url: &str) -> Option<String> {
    if host_bloqueado(url) {
        return None;
    }
    let arq = dir_cache().map(|d| d.join(format!("{:016x}", hash_fnv(url))));
    let lido = |p: &PathBuf| std::fs::read(p).ok().map(|v| String::from_utf8_lossy(&v).into_owned());
    if let Some(p) = &arq {
        let fresco = std::fs::metadata(p)
            .and_then(|m| m.modified())
            .ok()
            .and_then(|t| SystemTime::now().duration_since(t).ok())
            .is_some_and(|idade| idade < VALIDADE_CACHE);
        if fresco {
            if let Some(t) = lido(p) {
                return Some(t);
            }
        }
    }
    let pedido = PedidoHttp { metodo: "GET".into(), url: url.into(), headers: vec![], corpo: vec![] };
    let baixado = pedir_com(&pedido, LIMITE_HTTPGET, TEMPO_HTTPGET, &resolver_sistema, &executar_curl)
        .ok()
        .filter(|r| (200..300).contains(&r.status))
        .map(|r| r.corpo);
    match baixado {
        Some(bytes) => {
            let texto = String::from_utf8_lossy(&bytes).into_owned();
            if let Some(p) = &arq {
                if let Some(d) = p.parent() {
                    let _ = std::fs::create_dir_all(d);
                }
                let _ = std::fs::write(p, &bytes);
            }
            Some(texto)
        }
        // rede caiu: cache velho ainda serve
        None => arq.as_ref().and_then(lido),
    }
}

#[cfg(test)]
mod testes {
    use super::*;
    use std::cell::{Cell, RefCell};

    fn rodar(src: &str, sites: &[(&str, &str)]) -> (String, bool, Vec<String>) {
        let mapa: HashMap<String, String> = sites.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect();
        let mut log = vec![];
        let (s, h) = resolver(src, &|u| mapa.get(u).cloned(), &|c| !c.contains("NAO_COMPILA"), &mut log);
        (s, h, log)
    }

    #[test]
    fn httpget_literal() {
        let (s, h, log) = rodar("local x = game:HttpGet(\"http://a/x\", true)", &[("http://a/x", "oi")]);
        assert!(h);
        assert_eq!(s, "local x = [[oi]]");
        assert!(log[0].contains("http://a/x") && log[0].contains("2 bytes"));
    }

    #[test]
    fn httpget_aspas_simples_e_longo() {
        let (s, _, _) = rodar("a=game:HttpGetAsync('u1') b=game:HttpGet([[u2]])", &[("u1", "A"), ("u2", "B")]);
        assert_eq!(s, "a=[[A]] b=[[B]]");
    }

    #[test]
    fn conteudo_com_colchetes_escolhe_nivel() {
        let (s, _, _) = rodar("x=game:HttpGet('u')", &[("u", "a]]b]=]c")]);
        assert_eq!(s, "x=[==[a]]b]=]c]==]");
    }

    #[test]
    fn conteudo_comecando_com_quebra() {
        let (s, _, _) = rodar("x=game:HttpGet('u')", &[("u", "\nz")]);
        assert_eq!(s, "x=[[\n\nz]]");
    }

    #[test]
    fn loadstring_de_literal() {
        let (s, h, _) = rodar("loadstring('print(1)')()", &[]);
        assert!(h);
        assert_eq!(s, "__luaria_fn(function(...) print(1)\nend)()");
    }

    #[test]
    fn loadstring_httpget_com_nome() {
        let (s, _, _) = rodar("loadstring(game:HttpGet(\"u\"), \"nome\")(1, 2)", &[("u", "return ...")]);
        assert_eq!(s, "__luaria_fn(function(...) return ...\nend)(1, 2)");
    }

    #[test]
    fn encadeado() {
        let (s, _, log) = rodar(
            "loadstring(game:HttpGet('l1'))()",
            &[("l1", "loadstring(game:HttpGet('l2'))()"), ("l2", "print('fim')")],
        );
        assert_eq!(s, "__luaria_fn(function(...) __luaria_fn(function(...) print('fim')\nend)()\nend)()");
        assert_eq!(log.len(), 2);
    }

    #[test]
    fn ignora_comentario_e_string() {
        let src = "--loadstring('a')\n--[[ game:HttpGet('u') ]]\nlocal s = \"loadstring('b')\" local t = [[game:HttpGet('u')]]";
        let (s, h, _) = rodar(src, &[("u", "X")]);
        assert!(!h);
        assert_eq!(s, src);
    }

    #[test]
    fn limite_de_profundidade() {
        let sites: Vec<(String, String)> =
            (0..8).map(|n| (format!("u{n}"), format!("loadstring(game:HttpGet('u{}'))()", n + 1))).collect();
        let refs: Vec<(&str, &str)> = sites.iter().map(|(a, b)| (a.as_str(), b.as_str())).collect();
        let (s, _, log) = rodar("loadstring(game:HttpGet('u0'))()", &refs);
        assert!(s.contains("loadstring(game:HttpGet('u4'))()"));
        assert!(!s.contains("u5"));
        assert!(log.iter().any(|l| l.contains("depth")));
    }

    #[test]
    fn url_que_falha_deixa_intacto() {
        let src = "loadstring(game:HttpGet('morta'))()";
        let (s, h, log) = rodar(src, &[]);
        assert!(!h);
        assert_eq!(s, src);
        assert_eq!(log.len(), 1);
    }

    #[test]
    fn argumento_dinamico_intacto() {
        let src = "loadstring(codigo)() local y = game:HttpGet(url) loadstring(game:HttpGet(base .. 'x'))()";
        let (s, h, _) = rodar(src, &[]);
        assert!(!h);
        assert_eq!(s, src);
    }

    #[test]
    fn ponto_e_virgula_evita_chamada_acidental() {
        let (s, _, _) = rodar("local a = f\nloadstring('x')()", &[]);
        assert_eq!(s, "local a = f\n__luaria_fn(function(...) x\nend)()");
        let (s, _, _) = rodar("return loadstring('x')", &[]);
        assert_eq!(s, "return __luaria_fn(function(...) x\nend)");
    }

    #[test]
    fn limpa_shebang_e_bom() {
        let (s, _, _) = rodar("loadstring(game:HttpGet('u'))()", &[("u", "\u{feff}#!x\nprint(1)")]);
        assert_eq!(s, "__luaria_fn(function(...) \nprint(1)\nend)()");
    }

    #[test]
    fn fonte_que_nao_compila_fica_intacta() {
        let src = "local f, e = loadstring('NAO_COMPILA')";
        let (s, h, log) = rodar(src, &[]);
        assert!(!h);
        assert_eq!(s, src);
        assert!(log[0].contains("does not compile"));
    }

    #[test]
    fn hosts_privados_bloqueados() {
        for u in ["http://localhost/x", "http://127.0.0.1/", "http://10.1.2.3/", "http://192.168.0.1:80/", "http://172.20.0.1/", "http://169.254.1.1/", "http://[::1]/", "ftp://a/b", "http://u@127.0.0.1/"] {
            assert!(host_bloqueado(u), "{u}");
        }
        assert!(!host_bloqueado("https://raw.githubusercontent.com/a/b"));
        assert!(!host_bloqueado("http://172.32.0.1/"));
        assert!(host_bloqueado("http://[::ffff:127.0.0.1]/") && host_bloqueado("http://100.64.0.1/"));
        assert_eq!(alvo("https://a.b:8080/x"), Some(("a.b".into(), 8080)));
        assert_eq!(alvo("http://[::1]/"), Some(("::1".into(), 80)));
    }

    fn publico(_: &str, _: u16) -> Vec<IpAddr> {
        vec![IpAddr::from([93, 184, 216, 34])]
    }

    fn pedido(url: &str) -> PedidoHttp {
        PedidoHttp { metodo: "GET".into(), url: url.into(), headers: vec![], corpo: vec![] }
    }

    fn saida(texto: &str) -> Saida {
        Saida { bruta: texto.as_bytes().to_vec(), truncada: false }
    }

    const REDIRECT: &str = "HTTP/1.1 302 Found\r\nLocation: http://b.teste/\r\n\r\n";

    #[test]
    fn pedido_para_ip_privado_nao_executa_curl() {
        let chamadas = Cell::new(0);
        let exec = |_: &[String], _: &[u8]| -> Option<Saida> {
            chamadas.set(chamadas.get() + 1);
            None
        };
        assert!(pedir_com(&pedido("http://10.0.0.1/x"), 1024, Duration::from_secs(5), &publico, &exec).is_err());
        assert_eq!(chamadas.get(), 0);
    }

    #[test]
    fn redirect_para_ip_privado_recusado() {
        let chamadas = Cell::new(0);
        let exec = |_: &[String], _: &[u8]| -> Option<Saida> {
            chamadas.set(chamadas.get() + 1);
            Some(saida("HTTP/1.1 302 Found\r\nLocation: http://127.0.0.1/admin\r\n\r\n"))
        };
        let erro = pedir_com(&pedido("http://a.teste/"), 1024, Duration::from_secs(5), &publico, &exec).unwrap_err();
        assert!(erro.contains("127.0.0.1"), "{erro}");
        assert_eq!(chamadas.get(), 1);
    }

    #[test]
    fn mais_de_cinco_redirects_recusado() {
        let chamadas = Cell::new(0);
        let exec = |_: &[String], _: &[u8]| -> Option<Saida> {
            chamadas.set(chamadas.get() + 1);
            Some(saida(REDIRECT))
        };
        assert!(pedir_com(&pedido("http://a.teste/"), 1024, Duration::from_secs(5), &publico, &exec).is_err());
        assert_eq!(chamadas.get(), 6);
    }

    #[test]
    fn cinco_redirects_ainda_passam() {
        let chamadas = Cell::new(0);
        let exec = |_: &[String], _: &[u8]| -> Option<Saida> {
            let n = chamadas.get();
            chamadas.set(n + 1);
            Some(saida(if n < 5 { REDIRECT } else { "HTTP/1.1 200 OK\r\n\r\nok" }))
        };
        let resp = pedir_com(&pedido("http://a.teste/"), 1024, Duration::from_secs(5), &publico, &exec).unwrap();
        assert_eq!((resp.status, resp.corpo.as_slice()), (200, b"ok".as_slice()));
    }

    #[test]
    fn redirect_303_vira_get_sem_corpo() {
        let vistos = RefCell::new(Vec::new());
        let exec = |args: &[String], corpo: &[u8]| -> Option<Saida> {
            vistos.borrow_mut().push((args.to_vec(), corpo.len()));
            Some(saida(if vistos.borrow().len() == 1 { "HTTP/1.1 303 See Other\r\nLocation: /x\r\n\r\n" } else { "HTTP/1.1 204 No Content\r\n\r\n" }))
        };
        let mut p = pedido("https://a.teste/");
        p.metodo = "POST".into();
        p.corpo = b"dados".to_vec();
        pedir_com(&p, 1024, Duration::from_secs(5), &publico, &exec).unwrap();
        let vistos = vistos.borrow();
        assert_eq!(vistos[0].1, 5);
        assert!(vistos[1].0.windows(2).any(|w| w[0] == "-X" && w[1] == "GET"));
        assert_eq!(vistos[1].1, 0);
        assert!(!vistos[1].0.iter().any(|a| a == "--data-binary"));
    }

    #[test]
    fn cabecalho_de_resposta_parseado() {
        let exec = |_: &[String], _: &[u8]| -> Option<Saida> {
            Some(saida("HTTP/2 201\r\nContent-Type: text/plain\r\nX-Vazio:\r\nsem dois pontos\r\n\r\nolá"))
        };
        let resp = pedir_com(&pedido("https://a.teste/"), 1024, Duration::from_secs(5), &publico, &exec).unwrap();
        assert_eq!(resp.status, 201);
        assert_eq!(resp.mensagem, "");
        assert_eq!(
            resp.headers,
            vec![("Content-Type".into(), "text/plain".into()), ("X-Vazio".into(), "".into())]
        );
        assert_eq!(resp.corpo, "olá".as_bytes());
    }

    #[test]
    fn mensagem_de_status_parseada() {
        let exec = |_: &[String], _: &[u8]| -> Option<Saida> { Some(saida("HTTP/1.1 404 Not Found\r\n\r\n")) };
        let resp = pedir_com(&pedido("https://a.teste/"), 1024, Duration::from_secs(5), &publico, &exec).unwrap();
        assert_eq!((resp.status, resp.mensagem.as_str()), (404, "Not Found"));
    }

    #[test]
    fn corpo_acima_do_limite_truncado_e_marcado() {
        let exec = |_: &[String], _: &[u8]| -> Option<Saida> { Some(saida("HTTP/1.1 200 OK\r\n\r\nabcdef")) };
        let resp = pedir_com(&pedido("https://a.teste/"), 3, Duration::from_secs(5), &publico, &exec).unwrap();
        assert_eq!((resp.status, resp.corpo.as_slice()), (0, b"abc".as_slice()));
        assert!(resp.mensagem.contains("truncated"));
    }

    #[test]
    fn curl_que_parou_no_limite_marca_truncada() {
        let exec = |_: &[String], _: &[u8]| -> Option<Saida> {
            Some(Saida { bruta: b"HTTP/1.1 200 OK\r\n\r\nab".to_vec(), truncada: true })
        };
        let resp = pedir_com(&pedido("https://a.teste/"), 1024, Duration::from_secs(5), &publico, &exec).unwrap();
        assert_eq!(resp.status, 0);
    }

    #[test]
    fn args_do_curl_sem_seguir_redirect_e_com_ip_fixo() {
        let args = montar_args("POST", "https://a.teste/x", "a.teste:443:93.184.216.34", 30, 1000, &[("X-T".into(), "".into())], true);
        assert!(!args.iter().any(|a| a == "-L"));
        assert!(args.windows(2).any(|w| w[0] == "--resolve" && w[1] == "a.teste:443:93.184.216.34"));
        assert!(args.windows(2).any(|w| w[0] == "--proto" && w[1] == "=http,https"));
        assert!(args.windows(2).any(|w| w[0] == "-H" && w[1] == "User-Agent: Roblox/WinInet"));
        assert!(args.windows(2).any(|w| w[0] == "-H" && w[1] == "X-T;"));
        assert!(args.ends_with(&["--".to_string(), "https://a.teste/x".to_string()]));
    }

    #[test]
    fn destino_de_redirect_relativo_e_absoluto() {
        assert_eq!(destino_redirect("https://a.teste:8443/y?q", "/x").as_deref(), Some("https://a.teste:8443/x"));
        assert_eq!(destino_redirect("https://a.teste/y", "//b.teste/z").as_deref(), Some("https://b.teste/z"));
        assert_eq!(destino_redirect("https://a.teste/", "http://c.teste/").as_deref(), Some("http://c.teste/"));
        assert_eq!(destino_redirect("https://a.teste/", "outro"), None);
    }

    #[test]
    fn ip_v4_tem_prioridade_no_resolver() {
        let v6: IpAddr = "2606:4700::1".parse().unwrap();
        let v4 = IpAddr::from([93, 184, 216, 34]);
        let (_, _, ip) = ip_publico("https://a.teste/", &|_, _| vec![v6, v4]).unwrap();
        assert_eq!(ip, v4);
    }
}
