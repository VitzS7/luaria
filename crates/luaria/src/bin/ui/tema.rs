//! Tema da UI: paleta, CSS gerado e persistência em `~/.config/luaria/ui.json`.
//!
//! O GTK4 não tem custom properties (`--accent`); o equivalente dele é
//! `@define-color`. O CSS nasce daqui com os valores já resolvidos, então trocar
//! cor/tema/fonte é só regerar a string e recarregar o `CssProvider`.

use std::path::PathBuf;

use gtk4 as gtk;

pub const ACENTO_PADRAO: &str = "#4C8DFF";
pub const FONTE_PADRAO: i32 = 13;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Modo {
    Escuro,
    Claro,
    Sistema,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Densidade {
    Confortavel,
    Compacta,
}

#[derive(Clone)]
pub struct Tema {
    /// Sempre `#RRGGBB` — validado na leitura.
    pub acento: String,
    pub modo: Modo,
    pub fonte_editor: i32,
    pub densidade: Densidade,
}

impl Default for Tema {
    fn default() -> Self {
        Self {
            acento: ACENTO_PADRAO.into(),
            modo: Modo::Escuro,
            fonte_editor: FONTE_PADRAO,
            densidade: Densidade::Confortavel,
        }
    }
}

impl Tema {
    /// Lê o arquivo; qualquer coisa estranha cai no padrão daquele campo.
    pub fn carregar() -> Self {
        let Ok(txt) = std::fs::read_to_string(caminho()) else {
            return Self::default();
        };
        let Ok(json) = serde_json::from_str::<serde_json::Value>(&txt) else {
            return Self::default();
        };
        Self::do_json(&json)
    }

    fn do_json(json: &serde_json::Value) -> Self {
        let padrao = Self::default();
        let acento = json
            .get("accent")
            .and_then(|v| v.as_str())
            .and_then(normalizar_hex)
            .unwrap_or(padrao.acento);
        let modo = match json.get("theme").and_then(|v| v.as_str()) {
            Some("light") | Some("claro") => Modo::Claro,
            Some("system") | Some("sistema") => Modo::Sistema,
            Some("dark") | Some("escuro") => Modo::Escuro,
            _ => padrao.modo,
        };
        let fonte_editor = json
            .get("editor_font_size")
            .and_then(|v| v.as_i64())
            .map(|v| v.clamp(10, 20) as i32)
            .unwrap_or(padrao.fonte_editor);
        let densidade = match json.get("density").and_then(|v| v.as_str()) {
            Some("compact") | Some("compacta") => Densidade::Compacta,
            Some("comfortable") | Some("confortavel") => Densidade::Confortavel,
            _ => padrao.densidade,
        };
        Self {
            acento,
            modo,
            fonte_editor,
            densidade,
        }
    }

    /// Escrita atômica: um arquivo truncado no meio nunca vira tema válido.
    pub fn salvar(&self) {
        let destino = caminho();
        if let Some(dir) = destino.parent() {
            if std::fs::create_dir_all(dir).is_err() {
                return;
            }
        }
        let json = serde_json::json!({
            "accent": self.acento,
            "theme": self.modo.nome(),
            "editor_font_size": self.fonte_editor,
            "density": self.densidade.nome(),
        });
        let Ok(txt) = serde_json::to_string_pretty(&json) else {
            return;
        };
        let tmp = destino.with_extension("json.tmp");
        if std::fs::write(&tmp, txt + "\n").is_ok() {
            let _ = std::fs::rename(&tmp, &destino);
        }
    }

    /// `Sistema` resolve pelo tema do GTK/GNOME na hora de gerar o CSS.
    pub fn escuro_efetivo(&self) -> bool {
        match self.modo {
            Modo::Escuro => true,
            Modo::Claro => false,
            Modo::Sistema => sistema_escuro(),
        }
    }

    pub fn css(&self) -> String {
        let escuro = self.escuro_efetivo();
        let p = if escuro { Paleta::escura() } else { Paleta::clara() };
        let acento = normalizar_hex(&self.acento).unwrap_or_else(|| ACENTO_PADRAO.into());
        let alvo_hover = if escuro { 255 } else { 0 };
        let mut s = MODELO.to_string();
        let trocas: [(&str, String); 15] = [
            ("accent", acento.clone()),
            ("accent_hover", misturar(&acento, alvo_hover, 0.14)),
            ("accent_press", misturar(&acento, 0, 0.12)),
            ("selecao", alfa(&acento, if escuro { 0.16 } else { 0.11 })),
            ("selecao_forte", alfa(&acento, 0.38)),
            ("bg", p.bg.into()),
            ("surface", p.superficie.into()),
            ("surface_alt", p.superficie_alt.into()),
            ("console_bg", p.console.into()),
            ("line", p.borda.into()),
            ("fg", p.fg.into()),
            ("fg_muted", p.fg_suave.into()),
            ("fg_dim", p.fg_apagado.into()),
            ("ok", p.ok.into()),
            ("bad", p.bad.into()),
        ];
        for (chave, valor) in trocas {
            s = s.replace(&format!("%%{chave}%%"), &valor);
        }
        s = s.replace("%%fonte%%", &self.fonte_editor.to_string());
        s = s.replace("%%realce_linha%%", &p.realce_linha());
        s = s.replace("%%linha_pad%%", if self.densidade == Densidade::Compacta { "5px 6px" } else { "8px 6px" });
        s.replace("%%secao_pad%%", if self.densidade == Densidade::Compacta { "8px 10px" } else { "12px 12px" })
    }

    /// Cor do realce da linha do cursor; discreta nos dois temas.
    pub fn realce_linha(&self) -> String {
        if self.escuro_efetivo() {
            "rgba(255, 255, 255, 0.045)".into()
        } else {
            "rgba(15, 23, 42, 0.055)".into()
        }
    }

    pub fn cor_script(&self) -> String {
        Paleta::de(self).fg.into()
    }
    pub fn cor_info(&self) -> String {
        Paleta::de(self).fg_suave.into()
    }
    pub fn cor_erro(&self) -> String {
        Paleta::de(self).bad.into()
    }
    pub fn cor_secundaria(&self) -> String {
        Paleta::de(self).fg_apagado.into()
    }
}

impl Modo {
    pub fn nome(self) -> &'static str {
        match self {
            Modo::Escuro => "dark",
            Modo::Claro => "light",
            Modo::Sistema => "system",
        }
    }
}

impl Densidade {
    pub fn nome(self) -> &'static str {
        match self {
            Densidade::Confortavel => "comfortable",
            Densidade::Compacta => "compact",
        }
    }
}

struct Paleta {
    bg: &'static str,
    superficie: &'static str,
    superficie_alt: &'static str,
    console: &'static str,
    borda: &'static str,
    fg: &'static str,
    fg_suave: &'static str,
    fg_apagado: &'static str,
    ok: &'static str,
    bad: &'static str,
    realce: &'static str,
}

impl Paleta {
    fn escura() -> Self {
        Self {
            bg: "#0d0f12",
            superficie: "#14171c",
            superficie_alt: "#1a1f26",
            console: "#0a0c0f",
            borda: "#232830",
            fg: "#e8ecf3",
            fg_suave: "#98a2b3",
            fg_apagado: "#6b7684",
            ok: "#3fb950",
            bad: "#f85149",
            realce: "rgba(255, 255, 255, 0.045)",
        }
    }

    fn clara() -> Self {
        Self {
            bg: "#f6f7f9",
            superficie: "#ffffff",
            superficie_alt: "#eceff4",
            console: "#f0f2f6",
            borda: "#dce1e9",
            fg: "#12151a",
            fg_suave: "#55606e",
            fg_apagado: "#7d8794",
            ok: "#1f883d",
            bad: "#cf222e",
            realce: "rgba(15, 23, 42, 0.055)",
        }
    }

    fn de(tema: &Tema) -> Self {
        if tema.escuro_efetivo() {
            Self::escura()
        } else {
            Self::clara()
        }
    }

    fn realce_linha(&self) -> String {
        self.realce.into()
    }
}

fn sistema_escuro() -> bool {
    let Some(s) = gtk::Settings::default() else {
        return true;
    };
    if s.is_gtk_application_prefer_dark_theme() {
        return true;
    }
    s.gtk_theme_name()
        .map(|n| n.to_lowercase().contains("dark"))
        .unwrap_or(false)
}

fn caminho() -> PathBuf {
    if let Ok(base) = std::env::var("XDG_CONFIG_HOME") {
        return PathBuf::from(base).join("luaria/ui.json");
    }
    let home = std::env::var("HOME").unwrap_or_else(|_| ".".into());
    PathBuf::from(home).join(".config/luaria/ui.json")
}

/// Aceita `#abc`, `#aabbcc` e `aabbcc`; devolve sempre maiúsculo com `#`.
pub fn normalizar_hex(h: &str) -> Option<String> {
    let limpo = h.trim().trim_start_matches('#');
    let expandido = match limpo.len() {
        3 => limpo.chars().flat_map(|c| [c, c]).collect::<String>(),
        6 => limpo.to_string(),
        _ => return None,
    };
    if !expandido.chars().all(|c| c.is_ascii_hexdigit()) {
        return None;
    }
    Some(format!("#{}", expandido.to_uppercase()))
}

fn rgb(hex: &str) -> (u8, u8, u8) {
    let h = normalizar_hex(hex).unwrap_or_else(|| ACENTO_PADRAO.into());
    let n = u32::from_str_radix(&h[1..], 16).unwrap_or(0x4C8DFF);
    (((n >> 16) & 0xff) as u8, ((n >> 8) & 0xff) as u8, (n & 0xff) as u8)
}

/// Mistura `hex` com branco (alvo 255) ou preto (alvo 0).
fn misturar(hex: &str, alvo: u8, fator: f32) -> String {
    let (r, g, b) = rgb(hex);
    let m = |c: u8| -> u8 { (c as f32 + (alvo as f32 - c as f32) * fator).round().clamp(0.0, 255.0) as u8 };
    format!("#{:02X}{:02X}{:02X}", m(r), m(g), m(b))
}

fn alfa(hex: &str, a: f32) -> String {
    let (r, g, b) = rgb(hex);
    format!("rgba({r}, {g}, {b}, {a:.2})")
}

const MODELO: &str = r#"
@define-color accent %%accent%%;
@define-color accent_hover %%accent_hover%%;
@define-color accent_press %%accent_press%%;
@define-color bg %%bg%%;
@define-color surface %%surface%%;
@define-color surface_alt %%surface_alt%%;
@define-color console_bg %%console_bg%%;
@define-color line %%line%%;
@define-color fg %%fg%%;
@define-color fg_muted %%fg_muted%%;
@define-color fg_dim %%fg_dim%%;
@define-color ok %%ok%%;
@define-color bad %%bad%%;

window.luaria { background-color: @bg; color: @fg; }

/* ---------- barra superior ---------- */
window.luaria headerbar.barra {
    background-color: @surface;
    background-image: none;
    border-bottom: 1px solid @line;
    box-shadow: none;
    min-height: 46px;
    padding: 0 6px;
}
window.luaria headerbar.barra button,
window.luaria headerbar.barra menubutton > button {
    box-shadow: none;
    text-shadow: none;
}
window.luaria headerbar.barra windowcontrols button,
window.luaria headerbar.barra .titlebutton {
    color: @fg_muted;
    transition: background-color 120ms ease;
}
window.luaria headerbar.barra windowcontrols button:hover,
window.luaria headerbar.barra .titlebutton:hover {
    background-color: @surface_alt;
    color: @fg;
}
window.luaria .marca-ponto {
    min-width: 9px; min-height: 9px; border-radius: 3px;
    background-color: @accent;
}
window.luaria .marca { color: @fg; font-weight: 800; font-size: 14px; letter-spacing: 0.4px; }

window.luaria .pilula {
    background-color: @surface_alt;
    border: 1px solid @line;
    border-radius: 999px;
    padding: 2px 9px;
}
window.luaria .pilula label { color: @fg_muted; font-size: 11px; }
window.luaria .ponto { min-width: 7px; min-height: 7px; border-radius: 999px; background-color: @fg_dim; }
window.luaria .ponto.on { background-color: @ok; box-shadow: 0 0 0 3px alpha(@ok, 0.14); }
window.luaria .ponto.busy { background-color: @accent; }
window.luaria spinner.carregando { color: @accent; min-width: 12px; min-height: 12px; }

window.luaria button.primario {
    background-color: @accent;
    background-image: none;
    color: #ffffff;
    border: none;
    border-radius: 8px;
    padding: 6px 16px;
    font-weight: 600;
    box-shadow: none;
    transition: background-color 120ms ease;
}
window.luaria button.primario:hover { background-color: @accent_hover; }
window.luaria button.primario:active { background-color: @accent_press; }
window.luaria button.primario:disabled {
    background-color: alpha(@accent, 0.30);
    color: alpha(#ffffff, 0.60);
}

window.luaria button.secundario {
    background-color: transparent;
    background-image: none;
    color: @fg_muted;
    border: 1px solid @line;
    border-radius: 8px;
    padding: 5px 12px;
    box-shadow: none;
    transition: background-color 120ms ease;
}
window.luaria button.secundario:hover { background-color: @surface_alt; color: @fg; }
window.luaria button.secundario:active { background-color: %%selecao%%; color: @fg; }
window.luaria button.secundario:disabled { color: alpha(@fg_dim, 0.55); }

window.luaria button.icone,
window.luaria menubutton.icone > button,
window.luaria togglebutton.icone {
    background-color: transparent;
    background-image: none;
    border: none;
    box-shadow: none;
    color: @fg_muted;
    padding: 4px;
    min-width: 26px;
    min-height: 26px;
    border-radius: 6px;
    transition: background-color 120ms ease;
}
window.luaria button.icone:hover,
window.luaria menubutton.icone > button:hover { background-color: @surface_alt; color: @fg; }
window.luaria button.icone:active { background-color: %%selecao%%; }
window.luaria togglebutton.icone:checked { background-color: %%selecao%%; color: @accent; }
window.luaria button.icone:disabled { color: alpha(@fg_dim, 0.45); }

window.luaria button.discreto {
    background-color: transparent;
    background-image: none;
    border: none;
    box-shadow: none;
    color: @fg_dim;
    border-radius: 6px;
    padding: 3px 8px;
    font-size: 11px;
    transition: background-color 120ms ease;
}
window.luaria button.discreto:hover { background-color: @surface_alt; color: @fg_muted; }

/* ---------- sidebar ---------- */
window.luaria .sidebar { background-color: @surface; border-right: 1px solid @line; }
window.luaria .sidebar-titulo { color: @fg_dim; font-size: 10px; font-weight: 700; letter-spacing: 1.1px; }
window.luaria .sidebar-rodape { color: @fg_dim; font-size: 11px; }

window.luaria entry.busca {
    background-color: @surface_alt;
    background-image: none;
    border: 1px solid @line;
    border-radius: 8px;
    box-shadow: none;
    padding: 4px 8px;
    min-height: 26px;
    color: @fg;
    transition: border-color 120ms ease;
}
window.luaria entry.busca:focus-within { border-color: alpha(@accent, 0.65); }
window.luaria entry.busca text { background-color: transparent; color: @fg; }
window.luaria entry.busca text placeholder { color: @fg_dim; }

window.luaria list.scripts { background-color: transparent; }
window.luaria list.scripts > row {
    background-color: transparent;
    border-radius: 6px;
    margin: 1px 6px;
    padding: 0;
    transition: background-color 120ms ease;
}
window.luaria list.scripts > row:hover { background-color: @surface_alt; }
window.luaria list.scripts > row:selected {
    background-color: %%selecao%%;
    box-shadow: inset 2px 0 0 @accent;
}
window.luaria list.scripts > row label { color: @fg_muted; padding: %%linha_pad%%; font-size: 13px; }
window.luaria list.scripts > row:hover label { color: @fg; }
window.luaria list.scripts > row:selected label { color: @fg; font-weight: 600; }

window.luaria .vazio-titulo { color: @fg_muted; font-size: 13px; font-weight: 600; }
window.luaria .vazio-texto { color: @fg_dim; font-size: 12px; }

/* ---------- editor ---------- */
window.luaria .trilha { background-color: @bg; border-bottom: 1px solid alpha(@line, 0.65); padding: 0 8px; min-height: 32px; }
window.luaria .trilha label { color: @fg_dim; font-size: 11px; }
window.luaria .trilha .sujo { color: @accent; font-size: 11px; }

window.luaria textview.editor, window.luaria textview.editor text { background-color: @bg; color: @fg; }
window.luaria textview.editor text {
    font-family: "JetBrains Mono", "Fira Code", "Noto Sans Mono", monospace;
    font-size: %%fonte%%px;
}
window.luaria textview.editor text selection { background-color: %%selecao_forte%%; color: @fg; }
window.luaria textview.editor border { background-color: @bg; }

/* ---------- console ---------- */
window.luaria .console { background-color: @console_bg; border-top: 1px solid @line; }
window.luaria .console-topo { background-color: @surface; border-bottom: 1px solid @line; min-height: 30px; padding: 0 6px; }
window.luaria .console-topo label { color: @fg_dim; font-size: 10px; font-weight: 700; letter-spacing: 1.1px; }
window.luaria textview.console, window.luaria textview.console text { background-color: transparent; color: @fg_muted; }
window.luaria textview.console text {
    font-family: "JetBrains Mono", "Fira Code", "Noto Sans Mono", monospace;
    font-size: 12px;
}
window.luaria textview.console text selection { background-color: %%selecao_forte%%; color: @fg; }

/* ---------- aviso flutuante ---------- */
window.luaria .toast {
    background-color: @surface_alt;
    border: 1px solid @line;
    border-radius: 10px;
    padding: 8px 14px;
    box-shadow: 0 6px 20px alpha(#000000, 0.35);
}
window.luaria .toast label { color: @fg; font-size: 12px; }
window.luaria .toast.erro { border-color: alpha(@bad, 0.55); }
window.luaria .toast.erro label { color: @bad; }

/* ---------- configurações ---------- */
window.luaria popover.config > contents {
    background-color: @surface;
    border: 1px solid @line;
    border-radius: 10px;
    padding: 0;
}
window.luaria .config-titulo { color: @fg_dim; font-size: 10px; font-weight: 700; letter-spacing: 1.1px; }
window.luaria .cfg-rotulo { color: @fg_muted; font-size: 12px; }
window.luaria .cfg-secao { padding: %%secao_pad%%; }

/* ---------- trabalho ---------- */
window.luaria progressbar.trabalho trough {
    min-height: 2px;
    background-color: transparent;
    border: none;
    box-shadow: none;
}
window.luaria progressbar.trabalho progress {
    min-height: 2px;
    background-color: @accent;
    border: none;
    border-radius: 0;
}

window.luaria.compacta list.scripts > row label { padding: 5px 6px; }
window.luaria.compacta .trilha { min-height: 28px; }
window.luaria.compacta .console-topo { min-height: 26px; }
"#;
