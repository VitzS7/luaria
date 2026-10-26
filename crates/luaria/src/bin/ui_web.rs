//! UI do executor em WebKitGTK 6.0: a tela é HTML/CSS/JS servida por um HTTP
//! local; o motor vive numa thread própria e o WebView nunca espera por ele.
//!
//! A janela é só um casco sem decoração: a barra de título, os botões e o
//! tema moram em `assets/ui/`. Os pedidos de janela (minimizar, arrastar...)
//! chegam por HTTP e são executados aqui, na thread GTK, por um canal.
//! O `luaria-gtk` continua existindo como fallback GTK puro.

#[path = "ui_web/config.rs"]
mod config;
#[path = "ui_web/estado.rs"]
mod estado;
#[path = "ui_web/servidor.rs"]
mod servidor;
#[path = "ui_web/pastas.rs"]
mod pastas;
#[path = "ui_web/clientes_api.rs"]
mod clientes_api;

use std::cell::RefCell;
use std::sync::mpsc::{channel, Receiver};
use std::time::Duration;

use gtk4 as gtk;
use webkit6::prelude::*;

use servidor::AcaoJanela;

fn main() -> gtk::glib::ExitCode {
    let estado = estado::Estado::novo();
    pastas::garantir_pastas();
    let (tx_janela, rx_janela) = channel::<AcaoJanela>();
    let porta = match servidor::iniciar(estado.clone(), tx_janela) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("luaria: could not open the local server: {e}");
            std::process::exit(1);
        }
    };

    let app = gtk::Application::builder()
        .application_id("dev.luaria.executor")
        .build();

    // `connect_activate` aceita só `Fn`; o receptor sai do Option na primeira vez.
    let rx_janela = RefCell::new(Some(rx_janela));
    app.connect_activate(move |app| {
        // Segunda ativação (clicar no atalho de novo): só traz a janela existente
        // pra frente, senão nasceria uma 2ª janela com os botões mortos (rx já consumido).
        if let Some(janela) = app.active_window() {
            janela.present();
            return;
        }

        // A UI é escura; sem isso a barra de título segue o tema do sistema e
        // pode vir clara em cima de conteúdo escuro.
        if let Some(settings_gtk) = gtk::Settings::default() {
            settings_gtk.set_gtk_application_prefer_dark_theme(true);
        }

        let janela = gtk::ApplicationWindow::builder()
            .application(app)
            .title("Luaria")
            .default_width(1200)
            .default_height(780)
            .build();
        janela.set_icon_name(Some("luaria"));
        // Sem decoração do GTK: a barra de título é a da UI, com a mesma pilha
        // visual em qualquer tema de janela.
        janela.set_decorated(false);
        if let Some(rx) = rx_janela.borrow_mut().take() {
            ligar_acoes_janela(janela.clone(), rx);
        }

        let webview = webkit6::WebView::builder().build();
        let config_web = webkit6::Settings::builder()
            .enable_developer_extras(std::env::var("LUARIA_DEV").is_ok())
            .enable_smooth_scrolling(true)
            .build();
        webview.set_settings(&config_web);
        let mut url = format!("http://127.0.0.1:{porta}/");
        let mut consulta: Vec<String> = Vec::new();
        // Cantos arredondados = janela e WebView transparentes e o recorte no
        // CSS (html.cantos). Precisa de compositor: X11 sem um mostra cantos
        // pretos, daí o LUARIA_CANTOS=0 para voltar ao retângulo opaco.
        if std::env::var("LUARIA_CANTOS").map_or(true, |v| v != "0") {
            ligar_cantos(&janela, &webview);
            consulta.push("cantos=1".into());
        } else {
            webview.set_background_color(&gtk::gdk::RGBA::new(0.102, 0.129, 0.188, 1.0));
        }
        if std::env::var("LUARIA_DEV").is_ok() {
            consulta.push("dev=1".into());
        }
        for chave in ["LUARIA_VISTA", "LUARIA_ARQUIVO"] {
            if let Ok(valor) = std::env::var(chave) {
                let nome = if chave == "LUARIA_VISTA" { "vista" } else { "arquivo" };
                consulta.push(format!("{nome}={valor}"));
            }
        }
        if !consulta.is_empty() {
            url.push('?');
            url.push_str(&consulta.join("&"));
        }
        webview.load_uri(&url);

        // Nada de navegar para fora da UI local (um link no script não pode
        // trocar a tela do executor por uma página da web). O fundo por URL
        // é carregado como recurso de imagem, não como navegação.
        let origem = format!("http://127.0.0.1:{porta}");
        webview.connect_decide_policy(move |_, decisao, tipo| {
            if tipo == webkit6::PolicyDecisionType::NavigationAction {
                if let Some(nav) = decisao.downcast_ref::<webkit6::NavigationPolicyDecision>() {
                    if let Some(mut acao) = nav.navigation_action() {
                        let uri = acao
                            .request()
                            .map(|r| r.uri().unwrap_or_default())
                            .unwrap_or_default();
                        if !uri.starts_with(&origem) && !uri.starts_with("about:") {
                            decisao.ignore();
                            return true;
                        }
                    }
                }
            }
            false
        });

        janela.set_child(Some(&webview));
        janela.present();
        webview.grab_focus();
    });

    app.run()
}

fn ligar_cantos(janela: &gtk::ApplicationWindow, webview: &webkit6::WebView) {
    let css = gtk::CssProvider::new();
    css.load_from_data("window.luaria-transparente { background: transparent; }");
    if let Some(tela) = gtk::gdk::Display::default() {
        gtk::style_context_add_provider_for_display(&tela, &css, gtk::STYLE_PROVIDER_PRIORITY_APPLICATION);
    }
    janela.add_css_class("luaria-transparente");
    webview.set_background_color(&gtk::gdk::RGBA::new(0.0, 0.0, 0.0, 0.0));
    // maximizada/tela cheia encosta na borda da tela: sem raio
    let avisar = {
        let webview = webview.clone();
        move |j: &gtk::ApplicationWindow| {
            let colado = j.is_maximized() || j.is_fullscreen();
            let js = format!("document.documentElement.classList.toggle('maximizada', {colado})");
            webview.evaluate_javascript(&js, None, None, None::<&gtk::gio::Cancellable>, |_| {});
        }
    };
    let avisar2 = avisar.clone();
    janela.connect_maximized_notify(avisar);
    janela.connect_fullscreened_notify(avisar2);
}

/// Drena os pedidos da UI na thread GTK. O intervalo é curto para o clique
/// responder sem parecer atrasado.
fn ligar_acoes_janela(janela: gtk::ApplicationWindow, rx: Receiver<AcaoJanela>) {
    gtk::glib::timeout_add_local(Duration::from_millis(40), move || {
        while let Ok(acao) = rx.try_recv() {
            aplicar_acao(&janela, acao);
        }
        gtk::glib::ControlFlow::Continue
    });
}

fn aplicar_acao(janela: &gtk::ApplicationWindow, acao: AcaoJanela) {
    match acao {
        AcaoJanela::Minimizar => janela.minimize(),
        AcaoJanela::Maximizar => {
            if janela.is_maximized() {
                janela.unmaximize();
            } else {
                janela.maximize();
            }
        }
        AcaoJanela::Fechar => janela.close(),
        AcaoJanela::Opacidade { valor } => janela.set_opacity(valor.clamp(0.2, 1.0)),
        AcaoJanela::Arrastar { x, y, botao } => arrastar(janela, x, y, botao),
        AcaoJanela::Redimensionar { borda, x, y, botao } => redimensionar(janela, &borda, x, y, botao),
    }
}

/// Inicia o arraste nativo do WM. Sem decoração o GTK não arrasta sozinho; o
/// tempo 0 é GDK_CURRENT_TIME, que o compositor aceita para um clique recente.
fn arrastar(janela: &gtk::ApplicationWindow, x: f64, y: f64, botao: i32) {
    let Some(superficie) = janela.surface() else { return };
    let Ok(topo) = superficie.downcast::<gtk::gdk::Toplevel>() else { return };
    let Some(seat) = gtk::gdk::Display::default().and_then(|d| d.default_seat()) else {
        return;
    };
    let Some(ponteiro) = seat.pointer() else { return };
    topo.begin_move(&ponteiro, botao, x, y, 0);
}

/// Redimensiona pela borda pegada na UI. Sem decoração o WM não mostra alça, então
/// a UI manda a sigla (`no`, `ne`, `se`...) e o GTK inicia o resize nativo.
fn redimensionar(janela: &gtk::ApplicationWindow, borda: &str, x: f64, y: f64, botao: i32) {
    let Some(borda) = borda_de(borda) else { return };
    let Some(superficie) = janela.surface() else { return };
    let Ok(topo) = superficie.downcast::<gtk::gdk::Toplevel>() else { return };
    let Some(seat) = gtk::gdk::Display::default().and_then(|d| d.default_seat()) else {
        return;
    };
    let Some(ponteiro) = seat.pointer() else { return };
    topo.begin_resize(borda, Some(&ponteiro), botao, x, y, 0);
}

fn borda_de(sigla: &str) -> Option<gtk::gdk::SurfaceEdge> {
    use gtk::gdk::SurfaceEdge as Borda;
    Some(match sigla {
        "n" => Borda::North,
        "s" => Borda::South,
        "l" => Borda::East,
        "o" => Borda::West,
        "no" => Borda::NorthWest,
        "ne" => Borda::NorthEast,
        "so" => Borda::SouthWest,
        "se" => Borda::SouthEast,
        _ => return None,
    })
}
