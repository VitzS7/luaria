//! Prelude da API do executor: o Lua de `assets/prelude.lua` mais as atribuições
//! de globais geradas a partir dele.

use std::sync::OnceLock;

const BASE: &str = include_str!("../../../assets/prelude.lua");
const LOADSTRING: &str = include_str!("../../../assets/loadstring.lua");

/// Nomes publicados pelo prelude (`publicar("nome", ...)`), sem repetição.
fn nomes_publicados(fonte: &str) -> Vec<&str> {
    let mut nomes: Vec<&str> = Vec::new();
    for trecho in fonte.split("publicar(\"").skip(1) {
        let Some((nome, _)) = trecho.split_once('"') else { continue };
        let valido = !nome.is_empty()
            && nome.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
            && !nome.starts_with(|c: char| c.is_ascii_digit());
        if valido && !nomes.contains(&nome) {
            nomes.push(nome);
        }
    }
    nomes
}

/// O `_G` da Roblox não é o ambiente dos scripts; global de verdade só nasce de
/// atribuição literal no chunk (o `getfenv` é bloqueado pelo sandbox). Por isso
/// cada nome vira uma linha `nome = __luaria_env.nome`, uma por linha para o
/// número de linhas do prelude ser previsível.
fn exportacoes(fonte: &str) -> String {
    nomes_publicados(fonte)
        .iter()
        .map(|n| format!("{n} = __luaria_env.{n}"))
        .collect::<Vec<_>>()
        .join("\n")
}

pub fn completo() -> &'static str {
    static TEXTO: OnceLock<String> = OnceLock::new();
    TEXTO.get_or_init(|| {
        // loadstring.lua usa `env`/`publicar` locais do prelude, então vai no mesmo chunk.
        // A função em volta isola os locais do prelude: sem ela um global `rev` ou
        // `alvo` do script do usuário caía na variável interna.
        let fonte = format!("{BASE}\n{LOADSTRING}");
        format!(
            "local __luaria_env, __luaria_ponte = (function()\n{fonte}\nreturn env, ponte_globais\nend)()\n{}",
            exportacoes(&fonte)
        )
    })
}

/// Globais comuns que o ambiente do loadstring já resolve sozinho; ficar fora da
/// ponte só economiza bytecode.
const SEM_PONTE: &[&str] = &[
    "and", "break", "do", "else", "elseif", "end", "false", "for", "function", "if", "in", "local", "nil",
    "not", "or", "repeat", "return", "then", "true", "until", "while", "continue", "export", "type", "self",
    "game", "workspace", "script", "print", "warn", "error", "assert", "task", "string", "table", "math",
    "pairs", "ipairs", "next", "select", "type", "typeof", "tostring", "tonumber", "require", "Instance",
    "Vector2", "Vector3", "CFrame", "Color3", "UDim", "UDim2", "Enum", "wait", "spawn", "delay", "tick",
    "pcall", "xpcall", "setmetatable", "getmetatable", "rawget", "rawset", "rawequal", "unpack",
    "coroutine", "os", "debug", "utf8", "bit32", "buffer", "_G", "shared",
];
const MAX_PONTE: usize = 2000;

/// Nomes livres do código (identificador fora de string/comentário que não vem
/// depois de `.`, `:` ou `local`). Pega local também; sobrar nome é inofensivo.
fn nomes_livres(src: &str) -> Vec<String> {
    let b = src.as_bytes();
    let mut nomes = std::collections::BTreeSet::new();
    let mut anterior = "";
    let mut ponto = false;
    let mut i = 0;
    while i < b.len() {
        let c = b[i];
        if c == b'-' && b.get(i + 1) == Some(&b'-') {
            i += 2;
            match abre_longo(b, i) {
                Some((n, ini)) => i = fim_longo(b, n, ini),
                None => {
                    while i < b.len() && b[i] != b'\n' {
                        i += 1;
                    }
                }
            }
        } else if c == b'"' || c == b'\'' || c == b'`' {
            i += 1;
            while i < b.len() && b[i] != c && b[i] != b'\n' {
                i += if b[i] == b'\\' { 2 } else { 1 };
            }
            i += 1;
            ponto = false;
        } else if c == b'[' && abre_longo(b, i).is_some() {
            let (n, ini) = abre_longo(b, i).unwrap_or((0, i + 1));
            i = fim_longo(b, n, ini);
            ponto = false;
        } else if c.is_ascii_alphabetic() || c == b'_' {
            let ini = i;
            while i < b.len() && (b[i].is_ascii_alphanumeric() || b[i] == b'_') {
                i += 1;
            }
            let nome = &src[ini..i];
            if !ponto && anterior != "local" && !nome.starts_with("__luaria") && !SEM_PONTE.contains(&nome) {
                nomes.insert(nome.to_string());
            }
            anterior = nome;
            ponto = false;
        } else if c.is_ascii_digit() {
            // número com sufixo/expoente (1e5, 0xff) não vira nome
            while i < b.len() && (b[i].is_ascii_alphanumeric() || b[i] == b'.' || b[i] == b'_') {
                i += 1;
            }
            ponto = false;
        } else if c == b'.' && b.get(i + 1) == Some(&b'.') {
            // `..` e `...` não são acesso a campo
            while i < b.len() && b[i] == b'.' {
                i += 1;
            }
            ponto = false;
            anterior = "";
        } else {
            if !c.is_ascii_whitespace() {
                ponto = c == b'.' || c == b':';
                anterior = "";
            }
            i += 1;
        }
    }
    nomes.into_iter().take(MAX_PONTE).collect()
}

fn abre_longo(b: &[u8], i: usize) -> Option<(usize, usize)> {
    let mut j = i + 1;
    while b.get(j) == Some(&b'=') {
        j += 1;
    }
    (b.get(i) == Some(&b'[') && b.get(j) == Some(&b'[')).then_some((j - i - 1, j + 1))
}

fn fim_longo(b: &[u8], nivel: usize, ini: usize) -> usize {
    let mut fecha = vec![b'='; nivel + 2];
    fecha[0] = b']';
    fecha[nivel + 1] = b']';
    b[ini..].windows(fecha.len()).position(|w| w == fecha.as_slice()).map_or(b.len(), |p| ini + p + fecha.len())
}

/// Despacho em árvore binária por nome: log2(n) comparações por acesso.
fn arvore(nomes: &[String], ler: bool) -> String {
    if nomes.len() <= 4 {
        return nomes
            .iter()
            .map(|n| {
                if ler {
                    format!("if __luaria_k == \"{n}\" then return {n} end ")
                } else {
                    format!("if __luaria_k == \"{n}\" then {n} = __luaria_v return true end ")
                }
            })
            .collect();
    }
    let meio = nomes.len() / 2;
    format!(
        "if __luaria_k < \"{}\" then {}else {}end ",
        nomes[meio],
        arvore(&nomes[..meio], ler),
        arvore(&nomes[meio..], ler)
    )
}

/// Linha única que liga `getgenv()` aos globais de verdade do chunk: sem
/// `getfenv`/`setfenv` não há acesso dinâmico a global, então o host gera um
/// leitor/gravador com os nomes que o script usa. `getgenv().X = 1` passa a
/// valer como global `X` e vice-versa. Termina em `;` para o código do usuário
/// continuar na mesma linha sem deslocar a numeração.
pub fn ponte_globais(codigo: &str) -> String {
    let nomes = nomes_livres(codigo);
    format!(
        "__luaria_ponte(function(__luaria_k) {}end, function(__luaria_k, __luaria_v) {}return false end);",
        arvore(&nomes, true),
        arvore(&nomes, false)
    )
}

#[cfg(test)]
mod testes {
    use super::*;

    #[test]
    fn extrai_nomes_sem_repetir() {
        let src = "publicar(\"a\", 1)\npublicar(\"b_2\", f)\npublicar(\"a\", 2)\npublicar(\"x.y\", 3)";
        assert_eq!(nomes_publicados(src), vec!["a", "b_2"]);
    }

    #[test]
    fn gera_uma_linha_por_nome() {
        assert_eq!(exportacoes("publicar(\"a\",1) publicar(\"b\",2)"), "a = __luaria_env.a\nb = __luaria_env.b");
    }

    #[test]
    fn prelude_real_exporta_algo() {
        assert!(completo().contains("identifyexecutor = __luaria_env.identifyexecutor"));
    }

    #[test]
    fn prelude_exporta_loadstring_e_getgenv_uma_vez() {
        let c = completo();
        assert_eq!(c.matches("\nloadstring = __luaria_env.loadstring").count(), 1);
        assert_eq!(c.matches("\ngetgenv = __luaria_env.getgenv").count(), 1);
    }

    #[test]
    fn nomes_livres_ignora_campo_string_e_local() {
        let src = "local a = b.c + d:e() .. g X = 'Y' --[[ Z ]] -- W\nprint(Settings, 1e5, `T{U}`) [[V]] function F() end";
        assert_eq!(nomes_livres(src), vec!["F", "Settings", "X", "b", "d", "g"]);
    }

    #[test]
    fn ponte_e_uma_linha_so() {
        let p = ponte_globais("A B C D E F G");
        assert!(!p.contains('\n') && p.ends_with(");"));
        assert!(p.contains("if __luaria_k < \"D\" then"));
        assert!(p.contains("if __luaria_k == \"G\" then G = __luaria_v return true end"));
        assert_eq!(ponte_globais(""), "__luaria_ponte(function(__luaria_k) end, function(__luaria_k, __luaria_v) return false end);");
    }
}
