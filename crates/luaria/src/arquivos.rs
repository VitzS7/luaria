//! Funções de arquivo da UNC (`writefile`, `readfile`...) sem código nativo.
//!
//! Dentro do jogo não existe `io`. Host -> jogo: `instantaneo()` vira a primeira
//! linha do chunk com o conteúdo da pasta. Jogo -> host: o prelude dá `print` em
//! linhas `LUARIA_FS ...` e a UI, que já faz tail do log, passa cada uma por `aplicar`.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

pub const MAX_ARQUIVO: u64 = 256 * 1024;
// O chunk inteiro (bytecode com o base64 como constante) tem que caber em
// TAM_REGIAO (2 MB) no cliente, então o total fica abaixo do 1 MB do design.
pub const MAX_TOTAL: u64 = 512 * 1024;
pub const MAX_ESCRITA: usize = 4 * 1024 * 1024;
const MAX_NOME: usize = 200;
// 8 x 4 MB em base64 no pior caso: linha forjada não estoura a memória da UI
const MAX_PENDENTES: usize = 8;
/// Teto da pasta inteira: print de script do jogo não enche o disco.
const MAX_PASTA: u64 = 64 * 1024 * 1024;
const MAX_CLIP: usize = 192 * 1024;
const MAX_PARTES: usize = 4096;
const MAX_ENTRADAS: usize = 2000;
const MAX_PROFUNDIDADE: usize = 16;

/// Segredo compartilhado entre o host e o chunk. Qualquer script do jogo pode dar
/// `print("LUARIA_FS ...")`; sem o token essa linha não vira escrita no disco nem
/// clipboard. Fica em arquivo para CLI e UI (processos diferentes) concordarem.
pub fn token() -> &'static str {
    static TOKEN: std::sync::OnceLock<String> = std::sync::OnceLock::new();
    TOKEN.get_or_init(|| {
        let home = std::env::var("HOME").unwrap_or_default();
        let arq = PathBuf::from(home).join(".config/luaria/.token");
        if let Ok(t) = fs::read_to_string(&arq) {
            let t = t.trim();
            if t.len() == 32 && t.bytes().all(|c| c.is_ascii_hexdigit()) {
                return t.to_string();
            }
        }
        let mut bytes = [0u8; 16];
        if let Ok(mut f) = fs::File::open("/dev/urandom") {
            let _ = std::io::Read::read_exact(&mut f, &mut bytes);
        }
        let t: String = bytes.iter().map(|b| format!("{b:02x}")).collect();
        if let Some(pai) = arq.parent() {
            let _ = fs::create_dir_all(pai);
        }
        use std::os::unix::fs::OpenOptionsExt;
        let gravado = fs::OpenOptions::new().write(true).create(true).truncate(true).mode(0o600).open(&arq);
        if let Ok(mut f) = gravado {
            let _ = f.write_all(t.as_bytes());
        }
        t
    })
}

pub fn raiz_padrao() -> PathBuf {
    let home = std::env::var("HOME").unwrap_or_default();
    PathBuf::from(home).join(".config/luaria/Workspace")
}

/// Caminho relativo vindo do script -> caminho real, sempre dentro da raiz.
pub fn resolver(raiz: &Path, rel: &str) -> Option<PathBuf> {
    if rel.is_empty() || rel.len() > MAX_NOME || rel.contains(['\\', '\0']) {
        return None;
    }
    let mut destino = raiz.to_path_buf();
    for parte in rel.split('/') {
        // segmento vazio cobre absoluto ("/x"), "a//b" e barra final
        if parte.is_empty() || parte == "." || parte == ".." {
            return None;
        }
        destino.push(parte);
    }
    // symlink dentro da raiz apontando pra fora: confere o ancestral existente
    let raiz_real = fs::canonicalize(raiz).ok()?;
    let mut ancestral = destino.as_path();
    while !ancestral.exists() && ancestral.symlink_metadata().is_err() {
        ancestral = ancestral.parent()?;
    }
    if !fs::canonicalize(ancestral).ok()?.starts_with(&raiz_real) {
        return None;
    }
    Some(destino)
}

const ALFABETO: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

pub fn base64_codificar(dados: &[u8]) -> String {
    let mut saida = String::with_capacity(dados.len().div_ceil(3) * 4);
    for bloco in dados.chunks(3) {
        let n = (bloco[0] as u32) << 16
            | (*bloco.get(1).unwrap_or(&0) as u32) << 8
            | *bloco.get(2).unwrap_or(&0) as u32;
        for i in 0..4 {
            if i <= bloco.len() {
                saida.push(ALFABETO[(n >> (18 - 6 * i) & 63) as usize] as char);
            } else {
                saida.push('=');
            }
        }
    }
    saida
}

pub fn base64_decodificar(texto: &str) -> Option<Vec<u8>> {
    let bytes = texto.trim_end_matches('=').as_bytes();
    if bytes.len() % 4 == 1 {
        return None;
    }
    let mut saida = Vec::with_capacity(bytes.len() / 4 * 3 + 2);
    for bloco in bytes.chunks(4) {
        let mut n = 0u32;
        for &c in bloco {
            let v = ALFABETO.iter().position(|&a| a == c)? as u32;
            n = n << 6 | v;
        }
        n <<= 6 * (4 - bloco.len()) as u32;
        saida.push((n >> 16) as u8);
        if bloco.len() > 2 {
            saida.push((n >> 8) as u8);
        }
        if bloco.len() > 3 {
            saida.push(n as u8);
        }
    }
    Some(saida)
}

fn literal_lua(s: &str) -> String {
    let mut saida = String::from("\"");
    for c in s.chars() {
        match c {
            '"' => saida.push_str("\\\""),
            '\\' => saida.push_str("\\\\"),
            c if (c as u32) < 32 || c as u32 == 127 => saida.push_str(&format!("\\{:03}", c as u32)),
            c => saida.push(c),
        }
    }
    saida.push('"');
    saida
}

#[derive(Default)]
struct Coleta {
    arquivos: Vec<(String, String)>,
    pastas: Vec<String>,
    total: u64,
    ignorados: u32,
}

fn coletar(dir: &Path, prefixo: &str, nivel: usize, c: &mut Coleta) {
    let Ok(lista) = fs::read_dir(dir) else { return };
    let mut itens: Vec<_> = lista.flatten().collect();
    itens.sort_by_key(|e| e.file_name());
    for item in itens {
        let Some(nome) = item.file_name().to_str().map(str::to_string) else {
            c.ignorados += 1;
            continue;
        };
        let rel = if prefixo.is_empty() { nome.clone() } else { format!("{prefixo}/{nome}") };
        let Ok(tipo) = item.file_type() else { continue };
        // symlink fica de fora: pode apontar pra qualquer lugar do disco
        if tipo.is_symlink() || nome.contains('\\') || rel.len() > MAX_NOME {
            c.ignorados += 1;
        } else if tipo.is_dir() {
            if nivel >= MAX_PROFUNDIDADE {
                c.ignorados += 1;
                continue;
            }
            c.pastas.push(rel.clone());
            coletar(&item.path(), &rel, nivel + 1, c);
        } else if tipo.is_file() {
            let tam = item.metadata().map(|m| m.len()).unwrap_or(u64::MAX);
            if tam > MAX_ARQUIVO || c.total + tam > MAX_TOTAL || c.arquivos.len() >= MAX_ENTRADAS {
                c.ignorados += 1;
                continue;
            }
            let Ok(dados) = fs::read(item.path()) else {
                c.ignorados += 1;
                continue;
            };
            c.total += dados.len() as u64;
            c.arquivos.push((rel, base64_codificar(&dados)));
        }
    }
}

/// Uma única linha de Lua com o estado da pasta, para ir antes do prelude.
pub fn instantaneo_em(raiz: &Path, token: &str) -> String {
    let mut c = Coleta::default();
    coletar(raiz, "", 0, &mut c);
    if c.arquivos.is_empty() && c.pastas.is_empty() && c.ignorados == 0 {
        return format!("local __luaria_arquivos = {{token=\"{token}\"}}");
    }
    let arquivos: Vec<String> =
        c.arquivos.iter().map(|(k, v)| format!("[{}]=\"{v}\"", literal_lua(k))).collect();
    let pastas: Vec<String> = c.pastas.iter().map(|k| format!("[{}]=true", literal_lua(k))).collect();
    format!(
        "local __luaria_arquivos = {{token=\"{token}\",arquivos={{{}}},pastas={{{}}},ignorados={}}}",
        arquivos.join(","),
        pastas.join(","),
        c.ignorados
    )
}

pub fn instantaneo() -> String {
    instantaneo_em(&raiz_padrao(), token())
}

fn tamanho_pasta(dir: &Path, nivel: usize) -> u64 {
    let Ok(lista) = fs::read_dir(dir) else { return 0 };
    lista
        .flatten()
        .map(|e| match e.file_type() {
            Ok(t) if t.is_dir() && nivel < MAX_PROFUNDIDADE => tamanho_pasta(&e.path(), nivel + 1),
            Ok(t) if t.is_file() => e.metadata().map(|m| m.len()).unwrap_or(0),
            _ => 0,
        })
        .sum()
}

struct Pendente {
    id: String,
    op: char,
    caminho: String,
    partes: Vec<Option<String>>,
    recebidas: usize,
    tamanho: usize,
}

#[derive(Default)]
pub struct Pendentes(Vec<Pendente>);

pub type Resultado = Result<(), String>;

fn gravar_atomico(destino: &Path, dados: &[u8]) -> std::io::Result<()> {
    let pai = destino.parent().unwrap_or(Path::new("."));
    fs::create_dir_all(pai)?;
    let nome = destino.file_name().and_then(|n| n.to_str()).unwrap_or("arquivo");
    let temporario = pai.join(format!(".{nome}.luaria-tmp-{}", std::process::id()));
    let gravou = fs::File::create(&temporario).and_then(|mut f| f.write_all(dados).and_then(|_| f.sync_all()));
    if let Err(e) = gravou.and_then(|_| fs::rename(&temporario, destino)) {
        let _ = fs::remove_file(&temporario);
        return Err(e);
    }
    Ok(())
}

fn executar(raiz: &Path, op: char, caminho_b64: &str, dados: &[u8]) -> Resultado {
    let rel = base64_decodificar(caminho_b64)
        .and_then(|b| String::from_utf8(b).ok())
        .ok_or("invalid base64 path")?;
    fs::create_dir_all(raiz).map_err(|e| format!("files folder: {e}"))?;
    let destino = resolver(raiz, &rel).ok_or_else(|| format!("caminho rejeitado: {rel:?}"))?;
    let io = |e: std::io::Error| format!("{rel}: {e}");
    if matches!(op, 'w' | 'a') {
        let atual = if op == 'w' { fs::metadata(&destino).map(|m| m.len()).unwrap_or(0) } else { 0 };
        if tamanho_pasta(raiz, 0).saturating_sub(atual) + dados.len() as u64 > MAX_PASTA {
            return Err(format!("{rel}: files folder exceeded {} MB", MAX_PASTA >> 20));
        }
    }
    match op {
        'w' => gravar_atomico(&destino, dados).map_err(io),
        'a' => {
            if let Some(pai) = destino.parent() {
                fs::create_dir_all(pai).map_err(io)?;
            }
            let mut f = fs::OpenOptions::new().create(true).append(true).open(&destino).map_err(io)?;
            f.write_all(dados).map_err(io)
        }
        'm' => fs::create_dir_all(&destino).map_err(io),
        'd' => match fs::remove_file(&destino) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(io(e)),
            _ => Ok(()),
        },
        'D' => match fs::remove_dir_all(&destino) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(io(e)),
            _ => Ok(()),
        },
        _ => Err(format!("unknown operation: {op}")),
    }
}

/// `None` = a linha não é da ponte. `Some(Ok)` também vale para parte intermediária.
pub fn aplicar_em(raiz: &Path, pend: &mut Pendentes, token: &str, linha: &str) -> Option<Resultado> {
    let resto = &linha[linha.find("LUARIA_FS ")? + "LUARIA_FS ".len()..];
    Some(processar_linha(raiz, pend, resto, token))
}

fn processar_linha(raiz: &Path, pend: &mut Pendentes, resto: &str, token: &str) -> Resultado {
    let mut campos = resto.split_whitespace();
    let (Some(op), Some(id), Some(progresso), Some(caminho)) =
        (campos.next(), campos.next(), campos.next(), campos.next())
    else {
        return Err("incomplete LUARIA_FS line".into());
    };
    if !id.starts_with(token) {
        return Err("LUARIA_FS line without this session token (print from another script?)".into());
    }
    let dados = campos.next().unwrap_or("");
    let op = op.chars().next().ok_or("empty operation")?;
    let (i, n) = progresso
        .split_once('/')
        .and_then(|(i, n)| Some((i.parse::<usize>().ok()?, n.parse::<usize>().ok()?)))
        .ok_or("invalid progress")?;
    if n == 0 || n > MAX_PARTES || i == 0 || i > n {
        return Err(format!("progress out of range: {progresso}"));
    }

    if n == 1 {
        let bytes = base64_decodificar(dados).ok_or("invalid base64 data")?;
        if bytes.len() > MAX_ESCRITA {
            return Err("write above 4 MB".into());
        }
        return executar(raiz, op, caminho, &bytes);
    }

    let pos = match pend.0.iter().position(|p| p.id == id) {
        Some(p) => p,
        None => {
            // incompleto antigo some primeiro: o id novo é o que importa
            if pend.0.len() >= MAX_PENDENTES {
                pend.0.remove(0);
            }
            pend.0.push(Pendente {
                id: id.to_string(),
                op,
                caminho: caminho.to_string(),
                partes: vec![None; n],
                recebidas: 0,
                tamanho: 0,
            });
            pend.0.len() - 1
        }
    };
    let p = &mut pend.0[pos];
    if p.partes.len() != n || p.op != op || p.caminho != caminho {
        pend.0.remove(pos);
        return Err("partes inconsistentes".into());
    }
    if p.partes[i - 1].is_none() {
        p.tamanho += dados.len();
        p.recebidas += 1;
        p.partes[i - 1] = Some(dados.to_string());
    }
    if p.tamanho / 4 * 3 > MAX_ESCRITA {
        pend.0.remove(pos);
        return Err("write above 4 MB".into());
    }
    if p.recebidas < n {
        return Ok(());
    }
    let p = pend.0.remove(pos);
    let junto: String = p.partes.into_iter().flatten().collect();
    let bytes = base64_decodificar(&junto).ok_or("invalid base64 data")?;
    executar(raiz, p.op, &p.caminho, &bytes)
}

static PENDENTES: Mutex<Option<Pendentes>> = Mutex::new(None);

pub fn aplicar(linha: &str) -> Option<Resultado> {
    let mut guarda = PENDENTES.lock().unwrap_or_else(|e| e.into_inner());
    aplicar_em(&raiz_padrao(), guarda.get_or_insert_with(Pendentes::default), token(), linha)
}

/// `LUARIA_CLIP <token> <base64>` do `setclipboard`: devolve o texto a copiar.
pub fn texto_clipboard(linha: &str, token: &str) -> Option<Result<String, String>> {
    let resto = &linha[linha.find("LUARIA_CLIP ")? + "LUARIA_CLIP ".len()..];
    let mut campos = resto.split_whitespace();
    if campos.next() != Some(token) {
        return Some(Err("LUARIA_CLIP without this session token (print from another script?)".into()));
    }
    let dados = campos.next().unwrap_or("");
    if dados.len() > MAX_CLIP / 3 * 4 + 4 {
        return Some(Err("clipboard above 192 KB".into()));
    }
    Some(
        base64_decodificar(dados)
            .and_then(|b| String::from_utf8(b).ok())
            .ok_or_else(|| "invalid base64 clipboard".to_string()),
    )
}


#[cfg(test)]
mod testes {
    use super::*;

    struct Temp(PathBuf);
    impl Temp {
        fn novo(nome: &str) -> Self {
            let d = std::env::temp_dir().join(format!("luaria-teste-{}-{nome}", std::process::id()));
            let _ = fs::remove_dir_all(&d);
            fs::create_dir_all(&d).unwrap();
            Temp(d)
        }
    }
    impl Drop for Temp {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn linha(op: char, id: &str, i: usize, n: usize, caminho: &str, dados: &[u8]) -> String {
        format!(
            "[FLog::CreatorOutput] LUARIA_FS {op} {id} {i}/{n} {} {}",
            base64_codificar(caminho.as_bytes()),
            base64_codificar(dados)
        )
    }

    #[test]
    fn base64_ida_e_volta() {
        for dados in [&b""[..], b"a", b"ab", b"abc", b"abcd", &[0, 255, 128, 7, 9]] {
            assert_eq!(base64_decodificar(&base64_codificar(dados)).unwrap(), dados);
        }
        assert_eq!(base64_codificar(b"Man"), "TWFu");
        assert!(base64_decodificar("a$bc").is_none());
    }

    #[test]
    fn resolver_rejeita_fuga() {
        let t = Temp::novo("resolver");
        for ruim in ["", "/etc/passwd", "../x", "a/../b", "a\\b", "a\0b", "a//b", "a/", ".", &"x".repeat(201)] {
            assert!(resolver(&t.0, ruim).is_none(), "{ruim:?}");
        }
        assert_eq!(resolver(&t.0, "a/b.txt").unwrap(), t.0.join("a/b.txt"));
    }

    #[test]
    fn resolver_rejeita_symlink_para_fora() {
        let t = Temp::novo("symlink");
        let fora = Temp::novo("symlink-fora");
        std::os::unix::fs::symlink(&fora.0, t.0.join("link")).unwrap();
        assert!(resolver(&t.0, "link/x.txt").is_none());
    }

    #[test]
    fn escreve_e_anexa() {
        let t = Temp::novo("escreve");
        let mut p = Pendentes::default();
        assert!(aplicar_em(&t.0, &mut p, "", &linha('w', "1", 1, 1, "a/b.txt", b"ola")).unwrap().is_ok());
        assert!(aplicar_em(&t.0, &mut p, "", &linha('a', "2", 1, 1, "a/b.txt", b" mundo")).unwrap().is_ok());
        assert_eq!(fs::read_to_string(t.0.join("a/b.txt")).unwrap(), "ola mundo");
        assert!(aplicar_em(&t.0, &mut p, "", &linha('m', "3", 1, 1, "x/y", b"")).unwrap().is_ok());
        assert!(t.0.join("x/y").is_dir());
        assert!(aplicar_em(&t.0, &mut p, "", &linha('D', "4", 1, 1, "x", b"")).unwrap().is_ok());
        assert!(!t.0.join("x").exists());
        assert!(aplicar_em(&t.0, &mut p, "", &linha('d', "5", 1, 1, "a/b.txt", b"")).unwrap().is_ok());
        assert!(!t.0.join("a/b.txt").exists());
        assert!(aplicar_em(&t.0, &mut p, "", "[FLog::CreatorOutput] oi").is_none());
    }

    #[test]
    fn partes_fora_de_ordem() {
        let t = Temp::novo("partes");
        let mut p = Pendentes::default();
        let caminho = base64_codificar(b"g.bin");
        let parte = |i: usize, dados: &str| format!("LUARIA_FS w ID {i}/3 {caminho} {dados}");
        // 12 bytes de entrada cortados em 3 blocos de base64 alinhados em 4
        let todo = base64_codificar(b"abcdefghi");
        assert!(aplicar_em(&t.0, &mut p, "", &parte(3, &todo[8..])).unwrap().is_ok());
        assert!(aplicar_em(&t.0, &mut p, "", &parte(1, &todo[..4])).unwrap().is_ok());
        assert!(!t.0.join("g.bin").exists());
        assert!(aplicar_em(&t.0, &mut p, "", &parte(2, &todo[4..8])).unwrap().is_ok());
        assert_eq!(fs::read(t.0.join("g.bin")).unwrap(), b"abcdefghi");
        assert!(p.0.is_empty());
    }

    #[test]
    fn pendentes_tem_teto() {
        let t = Temp::novo("teto");
        let mut p = Pendentes::default();
        for i in 0..100 {
            let l = format!("LUARIA_FS w id{i} 1/2 {} AAAA", base64_codificar(b"x"));
            assert!(aplicar_em(&t.0, &mut p, "", &l).unwrap().is_ok());
        }
        assert_eq!(p.0.len(), MAX_PENDENTES);
    }

    #[test]
    fn rejeita_caminho_invalido_e_sem_gravar() {
        let t = Temp::novo("invalido");
        let mut p = Pendentes::default();
        let r = aplicar_em(&t.0, &mut p, "", &linha('w', "1", 1, 1, "../fora.txt", b"x")).unwrap();
        assert!(r.is_err());
        assert!(!t.0.parent().unwrap().join("fora.txt").exists());
    }

    #[test]
    fn gravacao_atomica_nao_deixa_temporario() {
        let t = Temp::novo("atomico");
        let mut p = Pendentes::default();
        aplicar_em(&t.0, &mut p, "", &linha('w', "1", 1, 1, "f.txt", b"um")).unwrap().unwrap();
        aplicar_em(&t.0, &mut p, "", &linha('w', "2", 1, 1, "f.txt", b"dois")).unwrap().unwrap();
        assert_eq!(fs::read_to_string(t.0.join("f.txt")).unwrap(), "dois");
        let nomes: Vec<_> = fs::read_dir(&t.0).unwrap().flatten().map(|e| e.file_name()).collect();
        assert_eq!(nomes.len(), 1);
    }

    #[test]
    fn exige_token_e_clipboard() {
        let t = Temp::novo("token");
        let mut p = Pendentes::default();
        let forjada = linha('w', "zz1", 1, 1, "f.txt", b"x");
        assert!(aplicar_em(&t.0, &mut p, "abc", &forjada).unwrap().is_err());
        assert!(!t.0.join("f.txt").exists());
        let boa = linha('w', "abc1", 1, 1, "f.txt", b"x");
        assert!(aplicar_em(&t.0, &mut p, "abc", &boa).unwrap().is_ok());
        let clip = format!("[FLog::CreatorOutput] LUARIA_CLIP abc {}", base64_codificar("olá".as_bytes()));
        assert_eq!(texto_clipboard(&clip, "abc").unwrap().unwrap(), "olá");
        assert!(texto_clipboard(&clip, "xyz").unwrap().is_err());
        assert!(texto_clipboard("print normal", "abc").is_none());
    }

    #[test]
    fn pasta_tem_teto() {
        let t = Temp::novo("teto-pasta");
        fs::write(t.0.join("grande.bin"), vec![0u8; (MAX_PASTA - 10) as usize]).unwrap();
        let mut p = Pendentes::default();
        assert!(aplicar_em(&t.0, &mut p, "", &linha('w', "1", 1, 1, "f.txt", &[1u8; 64])).unwrap().is_err());
        // regravar o próprio arquivo grande com menos bytes cabe
        assert!(aplicar_em(&t.0, &mut p, "", &linha('w', "2", 1, 1, "grande.bin", b"x")).unwrap().is_ok());
    }

    #[test]
    fn instantaneo_vazio_e_uma_linha() {
        let t = Temp::novo("inst-vazio");
        assert_eq!(instantaneo_em(&t.0, "ab"), "local __luaria_arquivos = {token=\"ab\"}");
    }

    #[test]
    fn instantaneo_respeita_limites() {
        let t = Temp::novo("inst-limite");
        fs::create_dir_all(t.0.join("pasta")).unwrap();
        fs::write(t.0.join("pasta/a\"b\n.txt"), b"oi").unwrap();
        fs::write(t.0.join("grande.bin"), vec![0u8; MAX_ARQUIVO as usize + 1]).unwrap();
        let fatia = vec![1u8; 200 * 1024];
        for n in 0..3 {
            fs::write(t.0.join(format!("f{n}.bin")), &fatia).unwrap();
        }
        let s = instantaneo_em(&t.0, "ab");
        assert_eq!(s.lines().count(), 1);
        assert!(s.starts_with("local __luaria_arquivos = {token=\"ab\",arquivos={"));
        assert!(s.contains("[\"pasta\"]=true"));
        assert!(s.contains("b\\010.txt"));
        assert!(!s.contains("grande.bin"));
        // 3 de 200 KB não cabem em 512 KB: o terceiro e o grande são ignorados
        assert!(s.ends_with("ignorados=2}"), "{}", &s[s.len() - 30..]);
    }
}
