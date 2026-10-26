//! Árvore de pastas do executor: `Scripts` (biblioteca), `Workspace` (raiz do
//! writefile/readfile do jogo) e `AutoExec` (roda ao anexar). `Workspace` e
//! `AutoExec` moram em `~/.config/luaria/`; `Scripts` é `pasta_scripts()`.
//!
//! Todo caminho que chega da UI tem a forma `Raiz/sub/arquivo.lua` e passa por
//! `resolver_caminho`, que só devolve algo dentro de uma das três raízes.

use std::path::{Path, PathBuf};

use serde_json::{json, Value};
use luaria::arquivos;

use super::config;
use super::estado::{pasta_config, pasta_scripts};
use super::servidor::{json_de, parametro};

#[cfg(test)]
pub const RAIZES: [&str; 3] = ["Scripts", "Workspace", "AutoExec"];

const PROFUNDIDADE_MAX: usize = 8;
const ENTRADAS_MAX: usize = 5000;
const NOME_MAX: usize = 120;
const MODELO: &str = "-- new script\n-- Ctrl+Enter runs, Ctrl+S saves\n\n";

type Raizes = [(&'static str, PathBuf); 3];

/// Falha com o status HTTP que a UI vai receber.
type Erro = (u16, String);
type Resultado = Result<Value, Erro>;

fn erro(status: u16, texto: impl Into<String>) -> Erro {
    (status, texto.into())
}

fn raizes() -> Raizes {
    [
        ("Scripts", pasta_scripts()),
        ("Workspace", pasta_config().join("Workspace")),
        ("AutoExec", pasta_config().join("AutoExec")),
    ]
}

/// Cria as pastas na subida e traz o conteúdo da antiga `Arquivos/` (writefile)
/// para dentro de `Workspace/`. Se falhar, a árvore mostra a pasta vazia e o
/// erro volta ao usuário na hora de salvar.
pub fn garantir_pastas() {
    for (nome, dir) in raizes() {
        if let Err(e) = std::fs::create_dir_all(&dir) {
            eprintln!("luaria: could not create {nome}: {e}");
        }
    }
    let base = pasta_config();
    migrar_pasta_antiga(&base.join("Arquivos"), &base.join("Workspace"));
}

/// Move entrada por entrada sem sobrescrever nada; só apaga a pasta velha se
/// ficou vazia, então um conflito de nome nunca perde arquivo.
fn migrar_pasta_antiga(antiga: &Path, nova: &Path) {
    let Ok(entradas) = std::fs::read_dir(antiga) else { return };
    for entrada in entradas.flatten() {
        let destino = nova.join(entrada.file_name());
        if !destino.exists() {
            let _ = std::fs::rename(entrada.path(), destino);
        }
    }
    let _ = std::fs::remove_dir(antiga);
}

fn nome_valido(nome: &str) -> Option<&str> {
    let nome = nome.trim();
    let ruim = nome.is_empty()
        || nome.len() > NOME_MAX
        || nome.starts_with('.')
        || nome.contains(['/', '\\', '\0']);
    (!ruim).then_some(nome)
}

fn eh_script(nome: &str) -> bool {
    nome.ends_with(".lua") || nome.ends_with(".luau")
}

fn com_extensao(nome: &str) -> String {
    if eh_script(nome) { nome.to_string() } else { format!("{nome}.lua") }
}

/// (raiz, caminho real). Sem subcaminho devolve a própria raiz.
fn resolver_caminho(raizes: &Raizes, caminho: &str) -> Result<(PathBuf, PathBuf), Erro> {
    let (nome_raiz, resto) = caminho.split_once('/').unwrap_or((caminho, ""));
    let Some((_, raiz)) = raizes.iter().find(|(n, _)| *n == nome_raiz) else {
        return Err(erro(400, "unknown root folder"));
    };
    if resto.is_empty() {
        return Ok((raiz.clone(), raiz.clone()));
    }
    let real = arquivos::resolver(raiz, resto).ok_or_else(|| erro(400, "invalid path"))?;
    Ok((raiz.clone(), real))
}

/// Como `resolver_caminho`, mas recusa a própria raiz (não se renomeia nem apaga).
fn resolver_filho(raizes: &Raizes, caminho: &str) -> Result<PathBuf, Erro> {
    let (raiz, real) = resolver_caminho(raizes, caminho)?;
    if real == raiz {
        return Err(erro(400, "the root folder cannot be changed"));
    }
    Ok(real)
}

fn caminho_relativo(raiz_nome: &str, raiz: &Path, real: &Path) -> String {
    let resto = real.strip_prefix(raiz).unwrap_or(real);
    let resto = resto.to_string_lossy();
    if resto.is_empty() { raiz_nome.to_string() } else { format!("{raiz_nome}/{resto}") }
}

fn nome_livre(pai: &Path, nome: &str) -> String {
    if !pai.join(nome).exists() {
        return nome.to_string();
    }
    let (tronco, ext) = match nome.rsplit_once('.') {
        Some((t, e)) if eh_script(nome) => (t, format!(".{e}")),
        _ => (nome, String::new()),
    };
    (2..)
        .map(|n| format!("{tronco}-{n}{ext}"))
        .find(|candidato| !pai.join(candidato).exists())
        .unwrap_or_else(|| nome.to_string())
}

fn io_erro(e: std::io::Error) -> Erro {
    let status = if e.kind() == std::io::ErrorKind::NotFound { 404 } else { 500 };
    erro(status, e.to_string())
}

/// Pastas primeiro, depois nome sem diferenciar maiúscula. Symlink e arquivo
/// oculto ficam de fora: a árvore nunca mostra nem segue nada que escape da raiz.
fn montar_arvore(dir: &Path, prefixo: &str, nivel: usize, contador: &mut usize) -> Vec<Value> {
    let Ok(entradas) = std::fs::read_dir(dir) else { return vec![] };
    let mut itens: Vec<(bool, String, u64)> = entradas
        .flatten()
        .filter_map(|e| {
            let tipo = e.file_type().ok()?;
            let nome = e.file_name().to_string_lossy().to_string();
            if nome.starts_with('.') || tipo.is_symlink() {
                return None;
            }
            if tipo.is_dir() {
                return Some((true, nome, 0));
            }
            (tipo.is_file() && eh_script(&nome)).then(|| (false, nome, e.metadata().map_or(0, |m| m.len())))
        })
        .collect();
    itens.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.to_lowercase().cmp(&b.1.to_lowercase())));

    let mut nos = Vec::new();
    for (pasta, nome, tamanho) in itens {
        if *contador >= ENTRADAS_MAX {
            break;
        }
        *contador += 1;
        let caminho = format!("{prefixo}/{nome}");
        if pasta {
            let filhos = if nivel < PROFUNDIDADE_MAX {
                montar_arvore(&dir.join(&nome), &caminho, nivel + 1, contador)
            } else {
                vec![]
            };
            nos.push(json!({"nome": nome, "caminho": caminho, "tipo": "pasta", "filhos": filhos}));
        } else {
            nos.push(json!({"nome": nome, "caminho": caminho, "tipo": "arquivo", "tamanho": tamanho}));
        }
    }
    nos
}

fn arvore(raizes: &Raizes) -> Value {
    let mut contador = 0;
    let lista: Vec<Value> = raizes
        .iter()
        .map(|(nome, dir)| {
            let filhos = montar_arvore(dir, nome, 0, &mut contador);
            json!({"nome": nome, "caminho": nome, "tipo": "pasta", "raiz": true, "filhos": filhos})
        })
        .collect();
    json!(lista)
}

fn ler(raizes: &Raizes, caminho: &str) -> Resultado {
    let real = resolver_filho(raizes, caminho)?;
    if !eh_script(caminho) {
        return Err(erro(400, "only .lua/.luau scripts"));
    }
    let conteudo = std::fs::read_to_string(&real).map_err(io_erro)?;
    Ok(json!({"caminho": caminho, "conteudo": conteudo}))
}

fn gravar(raizes: &Raizes, caminho: &str, conteudo: &str) -> Resultado {
    let real = resolver_filho(raizes, caminho)?;
    if !eh_script(caminho) {
        return Err(erro(400, "only .lua/.luau scripts"));
    }
    std::fs::write(&real, conteudo).map_err(io_erro)?;
    Ok(json!({"ok": true, "caminho": caminho}))
}

fn criar(raizes: &Raizes, pai: &str, nome: &str, tipo: &str, conteudo: Option<&str>) -> Resultado {
    let (raiz, dir_pai) = resolver_caminho(raizes, pai)?;
    if !dir_pai.is_dir() {
        return Err(erro(400, "the destination folder does not exist"));
    }
    let nome = nome_valido(nome).ok_or_else(|| erro(400, "invalid name"))?;
    let (nome, criar_arquivo) = match tipo {
        "pasta" => (nome.to_string(), false),
        "script" => (com_extensao(nome), true),
        _ => return Err(erro(400, "type must be folder or script")),
    };
    let nome = nome_livre(&dir_pai, &nome);
    let destino = dir_pai.join(&nome);
    if criar_arquivo {
        std::fs::write(&destino, conteudo.unwrap_or(MODELO)).map_err(io_erro)?;
    } else {
        std::fs::create_dir(&destino).map_err(io_erro)?;
    }
    let nome_raiz = pai.split('/').next().unwrap_or(pai);
    Ok(json!({"ok": true, "nome": nome, "caminho": caminho_relativo(nome_raiz, &raiz, &destino)}))
}

fn renomear(raizes: &Raizes, caminho: &str, novo: &str) -> Resultado {
    let real = resolver_filho(raizes, caminho)?;
    let novo = nome_valido(novo).ok_or_else(|| erro(400, "invalid name"))?;
    let novo = if real.is_dir() { novo.to_string() } else { com_extensao(novo) };
    let pai = real.parent().ok_or_else(|| erro(400, "invalid path"))?;
    let destino = pai.join(&novo);
    if destino.exists() {
        return Err(erro(409, format!("{novo} already exists")));
    }
    std::fs::rename(&real, &destino).map_err(io_erro)?;
    let base = caminho.rsplit_once('/').map_or("", |(p, _)| p);
    Ok(json!({"ok": true, "nome": novo, "caminho": format!("{base}/{novo}")}))
}

fn duplicar(raizes: &Raizes, caminho: &str) -> Resultado {
    let real = resolver_filho(raizes, caminho)?;
    if !real.is_file() {
        return Err(erro(400, "only scripts can be duplicated"));
    }
    let nome = real.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
    let (tronco, ext) = nome.rsplit_once('.').unwrap_or((&nome, "lua"));
    let pai = real.parent().ok_or_else(|| erro(400, "invalid path"))?;
    let copia = nome_livre(pai, &format!("{tronco}-copia.{ext}"));
    std::fs::copy(&real, pai.join(&copia)).map_err(io_erro)?;
    let base = caminho.rsplit_once('/').map_or("", |(p, _)| p);
    Ok(json!({"ok": true, "nome": copia, "caminho": format!("{base}/{copia}")}))
}

fn excluir(raizes: &Raizes, caminho: &str) -> Resultado {
    let real = resolver_filho(raizes, caminho)?;
    // symlink_metadata: um symlink dentro da raiz sai pelo resolver, mas aqui
    // garante que nunca se segue link para apagar o destino.
    let meta = real.symlink_metadata().map_err(io_erro)?;
    if meta.is_dir() {
        std::fs::remove_dir_all(&real).map_err(io_erro)?;
    } else {
        std::fs::remove_file(&real).map_err(io_erro)?;
    }
    Ok(json!({"ok": true}))
}

fn mover(raizes: &Raizes, caminho: &str, destino: &str) -> Resultado {
    let origem = resolver_filho(raizes, caminho)?;
    let (raiz_destino, pasta_destino) = resolver_caminho(raizes, destino)?;
    if !pasta_destino.is_dir() {
        return Err(erro(400, "the destination must be a folder"));
    }
    if origem.is_dir() && pasta_destino.starts_with(&origem) {
        return Err(erro(400, "a folder cannot be moved into itself"));
    }
    let nome = origem.file_name().ok_or_else(|| erro(400, "invalid path"))?;
    let final_ = pasta_destino.join(nome);
    if final_.exists() {
        return Err(erro(409, format!("{} already exists in the destination", nome.to_string_lossy())));
    }
    if let Err(e) = std::fs::rename(&origem, &final_) {
        // raízes em discos diferentes (Scripts do projeto x config): rename falha
        if !origem.is_file() {
            return Err(io_erro(e));
        }
        std::fs::copy(&origem, &final_).map_err(io_erro)?;
        std::fs::remove_file(&origem).map_err(io_erro)?;
    }
    let nome_raiz = destino.split('/').next().unwrap_or(destino);
    Ok(json!({"ok": true, "caminho": caminho_relativo(nome_raiz, &raiz_destino, &final_)}))
}

/// Abre a pasta (ou a pasta do arquivo) no gerenciador do sistema.
fn abrir_no_gerenciador(raizes: &Raizes, caminho: &str) -> Resultado {
    let (_, real) = resolver_caminho(raizes, caminho)?;
    let dir = if real.is_dir() { real } else { real.parent().map(Path::to_path_buf).unwrap_or(real) };
    let mut filho = std::process::Command::new("xdg-open")
        .arg(&dir)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .map_err(|e| erro(500, format!("xdg-open unavailable: {e}")))?;
    // sem wait o filho vira zumbi até a UI fechar
    std::thread::spawn(move || {
        let _ = filho.wait();
    });
    Ok(json!({"ok": true}))
}

/// Scripts do AutoExec na ordem de execução: caminho em ordem alfabética.
fn scripts_autoexec(raizes: &Raizes) -> Vec<Value> {
    let Some((nome, dir)) = raizes.iter().find(|(n, _)| *n == "AutoExec") else { return vec![] };
    let mut caminhos = Vec::new();
    achatar(&montar_arvore(dir, nome, 0, &mut 0), &mut caminhos);
    caminhos.sort_by_key(|c| c.to_lowercase());
    caminhos
        .into_iter()
        .map(|c| json!({"nome": c.rsplit('/').next().unwrap_or(&c), "caminho": c}))
        .collect()
}

fn achatar(nos: &[Value], saida: &mut Vec<String>) {
    for no in nos {
        if let Some(filhos) = no["filhos"].as_array() {
            achatar(filhos, saida);
        } else if let Some(caminho) = no["caminho"].as_str() {
            saida.push(caminho.to_string());
        }
    }
}

fn resposta(resultado: Resultado) -> Vec<u8> {
    match resultado {
        Ok(valor) => json_de(200, &valor),
        Err((status, texto)) => json_de(status, &json!({"ok": false, "erro": texto})),
    }
}

pub fn rota_arvore() -> Vec<u8> {
    json_de(200, &arvore(&raizes()))
}

pub fn rota_ler(query: &[(String, String)]) -> Vec<u8> {
    resposta(ler(&raizes(), &parametro(query, "caminho").unwrap_or_default()))
}

/// `operacao` é o sufixo de `/api/fs/<operacao>`.
pub fn rota_operar(operacao: &str, corpo: &[u8]) -> Vec<u8> {
    let Ok(dados) = serde_json::from_slice::<Value>(corpo) else {
        return json_de(400, &json!({"ok": false, "erro": "invalid JSON"}));
    };
    let texto = |chave: &str| dados[chave].as_str().unwrap_or("");
    let raizes = raizes();
    resposta(match operacao {
        "gravar" => gravar(&raizes, texto("caminho"), texto("conteudo")),
        "criar" => criar(&raizes, texto("pai"), texto("nome"), texto("tipo"), dados["conteudo"].as_str()),
        "renomear" => renomear(&raizes, texto("caminho"), texto("nome")),
        "duplicar" => duplicar(&raizes, texto("caminho")),
        "excluir" => excluir(&raizes, texto("caminho")),
        "mover" => mover(&raizes, texto("caminho"), texto("destino")),
        "abrir" => abrir_no_gerenciador(&raizes, texto("caminho")),
        _ => Err(erro(404, "unknown operation")),
    })
}

pub fn rota_autoexec_ler() -> Vec<u8> {
    json_de(200, &json!({"ligado": config::autoexec_ligado(), "scripts": scripts_autoexec(&raizes())}))
}

pub fn rota_autoexec_gravar(corpo: &[u8]) -> Vec<u8> {
    let ligado = serde_json::from_slice::<Value>(corpo).ok().and_then(|d| d["ligado"].as_bool());
    let Some(ligado) = ligado else {
        return json_de(400, &json!({"ok": false, "erro": "field 'ligado' missing"}));
    };
    match config::salvar_autoexec(ligado) {
        Ok(()) => json_de(200, &json!({"ok": true, "ligado": ligado})),
        Err(e) => json_de(500, &json!({"ok": false, "erro": e})),
    }
}

#[cfg(test)]
mod testes {
    use super::*;

    struct Tmp(PathBuf);
    impl Tmp {
        fn novo(etiqueta: &str) -> (Self, Raizes) {
            let base = std::env::temp_dir().join(format!("luaria-pastas-{etiqueta}-{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&base);
            for r in RAIZES {
                std::fs::create_dir_all(base.join(r)).unwrap();
            }
            let raizes = RAIZES.map(|r| (r, base.join(r)));
            (Tmp(base), raizes)
        }
    }
    impl Drop for Tmp {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn caminho_rejeita_fuga_e_raiz_desconhecida() {
        let (_t, r) = Tmp::novo("fuga");
        for ruim in ["", "Outra/a.lua", "Workspace/../x.lua", "Workspace//x.lua", "Workspace/a\\b.lua", "/etc/passwd"] {
            assert!(resolver_caminho(&r, ruim).is_err(), "{ruim}");
        }
        assert!(resolver_caminho(&r, "Workspace/sub/a.lua").is_ok());
    }

    #[test]
    fn symlink_para_fora_e_recusado() {
        let (t, r) = Tmp::novo("link");
        let fora = t.0.join("fora");
        std::fs::create_dir_all(&fora).unwrap();
        std::fs::write(fora.join("segredo.lua"), "x").unwrap();
        std::os::unix::fs::symlink(&fora, t.0.join("Workspace/atalho")).unwrap();
        assert!(resolver_caminho(&r, "Workspace/atalho/segredo.lua").is_err());
        assert!(excluir(&r, "Workspace/atalho").is_err());
        assert!(fora.join("segredo.lua").exists());
        let arvore = arvore(&r).to_string();
        assert!(!arvore.contains("atalho"));
    }

    #[test]
    fn raiz_nao_pode_ser_renomeada_nem_excluida() {
        let (_t, r) = Tmp::novo("raiz");
        assert!(excluir(&r, "AutoExec").is_err());
        assert!(renomear(&r, "AutoExec", "x").is_err());
    }

    #[test]
    fn criar_renomear_duplicar_mover_excluir() {
        let (t, r) = Tmp::novo("ciclo");
        let pasta = criar(&r, "Workspace", "dados", "pasta", None).unwrap();
        assert_eq!(pasta["caminho"], "Workspace/dados");
        let s = criar(&r, "Workspace/dados", "a", "script", None).unwrap();
        assert_eq!(s["caminho"], "Workspace/dados/a.lua");
        // nome repetido ganha sufixo em vez de sobrescrever
        let s2 = criar(&r, "Workspace/dados", "a", "script", None).unwrap();
        assert_eq!(s2["nome"], "a-2.lua");

        let ren = renomear(&r, "Workspace/dados/a.lua", "b").unwrap();
        assert_eq!(ren["caminho"], "Workspace/dados/b.lua");
        assert_eq!(renomear(&r, "Workspace/dados/b.lua", "a-2.lua").unwrap_err().0, 409);

        let dup = duplicar(&r, "Workspace/dados/b.lua").unwrap();
        assert_eq!(dup["nome"], "b-copia.lua");
        assert!(duplicar(&r, "Workspace/dados").is_err());

        let mv = mover(&r, "Workspace/dados/b.lua", "AutoExec").unwrap();
        assert_eq!(mv["caminho"], "AutoExec/b.lua");
        assert!(t.0.join("AutoExec/b.lua").is_file());
        assert!(mover(&r, "Workspace/dados", "Workspace/dados").is_err());

        excluir(&r, "Workspace/dados").unwrap();
        assert!(!t.0.join("Workspace/dados").exists());
        assert_eq!(excluir(&r, "Workspace/dados").unwrap_err().0, 404);
    }

    #[test]
    fn nomes_invalidos_sao_recusados() {
        let (_t, r) = Tmp::novo("nomes");
        for ruim in ["", "  ", ".oculto", "a/b", "a\\b", ".."] {
            assert!(criar(&r, "Workspace", ruim, "script", None).is_err(), "{ruim:?}");
        }
        assert!(criar(&r, "Workspace", "x", "outro", None).is_err());
    }

    #[test]
    fn ler_e_gravar_so_aceitam_script() {
        let (_t, r) = Tmp::novo("rw");
        gravar(&r, "Workspace/a.lua", "print(1)").unwrap();
        assert_eq!(ler(&r, "Workspace/a.lua").unwrap()["conteudo"], "print(1)");
        assert!(gravar(&r, "Workspace/a.txt", "x").is_err());
        assert_eq!(ler(&r, "Workspace/nao.lua").unwrap_err().0, 404);
    }

    #[test]
    fn autoexec_lista_em_ordem_com_subpastas() {
        let (t, r) = Tmp::novo("auto");
        std::fs::write(t.0.join("AutoExec/b.lua"), "").unwrap();
        std::fs::write(t.0.join("AutoExec/a.lua"), "").unwrap();
        std::fs::write(t.0.join("AutoExec/nota.txt"), "").unwrap();
        std::fs::create_dir(t.0.join("AutoExec/z")).unwrap();
        std::fs::write(t.0.join("AutoExec/z/c.lua"), "").unwrap();
        let caminhos: Vec<String> =
            scripts_autoexec(&r).iter().map(|s| s["caminho"].as_str().unwrap().to_string()).collect();
        assert_eq!(caminhos, ["AutoExec/a.lua", "AutoExec/b.lua", "AutoExec/z/c.lua"]);
    }

    #[test]
    fn migracao_nao_sobrescreve() {
        let (t, _) = Tmp::novo("migra");
        let antiga = t.0.join("Arquivos");
        std::fs::create_dir_all(&antiga).unwrap();
        std::fs::write(antiga.join("a.txt"), "velho").unwrap();
        std::fs::write(antiga.join("b.txt"), "velho-b").unwrap();
        std::fs::write(t.0.join("Workspace/b.txt"), "novo-b").unwrap();
        migrar_pasta_antiga(&antiga, &t.0.join("Workspace"));
        assert_eq!(std::fs::read_to_string(t.0.join("Workspace/a.txt")).unwrap(), "velho");
        assert_eq!(std::fs::read_to_string(t.0.join("Workspace/b.txt")).unwrap(), "novo-b");
        assert!(antiga.join("b.txt").exists());
    }

    #[test]
    fn mover_recusa_conflito_de_nome_no_destino() {
        let (t, r) = Tmp::novo("conflito");
        std::fs::create_dir_all(t.0.join("Workspace/sub")).unwrap();
        std::fs::write(t.0.join("Workspace/a.lua"), "origem").unwrap();
        std::fs::write(t.0.join("Workspace/sub/a.lua"), "destino").unwrap();
        assert_eq!(mover(&r, "Workspace/a.lua", "Workspace/sub").unwrap_err().0, 409);
        assert_eq!(std::fs::read_to_string(t.0.join("Workspace/sub/a.lua")).unwrap(), "destino");
        assert!(t.0.join("Workspace/a.lua").is_file());
    }

    #[test]
    fn resolver_raiz_devolve_a_propria_raiz() {
        let (_t, r) = Tmp::novo("raiz-resolve");
        let (raiz, real) = resolver_caminho(&r, "Workspace").unwrap();
        assert_eq!(raiz, r[1].1);
        assert_eq!(real, r[1].1);
    }
}
