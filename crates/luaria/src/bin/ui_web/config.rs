//! Preferências da UI e dados de uso guardados em `~/.config/luaria/`.
//!
//! O `ui.json` é a única fonte do tema: a página só aplica o que chega daqui.
//! Fundo e atividade ficam na mesma pasta pelo mesmo motivo — é o lugar que o
//! usuário já conhece para o que é dele.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use serde::Serialize;
use serde_json::{json, Map, Value};

use super::estado::pasta_config;

/// Dois Ctrl+Enter seguidos não podem perder uma contagem na escrita concorrente.
static TRAVA_ATIVIDADE: Mutex<()> = Mutex::new(());

/// Fundo enviado pelo usuário: o tamanho máximo fica aqui e no limite HTTP.
pub const LIMITE_FUNDO: usize = 12 << 20;

const SKINS: [&str; 2] = ["vidro", "aero"];
const MODOS: [&str; 3] = ["dark", "light", "amoled"];
const NAVS: [&str; 2] = ["centro", "esquerda"];
const DENSIDADES: [&str; 2] = ["comfortable", "compact"];
const FONTES: [&str; 5] = [
    "Fira Code",
    "IBM Plex Mono",
    "Noto Sans Mono",
    "Liberation Mono",
    "DejaVu Sans Mono",
];
const MAX_TEMAS: usize = 20;

pub fn tema_padrao() -> Value {
    json!({
        "preset": "vidro-escuro",
        "skin": "vidro",
        "accent": "#8CC4D4",
        "theme": "dark",
        "editor_font_size": 13,
        "editor_font_family": "IBM Plex Mono",
        "density": "comfortable",
        "panel_opacity": 0.7,
        "panel_blur": 14,
        "panel_radius": 12,
        "nav": "centro",
        "wallpaper": "",
        "wallpaper_blur": 4,
        "wallpaper_forca": 70,
        "window_opacity": 1.0,
        "confirmar_fechar": true,
        "confirmar_excluir": true,
        "nome_usuario": "",
        "temas_personalizados": [],
        "tokens": {
            "window": "#1A2130",
            "panel": "#2A3142",
            "surface": "#363E52",
            "text": "#EEF3F9",
            "muted": "#AEB9C9",
            "border": "#4B5670"
        }
    })
}

fn mesclar(base: &mut Value, novo: &Value) {
    match (base, novo) {
        (Value::Object(b), Value::Object(n)) => {
            for (k, v) in n {
                match b.get_mut(k) {
                    Some(antigo) => mesclar(antigo, v),
                    None => {
                        b.insert(k.clone(), v.clone());
                    }
                }
            }
        }
        (b, n) => *b = n.clone(),
    }
}

/// Lê o `ui.json`, completa com o padrão e normaliza cada campo.
pub fn ler_tema() -> Value {
    let mut tema = tema_padrao();
    if let Ok(txt) = std::fs::read_to_string(pasta_config().join("ui.json")) {
        if let Ok(json) = serde_json::from_str::<Value>(&txt) {
            mesclar(&mut tema, &json);
        }
    }
    normalizar_tema(tema)
}

/// Atualização parcial não pode apagar as chaves que não vieram no corpo.
pub fn salvar_tema(json: &Value) -> Result<Value, String> {
    if !json.is_object() {
        return Err("the theme must be a JSON object".into());
    }
    let mut base = ler_tema();
    mesclar(&mut base, json);
    let tema = normalizar_tema(base);
    let destino = pasta_config();
    std::fs::create_dir_all(&destino).map_err(|e| format!("config: {e}"))?;
    let tmp = destino.join("ui.json.tmp");
    let texto = serde_json::to_string_pretty(&tema).map_err(|e| e.to_string())?;
    // Escreve em tmp e troca: um corte no meio não deixa o ui.json pela metade.
    std::fs::write(&tmp, texto + "\n").map_err(|e| format!("write: {e}"))?;
    std::fs::rename(&tmp, destino.join("ui.json")).map_err(|e| format!("rename: {e}"))?;
    Ok(tema)
}

fn texto_de(v: &Value, max: usize, padrao: &str) -> String {
    match v.as_str().map(str::trim) {
        Some(s) if !s.is_empty() => s.chars().take(max).collect(),
        _ => padrao.to_string(),
    }
}

fn escolha(v: &Value, opcoes: &[&str], padrao: &str) -> String {
    v.as_str()
        .filter(|s| opcoes.contains(s))
        .unwrap_or(padrao)
        .to_string()
}

fn inteiro(v: &Value, min: i64, max: i64, padrao: i64) -> i64 {
    v.as_i64().unwrap_or(padrao).clamp(min, max)
}

fn decimal(v: &Value, min: f64, max: f64, padrao: f64) -> f64 {
    let n = v.as_f64().unwrap_or(padrao).clamp(min, max);
    (n * 100.0).round() / 100.0
}

fn cor_valida(v: &Value) -> Option<String> {
    let s = v.as_str()?;
    if s.len() == 7 && s.starts_with('#') && s[1..].chars().all(|c| c.is_ascii_hexdigit()) {
        Some(s.to_ascii_uppercase())
    } else {
        None
    }
}

fn cor(v: &Value, padrao: &str) -> String {
    cor_valida(v).unwrap_or_else(|| padrao.to_string())
}

/// `""` = padrão embutido, `"arquivo"` = fundo enviado, ou URL http(s).
fn fundo_valido(v: &Value) -> String {
    let Some(s) = v.as_str().map(str::trim) else {
        return String::new();
    };
    let url = (s.starts_with("https://") || s.starts_with("http://"))
        && s.len() <= 2048
        && !s.contains(char::is_whitespace)
        && !s.contains(['"', '\'', '<', '>']);
    if s == "arquivo" || url {
        s.to_string()
    } else {
        String::new()
    }
}

/// Cores do editor e da sintaxe: opcionais; ausentes usam o que o modo define.
const TOKENS_EDITOR: [&str; 8] = [
    "editor_fundo",
    "sin_keyword",
    "sin_string",
    "sin_number",
    "sin_comment",
    "sin_funcao",
    "sin_builtin",
    "sin_operador",
];

fn tokens_de(v: &Value, padrao: &Value) -> Value {
    let mut saida = Map::new();
    for chave in ["window", "panel", "surface", "text", "muted", "border"] {
        let fallback = padrao[chave].as_str().unwrap_or("#000000");
        saida.insert(chave.into(), cor(&v[chave], fallback).into());
    }
    for chave in TOKENS_EDITOR {
        if let Some(valor) = cor_valida(&v[chave]) {
            saida.insert(chave.into(), valor.into());
        }
    }
    Value::Object(saida)
}

fn booleano(v: &Value, padrao: bool) -> bool {
    v.as_bool().unwrap_or(padrao)
}

/// `null` = o usuário nunca escolheu; o front então decide pelo prefers-reduced-motion.
fn efeitos_de(v: &Value) -> Value {
    v.as_bool().map_or(Value::Null, Value::Bool)
}

fn temas_de(v: &Value, padrao: &Value) -> Value {
    let Some(lista) = v.as_array() else {
        return json!([]);
    };
    let itens: Vec<Value> = lista
        .iter()
        .filter(|t| t.is_object())
        .take(MAX_TEMAS)
        .map(|t| {
            json!({
                "nome": texto_de(&t["nome"], 40, "Tema"),
                "accent": cor(&t["accent"], "#8CC4D4"),
                "theme": escolha(&t["theme"], &MODOS, "dark"),
                "tokens": tokens_de(&t["tokens"], &padrao["tokens"]),
            })
        })
        .collect();
    Value::Array(itens)
}

/// Clampa e valida cada campo. Campo que não passa cai no padrão em vez de
/// derrubar a UI inteira.
pub fn normalizar_tema(tema: Value) -> Value {
    let mut t = if tema.is_object() { tema } else { tema_padrao() };
    let padrao = tema_padrao();
    for (novo, antigo) in [
        ("accent", "acento"),
        ("theme", "modo"),
        ("editor_font_size", "fonte_editor"),
        ("density", "densidade"),
    ] {
        if let Some(v) = t.get(antigo).cloned() {
            t[novo] = v;
        }
    }

    json!({
        "preset": texto_de(&t["preset"], 40, "vidro-escuro"),
        "skin": escolha(&t["skin"], &SKINS, "vidro"),
        "accent": cor(&t["accent"], "#8CC4D4"),
        "theme": escolha(&t["theme"], &MODOS, "dark"),
        "editor_font_size": inteiro(&t["editor_font_size"], 10, 22, 13),
        "editor_font_family": escolha(&t["editor_font_family"], &FONTES, "IBM Plex Mono"),
        "density": escolha(&t["density"], &DENSIDADES, "comfortable"),
        "panel_opacity": decimal(&t["panel_opacity"], 0.35, 1.0, 0.7),
        "panel_blur": inteiro(&t["panel_blur"], 0, 40, 14),
        "panel_radius": inteiro(&t["panel_radius"], 0, 24, 12),
        "nav": escolha(&t["nav"], &NAVS, "centro"),
        "wallpaper": fundo_valido(&t["wallpaper"]),
        "wallpaper_blur": inteiro(&t["wallpaper_blur"], 0, 40, 4),
        "wallpaper_forca": inteiro(&t["wallpaper_forca"], 0, 100, 70),
        "nome_usuario": texto_de(&t["nome_usuario"], 32, ""),
        "temas_personalizados": temas_de(&t["temas_personalizados"], &padrao),
        "tokens": tokens_de(&t["tokens"], &padrao["tokens"]),
        "window_opacity": decimal(&t["window_opacity"], 0.2, 1.0, 1.0),
        "confirmar_fechar": booleano(&t["confirmar_fechar"], true),
        "confirmar_excluir": booleano(&t["confirmar_excluir"], true),
        "efeitos": efeitos_de(&t["efeitos"]),
    })
}

// ================================ fundo ===================================

fn extensao_do_tipo(tipo: &str) -> Option<&'static str> {
    match tipo.split(';').next()?.trim() {
        "image/png" => Some("png"),
        "image/jpeg" => Some("jpg"),
        "image/webp" => Some("webp"),
        "image/gif" => Some("gif"),
        _ => None,
    }
}

fn caminho_fundo(ext: &str) -> PathBuf {
    pasta_config().join(format!("fundo.{ext}"))
}

/// Guarda o fundo enviado e apaga o anterior (uma extensão só por vez).
pub fn salvar_fundo(corpo: &[u8], tipo: &str) -> Result<(), String> {
    let ext = extensao_do_tipo(tipo).ok_or("unsupported format (png, jpeg, webp or gif)")?;
    if corpo.is_empty() || corpo.len() > LIMITE_FUNDO {
        return Err("empty image or larger than 12 MB".into());
    }
    std::fs::create_dir_all(pasta_config()).map_err(|e| format!("config: {e}"))?;
    remover_fundo();
    std::fs::write(caminho_fundo(ext), corpo).map_err(|e| format!("write: {e}"))
}

pub fn remover_fundo() {
    for ext in ["png", "jpg", "webp", "gif"] {
        let _ = std::fs::remove_file(caminho_fundo(ext));
    }
}

/// Tipo MIME e bytes do fundo salvo, se existir.
pub fn ler_fundo() -> Option<(&'static str, Vec<u8>)> {
    for (ext, tipo) in [
        ("png", "image/png"),
        ("jpg", "image/jpeg"),
        ("webp", "image/webp"),
        ("gif", "image/gif"),
    ] {
        if let Ok(bytes) = std::fs::read(caminho_fundo(ext)) {
            return Some((tipo, bytes));
        }
    }
    None
}

// ============================== atividade ==================================

fn caminho_atividade() -> PathBuf {
    pasta_config().join("atividade.json")
}

/// Formato `AAAA-MM-DD` — o front manda a data local, que é a que o usuário vê.
fn dia_valido(dia: &str) -> bool {
    let b = dia.as_bytes();
    b.len() == 10
        && b[4] == b'-'
        && b[7] == b'-'
        && b.iter()
            .enumerate()
            .all(|(i, c)| i == 4 || i == 7 || c.is_ascii_digit())
}

pub fn ler_atividade() -> BTreeMap<String, u64> {
    std::fs::read_to_string(caminho_atividade())
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_default()
}

/// Soma uma execução bem-sucedida no dia informado.
pub fn registrar_execucao(dia: &str) -> Result<(), String> {
    if !dia_valido(dia) {
        return Err("day not in YYYY-MM-DD format".into());
    }
    let _trava = TRAVA_ATIVIDADE.lock().unwrap_or_else(|e| e.into_inner());
    let mut mapa = ler_atividade();
    *mapa.entry(dia.to_string()).or_insert(0) += 1;
    std::fs::create_dir_all(pasta_config()).map_err(|e| format!("config: {e}"))?;
    let texto = serde_json::to_string_pretty(&mapa).map_err(|e| e.to_string())?;
    std::fs::write(caminho_atividade(), texto + "\n").map_err(|e| format!("write: {e}"))
}

// ============================== autoexec ===================================

fn caminho_autoexec() -> PathBuf {
    pasta_config().join("autoexec.json")
}

/// Ligado por padrão: a pasta nasce vazia, então nada roda até o usuário pôr script nela.
pub fn autoexec_ligado() -> bool {
    std::fs::read_to_string(caminho_autoexec())
        .ok()
        .and_then(|t| serde_json::from_str::<Value>(&t).ok())
        .and_then(|v| v["ligado"].as_bool())
        .unwrap_or(true)
}

pub fn salvar_autoexec(ligado: bool) -> Result<(), String> {
    std::fs::create_dir_all(pasta_config()).map_err(|e| format!("config: {e}"))?;
    std::fs::write(caminho_autoexec(), json!({"ligado": ligado}).to_string() + "\n")
        .map_err(|e| format!("write: {e}"))
}

// ============================== changelog ==================================

#[derive(Serialize)]
pub struct Versao {
    pub versao: String,
    pub data: String,
    pub notas: Vec<String>,
}

/// Lê `CHANGELOG.md`: cada `## versão — data` abre uma entrada e cada `- item`
/// vira uma nota. Sem arquivo, a lista volta vazia e a UI mostra o estado vazio.
pub fn ler_changelog(caminho: &Path) -> Vec<Versao> {
    std::fs::read_to_string(caminho)
        .map(|texto| parsear_changelog(&texto))
        .unwrap_or_default()
}

fn parsear_changelog(texto: &str) -> Vec<Versao> {
    let mut versoes: Vec<Versao> = Vec::new();
    for linha in texto.lines().map(str::trim) {
        if let Some(cabeca) = linha.strip_prefix("## ") {
            let (versao, data) = cabeca
                .split_once(" — ")
                .or_else(|| cabeca.split_once(" - "))
                .unwrap_or((cabeca, ""));
            versoes.push(Versao {
                versao: versao.trim().to_string(),
                data: data.trim().to_string(),
                notas: vec![],
            });
        } else if let (Some(item), Some(atual)) = (linha.strip_prefix("- "), versoes.last_mut()) {
            atual.notas.push(item.to_string());
        }
    }
    versoes
}

#[cfg(test)]
mod testes {
    use super::*;

    #[test]
    fn tema_ruim_cai_no_padrao_campo_a_campo() {
        let tema = normalizar_tema(json!({
            "skin": "neon",
            "panel_opacity": 9,
            "tokens": {"window": "vermelho"},
            "nav": "esquerda"
        }));
        assert_eq!(tema["skin"], "vidro");
        assert_eq!(tema["panel_opacity"], 1.0);
        assert_eq!(tema["tokens"]["window"], "#1A2130");
        assert_eq!(tema["nav"], "esquerda");
    }

    #[test]
    fn efeitos_sobrevive_ao_round_trip() {
        for valor in [json!(true), json!(false)] {
            let primeiro = normalizar_tema(json!({"efeitos": valor}));
            assert_eq!(primeiro["efeitos"], valor);
            let segundo = normalizar_tema(primeiro.clone());
            assert_eq!(segundo, primeiro);
        }
    }

    #[test]
    fn ui_json_antigo_sem_efeitos_nao_quebra() {
        let tema = normalizar_tema(json!({"accent": "#ABCDEF", "confirmar_fechar": false}));
        assert!(tema["efeitos"].is_null());
        assert_eq!(tema["accent"], "#ABCDEF");
        assert_eq!(tema["confirmar_fechar"], false);
        assert!(normalizar_tema(json!({"efeitos": "sim"}))["efeitos"].is_null());
    }

    #[test]
    fn cores_do_editor_so_entram_se_valerem() {
        let tema = normalizar_tema(json!({
            "window_opacity": 0.05,
            "confirmar_fechar": false,
            "tokens": {"sin_keyword": "#abcdef", "sin_string": "azul", "editor_fundo": 7}
        }));
        assert_eq!(tema["tokens"]["sin_keyword"], "#ABCDEF");
        assert!(tema["tokens"].get("sin_string").is_none());
        assert!(tema["tokens"].get("editor_fundo").is_none());
        assert_eq!(tema["window_opacity"], 0.2);
        assert_eq!(tema["confirmar_fechar"], false);
        assert_eq!(tema["confirmar_excluir"], true);
    }

    #[test]
    fn fundo_aceita_so_url_http_ou_upload() {
        assert_eq!(fundo_valido(&json!("https://x.y/a.png")), "https://x.y/a.png");
        assert_eq!(fundo_valido(&json!("arquivo")), "arquivo");
        assert_eq!(fundo_valido(&json!("file:///etc/passwd")), "");
        assert_eq!(fundo_valido(&json!("https://x \"<b>")), "");
    }

    #[test]
    fn dia_precisa_do_formato() {
        assert!(dia_valido("2026-10-24"));
        assert!(!dia_valido("24/10/2026"));
        assert!(!dia_valido("2026-10-2x"));
    }

    #[test]
    fn changelog_separa_versao_data_e_notas() {
        let versoes = parsear_changelog("## v1.2 — 2026-08-08\n- primeiro\n- segundo\n## v1.1\n");
        assert_eq!(versoes.len(), 2);
        assert_eq!(versoes[0].versao, "v1.2");
        assert_eq!(versoes[0].data, "2026-08-08");
        assert_eq!(versoes[0].notas.len(), 2);
        assert!(versoes[1].notas.is_empty());
    }
}
