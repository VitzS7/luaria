//! UI do executor: editor de script + console do cliente + execução.
//!
//! O motor nunca roda na thread do GTK: o clique retira o `Motor` do
//! compartimento, executa numa thread própria (com retry) e devolve. O console
//! recebe o log por canal e marca a saída do script com `»`.
//!
//! Aparência e persistência ficam em `ui/tema.rs`; aqui só se monta a tela.

#[path = "ui/tema.rs"]
mod tema;

use std::cell::{Cell, RefCell};
use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{channel, Sender};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use gdk4 as gdk;
use gtk4 as gtk;
use gtk::glib;
use gtk::prelude::*;

use luaria::engine::Motor;
use tema::{Densidade, Modo, Tema};

/// Tentativas por clique. Cada `Motor::executar` ainda tenta 3 vezes por dentro.
const TENTATIVAS: usize = 3;

/// Ruído conhecido do log do cliente: não vai para o console.
const RUIDO: [&str; 3] = ["[perf]", "Wrap-deformer", "[compat]"];

/// Teto do console; ao passar disso as mil linhas mais velhas caem.
const MAX_LINHAS: usize = 4000;

#[derive(Clone, Copy)]
enum Estilo {
    /// Saída do script (`FLog::CreatorOutput`) e marcos do motor.
    Script,
    /// Diagnóstico do motor.
    Info,
    Erro,
    /// Linha secundária do cliente (`FLog::Output`, warnings).
    Secundario,
}

struct Linha {
    estilo: Estilo,
    texto: String,
}

/// Estado visível do cliente, para o cabeçalho e para o rodapé.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Conexao {
    Off,
    Conectando,
    Conectado(i32),
    Executando,
}

/// Aviso que chega da thread de execução/conexão.
enum EstadoCli {
    Conectado(i32),
    Falha { mensagem: String, conectado: Option<i32> },
}

/// Compartimento do motor + travas. Só isso atravessa para as threads.
#[derive(Clone)]
struct MotorCtx {
    motor: Arc<Mutex<Option<Motor>>>,
    rodando: Arc<AtomicBool>,
    conectando: Arc<AtomicBool>,
}

struct Documento {
    caminho: Option<PathBuf>,
    sujo: bool,
}

/// Solta o "ocupado" mesmo quando a thread sai por erro.
struct Ocupado(Arc<AtomicBool>);

impl Drop for Ocupado {
    fn drop(&mut self) {
        self.0.store(false, Ordering::SeqCst);
    }
}

#[derive(Clone)]
struct Toast {
    caixa: gtk::Box,
    rotulo: gtk::Label,
    revealer: gtk::Revealer,
    geracao: Rc<Cell<u32>>,
}

impl Toast {
    fn mostrar(&self, texto: &str, erro: bool) {
        self.rotulo.set_text(texto);
        if erro {
            self.caixa.add_css_class("erro");
        } else {
            self.caixa.remove_css_class("erro");
        }
        self.revealer.set_reveal_child(true);
        let g = self.geracao.get().wrapping_add(1);
        self.geracao.set(g);
        let este = self.clone();
        glib::timeout_add_local_once(Duration::from_secs(3), move || {
            if este.geracao.get() == g {
                este.revealer.set_reveal_child(false);
            }
        });
    }
}

#[derive(Clone)]
struct Cores {
    script: gtk::TextTag,
    info: gtk::TextTag,
    erro: gtk::TextTag,
    secundario: gtk::TextTag,
    linha: gtk::TextTag,
}

impl Cores {
    fn criar(editor: &gtk::TextBuffer, console: &gtk::TextBuffer, tema: &Tema) -> Self {
        let script = console
            .create_tag(
                Some("console-script"),
                &[("foreground", &tema.cor_script()), ("weight", &600i32)],
            )
            .expect("tag script");
        let info = console
            .create_tag(Some("console-info"), &[("foreground", &tema.cor_info())])
            .expect("tag info");
        let erro = console
            .create_tag(Some("console-erro"), &[("foreground", &tema.cor_erro())])
            .expect("tag erro");
        let secundario = console
            .create_tag(
                Some("console-secundario"),
                &[("foreground", &tema.cor_secundaria())],
            )
            .expect("tag secundario");
        let linha = editor
            .create_tag(
                Some("linha-atual"),
                &[("paragraph-background", &tema.realce_linha())],
            )
            .expect("tag linha");
        Self {
            script,
            info,
            erro,
            secundario,
            linha,
        }
    }

    fn atualizar(&self, tema: &Tema) {
        self.script.set_property("foreground", tema.cor_script());
        self.info.set_property("foreground", tema.cor_info());
        self.erro.set_property("foreground", tema.cor_erro());
        self.secundario.set_property("foreground", tema.cor_secundaria());
        self.linha
            .set_property("paragraph-background", tema.realce_linha());
    }
}

#[derive(Clone)]
struct Tela {
    janela: gtk::ApplicationWindow,
    provider: gtk::CssProvider,
    ponto: gtk::Box,
    status: gtk::Label,
    spinner: gtk::Spinner,
    botao_anexar: gtk::Button,
    botao_exec: gtk::Button,
    botao_console: gtk::ToggleButton,
    progresso: gtk::ProgressBar,
    toast: Toast,
    console_revealer: gtk::Revealer,
    pilha_console: gtk::Stack,
    console_buffer: gtk::TextBuffer,
    console_view: gtk::TextView,
    editor: gtk::TextView,
    lista: gtk::ListBox,
    pilha_lista: gtk::Stack,
    trilha_nome: gtk::Label,
    marca_sujo: gtk::Label,
    total: gtk::Label,
    cores: Cores,
    visual: Rc<Cell<Conexao>>,
}

fn main() {
    let app = gtk::Application::builder()
        .application_id("dev.luaria.executor")
        .build();
    app.connect_activate(monta);
    std::process::exit(app.run().into());
}

fn monta(app: &gtk::Application) {
    // ícone do hicolor (tools/instalar.sh); precisa do GTK já inicializado
    gtk::Window::set_default_icon_name("luaria");

    let provider = gtk::CssProvider::new();
    gtk::style_context_add_provider_for_display(
        &gdk::Display::default().expect("sem display"),
        &provider,
        gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
    );

    let tema = Rc::new(RefCell::new(Tema::carregar()));
    let doc = Rc::new(RefCell::new(Documento {
        caminho: None,
        sujo: false,
    }));

    let (tx, rx) = channel::<String>();
    let (tx_conexao, rx_conexao) = channel::<(bool, Result<Motor, String>)>();
    let (tx_estado, rx_estado) = channel::<EstadoCli>();

    let ctx = MotorCtx {
        motor: Arc::new(Mutex::new(None)),
        rodando: Arc::new(AtomicBool::new(false)),
        conectando: Arc::new(AtomicBool::new(false)),
    };

    // ---- janela + barra superior ------------------------------------------
    let janela = gtk::ApplicationWindow::builder()
        .application(app)
        .title("luaria")
        .default_width(1180)
        .default_height(760)
        .build();
    janela.add_css_class("luaria");
    janela.set_size_request(940, 580);

    let barra = gtk::HeaderBar::new();
    barra.add_css_class("barra");
    barra.set_show_title_buttons(true);

    let marca_ponto = gtk::Box::new(gtk::Orientation::Vertical, 0);
    marca_ponto.add_css_class("marca-ponto");
    marca_ponto.set_valign(gtk::Align::Center);

    let marca = gtk::Label::new(Some("luaria"));
    marca.add_css_class("marca");
    marca.set_valign(gtk::Align::Center);

    let ponto = gtk::Box::new(gtk::Orientation::Vertical, 0);
    ponto.add_css_class("ponto");
    ponto.set_valign(gtk::Align::Center);

    let spinner = gtk::Spinner::new();
    spinner.add_css_class("carregando");
    spinner.set_valign(gtk::Align::Center);
    spinner.set_visible(false);

    let status = gtk::Label::new(Some("disconnected"));
    status.set_valign(gtk::Align::Center);

    let pilula = gtk::Box::new(gtk::Orientation::Horizontal, 7);
    pilula.add_css_class("pilula");
    pilula.set_valign(gtk::Align::Center);
    pilula.append(&ponto);
    pilula.append(&spinner);
    pilula.append(&status);

    let titulo = gtk::Box::new(gtk::Orientation::Horizontal, 9);
    titulo.set_valign(gtk::Align::Center);
    titulo.append(&marca_ponto);
    titulo.append(&marca);
    titulo.append(&pilula);
    barra.set_title_widget(Some(&titulo));

    let botao_config = gtk::MenuButton::builder()
        .icon_name("emblem-system-symbolic")
        .build();
    botao_config.add_css_class("icone");
    botao_config.set_tooltip_text(Some("Appearance"));

    let botao_console = gtk::ToggleButton::builder()
        .icon_name("utilities-terminal-symbolic")
        .active(true)
        .build();
    botao_console.add_css_class("icone");
    botao_console.set_tooltip_text(Some("Show/hide console (Ctrl+`)"));
    botao_console.set_active(true);

    let botao_anexar = gtk::Button::with_label("Attach");
    botao_anexar.add_css_class("secundario");
    botao_anexar.set_tooltip_text(Some("Attach to the open Roblox client"));

    let botao_exec = gtk::Button::with_label("Run");
    botao_exec.add_css_class("primario");
    botao_exec.set_tooltip_text(Some("Run the editor script (Ctrl+Enter)"));
    botao_exec.set_sensitive(false);

    barra.pack_start(&botao_config);
    barra.pack_start(&botao_console);
    barra.pack_end(&botao_exec);
    barra.pack_end(&botao_anexar);
    janela.set_titlebar(Some(&barra));

    // ---- sidebar -----------------------------------------------------------
    let lateral = gtk::Box::new(gtk::Orientation::Vertical, 0);
    lateral.add_css_class("sidebar");
    lateral.set_size_request(240, -1);

    let topo_lateral = gtk::Box::new(gtk::Orientation::Horizontal, 2);
    topo_lateral.set_margin_top(10);
    topo_lateral.set_margin_bottom(6);
    topo_lateral.set_margin_start(12);
    topo_lateral.set_margin_end(8);

    let rotulo_scripts = gtk::Label::new(Some("SCRIPTS"));
    rotulo_scripts.add_css_class("sidebar-titulo");
    rotulo_scripts.set_xalign(0.0);
    rotulo_scripts.set_hexpand(true);

    let b_novo = botao_icone("list-add-symbolic", "New script (Ctrl+N)");
    let b_salvar = botao_icone("document-save-symbolic", "Save script (Ctrl+S)");
    let b_excluir = botao_icone("user-trash-symbolic", "Delete script");
    topo_lateral.append(&rotulo_scripts);
    topo_lateral.append(&b_novo);
    topo_lateral.append(&b_salvar);
    topo_lateral.append(&b_excluir);

    let busca = gtk::SearchEntry::new();
    busca.add_css_class("busca");
    busca.set_placeholder_text(Some("filter scripts…"));
    busca.set_margin_start(10);
    busca.set_margin_end(10);
    busca.set_margin_bottom(8);

    let lista = gtk::ListBox::new();
    lista.add_css_class("scripts");
    lista.set_selection_mode(gtk::SelectionMode::Single);
    lista.set_activate_on_single_click(true);

    let scroll_lista = gtk::ScrolledWindow::builder()
        .child(&lista)
        .hscrollbar_policy(gtk::PolicyType::Never)
        .vexpand(true)
        .build();

    let vazio_lista = gtk::Box::new(gtk::Orientation::Vertical, 6);
    vazio_lista.set_valign(gtk::Align::Center);
    vazio_lista.set_halign(gtk::Align::Center);
    vazio_lista.set_margin_start(18);
    vazio_lista.set_margin_end(18);
    let vazio_titulo = gtk::Label::new(Some("no scripts"));
    vazio_titulo.add_css_class("vazio-titulo");
    let vazio_texto = gtk::Label::new(Some("create the first one with +"));
    vazio_texto.add_css_class("vazio-texto");
    let vazio_botao = gtk::Button::with_label("New script");
    vazio_botao.add_css_class("secundario");
    vazio_botao.set_margin_top(6);
    vazio_lista.append(&vazio_titulo);
    vazio_lista.append(&vazio_texto);
    vazio_lista.append(&vazio_botao);

    let pilha_lista = gtk::Stack::new();
    pilha_lista.add_named(&scroll_lista, Some("lista"));
    pilha_lista.add_named(&vazio_lista, Some("vazio"));
    pilha_lista.set_visible_child_name("vazio");
    pilha_lista.set_vexpand(true);

    let total = gtk::Label::new(Some("0 scripts"));
    total.add_css_class("sidebar-rodape");
    total.set_xalign(0.0);
    total.set_margin_start(12);
    total.set_margin_end(12);
    total.set_margin_top(4);
    total.set_margin_bottom(8);

    lateral.append(&topo_lateral);
    lateral.append(&busca);
    lateral.append(&pilha_lista);
    lateral.append(&total);

    // ---- editor ------------------------------------------------------------
    let editor = gtk::TextView::builder()
        .monospace(true)
        .wrap_mode(gtk::WrapMode::None)
        .left_margin(28)
        .right_margin(28)
        .top_margin(18)
        .bottom_margin(18)
        .build();
    editor.add_css_class("editor");
    editor.set_pixels_below_lines(2);
    editor.set_accepts_tab(true);

    let scroll_editor = gtk::ScrolledWindow::builder()
        .child(&editor)
        .vexpand(true)
        .hscrollbar_policy(gtk::PolicyType::Automatic)
        .build();

    let trilha = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    trilha.add_css_class("trilha");
    let trilha_nome = gtk::Label::new(Some("no file"));
    trilha_nome.set_xalign(0.0);
    trilha_nome.set_hexpand(true);
    trilha_nome.set_ellipsize(gtk::pango::EllipsizeMode::Middle);
    trilha_nome.set_valign(gtk::Align::Center);
    let marca_sujo = gtk::Label::new(Some("●"));
    marca_sujo.add_css_class("sujo");
    marca_sujo.set_valign(gtk::Align::Center);
    marca_sujo.set_visible(false);
    trilha.append(&trilha_nome);
    trilha.append(&marca_sujo);

    // ---- console -----------------------------------------------------------
    let console = gtk::TextView::builder()
        .editable(false)
        .cursor_visible(false)
        .monospace(true)
        .left_margin(14)
        .right_margin(14)
        .top_margin(10)
        .bottom_margin(10)
        .build();
    console.add_css_class("console");
    let console_buffer = console.buffer();

    let scroll_console = gtk::ScrolledWindow::builder()
        .child(&console)
        .vexpand(true)
        .min_content_height(184)
        .max_content_height(184)
        .build();

    let vazio_console = gtk::Box::new(gtk::Orientation::Vertical, 4);
    vazio_console.set_valign(gtk::Align::Center);
    vazio_console.set_halign(gtk::Align::Center);
    let vazio_console_titulo = gtk::Label::new(Some("nothing run yet"));
    vazio_console_titulo.add_css_class("vazio-titulo");
    let vazio_console_texto = gtk::Label::new(Some("Ctrl+Enter runs the editor script"));
    vazio_console_texto.add_css_class("vazio-texto");
    vazio_console.append(&vazio_console_titulo);
    vazio_console.append(&vazio_console_texto);

    let pilha_console = gtk::Stack::new();
    pilha_console.add_named(&scroll_console, Some("log"));
    pilha_console.add_named(&vazio_console, Some("vazio"));
    pilha_console.set_visible_child_name("vazio");
    pilha_console.set_vexpand(true);

    let topo_console = gtk::Box::new(gtk::Orientation::Horizontal, 4);
    topo_console.add_css_class("console-topo");
    let rotulo_console = gtk::Label::new(Some("OUTPUT"));
    rotulo_console.add_css_class("sidebar-titulo");
    rotulo_console.set_xalign(0.0);
    rotulo_console.set_hexpand(true);
    rotulo_console.set_margin_start(6);
    let b_limpar = gtk::Button::with_label("Clear");
    b_limpar.add_css_class("discreto");
    b_limpar.set_tooltip_text(Some("Clear output"));
    let b_fechar = botao_icone("window-close-symbolic", "Hide console");
    topo_console.append(&rotulo_console);
    topo_console.append(&b_limpar);
    topo_console.append(&b_fechar);

    let painel_console = gtk::Box::new(gtk::Orientation::Vertical, 0);
    painel_console.add_css_class("console");
    painel_console.set_size_request(-1, 212);
    painel_console.append(&topo_console);
    painel_console.append(&pilha_console);

    let console_revealer = gtk::Revealer::builder()
        .transition_type(gtk::RevealerTransitionType::SlideDown)
        .transition_duration(220)
        .reveal_child(true)
        .valign(gtk::Align::End)
        .child(&painel_console)
        .build();

    let direita = gtk::Box::new(gtk::Orientation::Vertical, 0);
    direita.append(&trilha);
    direita.append(&scroll_editor);
    direita.append(&console_revealer);

    let corpo = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    corpo.append(&lateral);
    corpo.append(&direita);

    // ---- rodapé: progresso + toast ----------------------------------------
    let progresso = gtk::ProgressBar::new();
    progresso.add_css_class("trabalho");
    progresso.set_valign(gtk::Align::End);
    progresso.set_halign(gtk::Align::Fill);
    progresso.set_size_request(-1, 2);
    // sempre visível (opacidade 0 parado): esconder o widget faz o GTK medir
    // a gizmo de progresso com largura zero e reclamar no stderr
    progresso.set_opacity(0.0);

    let toast_caixa = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    toast_caixa.add_css_class("toast");
    let toast_rotulo = gtk::Label::new(None);
    toast_rotulo.set_max_width_chars(64);
    toast_rotulo.set_ellipsize(gtk::pango::EllipsizeMode::End);
    toast_caixa.append(&toast_rotulo);

    let toast_revealer = gtk::Revealer::builder()
        .transition_type(gtk::RevealerTransitionType::Crossfade)
        .transition_duration(160)
        .reveal_child(false)
        .valign(gtk::Align::End)
        .halign(gtk::Align::Center)
        .margin_bottom(20)
        .margin_start(16)
        .margin_end(16)
        .child(&toast_caixa)
        .build();

    let overlay = gtk::Overlay::new();
    overlay.set_child(Some(&corpo));
    overlay.add_overlay(&toast_revealer);
    overlay.add_overlay(&progresso);
    janela.set_child(Some(&overlay));

    // ---- estado da tela ----------------------------------------------------
    let cores = Cores::criar(&editor.buffer(), &console_buffer, &tema.borrow());
    let tela = Tela {
        janela: janela.clone(),
        provider: provider.clone(),
        ponto,
        status,
        spinner,
        botao_anexar: botao_anexar.clone(),
        botao_exec: botao_exec.clone(),
        botao_console: botao_console.clone(),
        progresso: progresso.clone(),
        toast: Toast {
            caixa: toast_caixa.clone(),
            rotulo: toast_rotulo.clone(),
            revealer: toast_revealer.clone(),
            geracao: Rc::new(Cell::new(0)),
        },
        console_revealer: console_revealer.clone(),
        pilha_console: pilha_console.clone(),
        console_buffer: console_buffer.clone(),
        console_view: console.clone(),
        editor: editor.clone(),
        lista: lista.clone(),
        pilha_lista: pilha_lista.clone(),
        trilha_nome: trilha_nome.clone(),
        marca_sujo: marca_sujo.clone(),
        total: total.clone(),
        cores: cores.clone(),
        visual: Rc::new(Cell::new(Conexao::Off)),
    };

    aplicar_tema(&tela, &tema.borrow());
    definir_status(&tela, Conexao::Off);

    // ---- console: limpar/fechar + linha atual ------------------------------
    {
        let tela = tela.clone();
        b_limpar.connect_clicked(move |_| {
            tela.console_buffer.set_text("");
            tela.pilha_console.set_visible_child_name("vazio");
        });
    }
    {
        let tela = tela.clone();
        b_fechar.connect_clicked(move |_| {
            tela.botao_console.set_active(false);
        });
    }
    {
        let tela = tela.clone();
        tela.botao_console.connect_toggled(move |b| {
            let aberto = b.is_active();
            tela.console_revealer.set_reveal_child(aberto);
        });
    }
    {
        let buffer = editor.buffer();
        let tela_linha = tela.clone();
        buffer.connect_mark_set(move |b, _, mark| {
            if mark.name().as_deref() != Some("insert") {
                return;
            }
            pintar_linha(b, &tela_linha.cores.linha);
        });
    }
    {
        let buffer = editor.buffer();
        let doc = doc.clone();
        let tela_doc = tela.clone();
        buffer.connect_changed(move |_| {
            let mut d = doc.borrow_mut();
            if !d.sujo {
                d.sujo = true;
                tela_doc.marca_sujo.set_visible(true);
            }
        });
    }

    // ---- scripts: seleção, busca, novo, salvar, excluir --------------------
    {
        let tela = tela.clone();
        let doc = doc.clone();
        lista.connect_row_selected(move |_, row| {
            let Some(row) = row else { return };
            let caminho = PathBuf::from(row.widget_name().to_string());
            if doc.borrow().caminho.as_deref() == Some(caminho.as_path()) {
                return;
            }
            match std::fs::read_to_string(&caminho) {
                Ok(txt) => {
                    let buffer = tela.editor.buffer();
                    buffer.set_text(&txt);
                    {
                        let mut d = doc.borrow_mut();
                        d.caminho = Some(caminho);
                        d.sujo = false;
                    }
                    tela.marca_sujo.set_visible(false);
                    atualizar_trilha(&tela, &doc);
                    pintar_linha(&buffer, &tela.cores.linha);
                }
                Err(e) => tela.toast.mostrar(&format!("could not open: {e}"), true),
            }
        });
    }
    {
        let termo = Rc::new(RefCell::new(String::new()));
        let termo_filtro = termo.clone();
        lista.set_filter_func(move |row| {
            let t = termo_filtro.borrow();
            if t.is_empty() {
                return true;
            }
            row.widget_name().to_lowercase().contains(t.as_str())
        });
        let tela_busca = tela.clone();
        busca.connect_search_changed(move |b| {
            *termo.borrow_mut() = b.text().to_lowercase();
            tela_busca.lista.invalidate_filter();
        });
    }
    {
        let tela = tela.clone();
        let doc = doc.clone();
        b_novo.connect_clicked(move |_| novo_script(&tela, &doc));
    }
    {
        let tela = tela.clone();
        let doc = doc.clone();
        vazio_botao.connect_clicked(move |_| novo_script(&tela, &doc));
    }
    {
        let tela = tela.clone();
        let doc = doc.clone();
        b_salvar.connect_clicked(move |_| salvar(&tela, &doc));
    }
    {
        let tela = tela.clone();
        let doc = doc.clone();
        b_excluir.connect_clicked(move |_| excluir(&tela, &doc));
    }

    // ---- atalhos -----------------------------------------------------------
    {
        let tela = tela.clone();
        let doc = doc.clone();
        let atalhos = gtk::EventControllerKey::new();
        atalhos.set_propagation_phase(gtk::PropagationPhase::Capture);
        atalhos.connect_key_pressed(move |_, tecla, _, mods| {
            if !mods.contains(gdk::ModifierType::CONTROL_MASK) {
                return glib::Propagation::Proceed;
            }
            match tecla {
                gdk::Key::Return | gdk::Key::KP_Enter => {
                    if tela.botao_exec.is_sensitive() {
                        tela.botao_exec.activate();
                    }
                    glib::Propagation::Stop
                }
                gdk::Key::s | gdk::Key::S => {
                    salvar(&tela, &doc);
                    glib::Propagation::Stop
                }
                gdk::Key::n | gdk::Key::N => {
                    novo_script(&tela, &doc);
                    glib::Propagation::Stop
                }
                gdk::Key::grave => {
                    let aberto = !tela.botao_console.is_active();
                    tela.botao_console.set_active(aberto);
                    glib::Propagation::Stop
                }
                _ => glib::Propagation::Proceed,
            }
        });
        janela.add_controller(atalhos);
    }

    // ---- conectar (manual e automático) ------------------------------------
    {
        let tela = tela.clone();
        let ctx = ctx.clone();
        let tx_conexao = tx_conexao.clone();
        botao_anexar.connect_clicked(move |_| {
            definir_status(&tela, Conexao::Conectando);
            // reconecta do zero: o cliente pode ter reiniciado
            if let Ok(mut guardo) = ctx.motor.try_lock() {
                let _ = guardo.take();
            }
            iniciar_conexao(&ctx, &tx_conexao, true);
        });
    }
    {
        let tela = tela.clone();
        let ctx = ctx.clone();
        let tx = tx.clone();
        let ultimo_aviso = Rc::new(Cell::new(Instant::now() - Duration::from_secs(120)));
        glib::timeout_add_local(Duration::from_millis(100), move || {
            while let Ok((manual, r)) = rx_conexao.try_recv() {
                ctx.conectando.store(false, Ordering::SeqCst);
                match r {
                    Ok(m) => {
                        let pid = m.pid;
                        let resumo = format!(
                            "pid {} · luau_load +{:#x} · luaD_call +{:#x}",
                            pid,
                            m.anchors.luau_load.unwrap_or(0),
                            m.anchors.lua_d_call.unwrap_or(0)
                        );
                        let mut instalado = false;
                        if let Ok(mut guardo) = ctx.motor.lock() {
                            if guardo.is_none() {
                                *guardo = Some(m);
                                instalado = true;
                            }
                        }
                        if instalado {
                            let _ = tx.send(format!("[conectado] {resumo}"));
                            definir_status(&tela, Conexao::Conectado(pid));
                            if manual {
                                tela.toast.mostrar(&format!("connected · pid {pid}"), false);
                            } else {
                                tela.toast.mostrar(&format!("client found · pid {pid}"), false);
                            }
                        }
                    }
                    Err(e) => {
                        // reconexão automática sem cliente é rotina: não enche o console
                        let avisar = manual || ultimo_aviso.get().elapsed() > Duration::from_secs(60);
                        if avisar {
                            ultimo_aviso.set(Instant::now());
                            let _ = tx.send(format!("[erro] {e}"));
                        }
                        if manual {
                            tela.toast.mostrar("client not found", true);
                        }
                        if tela.visual.get() != Conexao::Executando {
                            definir_status(&tela, Conexao::Off);
                        }
                    }
                }
            }
            glib::ControlFlow::Continue
        });
    }
    {
        let tela = tela.clone();
        let ctx = ctx.clone();
        let tx = tx.clone();
        let tx_conexao = tx_conexao.clone();
        glib::timeout_add_seconds_local(2, move || {
            if ctx.rodando.load(Ordering::SeqCst) || ctx.conectando.load(Ordering::SeqCst) {
                return glib::ControlFlow::Continue;
            }
            let mut sumiu = None;
            let mut falta = false;
            if let Ok(mut guardo) = ctx.motor.try_lock() {
                let morto = guardo
                    .as_ref()
                    .map(|m| m.pid)
                    .filter(|pid| !pid_vivo(*pid));
                if let Some(pid) = morto {
                    *guardo = None;
                    sumiu = Some(pid);
                }
                falta = guardo.is_none();
            }
            if let Some(pid) = sumiu {
                let _ = tx.send(format!("[cliente] process {pid} exited"));
                definir_status(&tela, Conexao::Off);
                tela.toast.mostrar("client disconnected", true);
            }
            if falta {
                // silencioso: o status só muda quando o cliente aparecer
                iniciar_conexao(&ctx, &tx_conexao, false);
            }
            glib::ControlFlow::Continue
        });
    }

    // ---- executar ----------------------------------------------------------
    {
        let tela = tela.clone();
        let ctx = ctx.clone();
        let tx = tx.clone();
        let tx_estado = tx_estado.clone();
        botao_exec.connect_clicked(move |_| {
            let codigo = texto_do_editor(&tela.editor);
            if codigo.trim().is_empty() {
                tela.toast.mostrar("script is empty", true);
                return;
            }
            if ctx.rodando.swap(true, Ordering::SeqCst) {
                tela.toast.mostrar("an execution is already running", true);
                return;
            }
            definir_status(&tela, Conexao::Executando);
            let tx = tx.clone();
            let tx_estado = tx_estado.clone();
            let motor = ctx.motor.clone();
            let rodando = ctx.rodando.clone();
            std::thread::spawn(move || {
                let _ocupado = Ocupado(rodando);
                let _ = tx.send(format!("» executando {} bytes", codigo.len()));
                let mut guardo = match motor.lock() {
                    Ok(g) => g,
                    Err(_) => {
                        let _ = tx.send("[erro] engine slot stuck".to_string());
                        let _ = tx_estado.send(EstadoCli::Falha {
                            mensagem: "engine slot stuck".into(),
                            conectado: None,
                        });
                        return;
                    }
                };
                // tira o motor do compartimento: a thread do GTK não pode ficar
                // esperando um lock segurado durante a execução inteira
                let mut m = match guardo.take() {
                    Some(m) => m,
                    None => match Motor::conectar() {
                        Ok(m) => m,
                        Err(e) => {
                            drop(guardo);
                            let _ = tx.send(format!("[erro] {e}"));
                            let _ = tx_estado.send(EstadoCli::Falha {
                                mensagem: format!("client unavailable: {e}"),
                                conectado: None,
                            });
                            return;
                        }
                    },
                };
                drop(guardo);
                let pid = m.pid;

                let mut ultimo = String::new();
                for tentativa in 1..=TENTATIVAS {
                    if tentativa > 1 {
                        let _ = tx.send(format!("[retry] attempt {tentativa}/{TENTATIVAS}"));
                        std::thread::sleep(Duration::from_millis(300));
                    }
                    let mut log = Vec::new();
                    match m.executar_com_log(&codigo, &mut log) {
                        Ok(()) => {
                            for l in log {
                                let _ = tx.send(format!("  {l}"));
                            }
                            let _ = tx.send("» done".to_string());
                            devolver(&motor, m);
                            let _ = tx_estado.send(EstadoCli::Conectado(pid));
                            return;
                        }
                        Err(e) => {
                            for l in log {
                                let _ = tx.send(format!("  {l}"));
                            }
                            ultimo = e.to_string();
                            let _ = tx.send(format!(
                                "[erro] attempt {tentativa}/{TENTATIVAS}: {ultimo}"
                            ));
                        }
                    }
                }
                let _ = tx.send(format!("[erro] all {TENTATIVAS} attempts failed: {ultimo}"));
                devolver(&motor, m);
                let _ = tx_estado.send(EstadoCli::Falha {
                    mensagem: format!("all {TENTATIVAS} attempts failed"),
                    conectado: Some(pid),
                });
            });
        });
    }
    {
        let tela = tela.clone();
        glib::timeout_add_local(Duration::from_millis(100), move || {
            while let Ok(e) = rx_estado.try_recv() {
                match e {
                    EstadoCli::Conectado(pid) => definir_status(&tela, Conexao::Conectado(pid)),
                    EstadoCli::Falha {
                        mensagem,
                        conectado,
                    } => {
                        tela.toast.mostrar(&mensagem, true);
                        match conectado {
                            Some(pid) => definir_status(&tela, Conexao::Conectado(pid)),
                            None => definir_status(&tela, Conexao::Off),
                        }
                    }
                }
            }
            glib::ControlFlow::Continue
        });
    }
    {
        // progresso indeterminado só enquanto o motor trabalha; sem layout shift
        let tela = tela.clone();
        let ctx = ctx.clone();
        glib::timeout_add_local(Duration::from_millis(120), move || {
            if ctx.rodando.load(Ordering::SeqCst) {
                if tela.progresso.opacity() < 1.0 {
                    tela.progresso.set_opacity(1.0);
                }
                tela.progresso.pulse();
            } else {
                if tela.progresso.opacity() > 0.0 {
                    tela.progresso.set_opacity(0.0);
                }
                // se a thread sumir sem avisar, não deixa "executando" preso
                if tela.visual.get() == Conexao::Executando {
                    let pid = ctx
                        .motor
                        .try_lock()
                        .ok()
                        .and_then(|g| g.as_ref().map(|m| m.pid));
                    match pid {
                        Some(pid) => definir_status(&tela, Conexao::Conectado(pid)),
                        None => definir_status(&tela, Conexao::Off),
                    }
                }
            }
            glib::ControlFlow::Continue
        });
    }

    // ---- console: fila da UI + log do cliente ------------------------------
    let fila: Arc<Mutex<VecDeque<Linha>>> = Arc::new(Mutex::new(VecDeque::new()));
    {
        let fila = fila.clone();
        std::thread::spawn(move || {
            for linha in rx {
                let estilo = if linha.starts_with('»') {
                    Estilo::Script
                } else if linha.starts_with("[erro]") || linha.starts_with("[retry]") {
                    Estilo::Erro
                } else {
                    Estilo::Info
                };
                if let Ok(mut f) = fila.lock() {
                    f.push_back(Linha { estilo, texto: linha });
                    if f.len() > MAX_LINHAS {
                        f.drain(0..1000);
                    }
                }
            }
        });
    }
    {
        let fila = fila.clone();
        std::thread::spawn(move || {
            let mut vistos: VecDeque<String> = VecDeque::new();
            loop {
                if let Ok(txt) = std::fs::read_to_string("/tmp/luaria-client.log") {
                    let janela: Vec<&str> = txt.lines().rev().take(400).collect();
                    let mut novas = Vec::new();
                    for l in janela.into_iter().rev() {
                        if RUIDO.iter().any(|r| l.contains(r)) {
                            continue;
                        }
                        let Some((estilo, texto)) = classificar_log(l) else {
                            continue;
                        };
                        if vistos.iter().any(|v| v == &texto) {
                            continue;
                        }
                        vistos.push_back(texto.clone());
                        if vistos.len() > 800 {
                            vistos.pop_front();
                        }
                        novas.push(Linha { estilo, texto });
                    }
                    if !novas.is_empty() {
                        if let Ok(mut f) = fila.lock() {
                            f.extend(novas);
                            if f.len() > MAX_LINHAS {
                                f.drain(0..1000);
                            }
                        }
                    }
                }
                std::thread::sleep(Duration::from_secs(2));
            }
        });
    }
    {
        let tela = tela.clone();
        glib::timeout_add_local(Duration::from_millis(180), move || {
            let linhas: Vec<Linha> = match fila.lock() {
                Ok(mut f) if !f.is_empty() => f.drain(..).collect(),
                _ => return glib::ControlFlow::Continue,
            };
            for linha in linhas {
                let tag = match linha.estilo {
                    Estilo::Script => &tela.cores.script,
                    Estilo::Info => &tela.cores.info,
                    Estilo::Erro => &tela.cores.erro,
                    Estilo::Secundario => &tela.cores.secundario,
                };
                let mut fim = tela.console_buffer.end_iter();
                tela.console_buffer
                    .insert_with_tags(&mut fim, &format!("{}\n", linha.texto), &[tag]);
            }
            tela.pilha_console.set_visible_child_name("log");
            let mut fim = tela.console_buffer.end_iter();
            tela.console_view
                .scroll_to_iter(&mut fim, 0.0, false, 0.0, 1.0);
            glib::ControlFlow::Continue
        });
    }

    // ---- aparência: popover de configurações -------------------------------
    {
        let popover = montar_config(&tela, &tema);
        botao_config.set_popover(Some(&popover));
    }
    {
        // tema do sistema pode mudar com o app aberto
        if let Some(settings) = gtk::Settings::default() {
            let tela = tela.clone();
            let tema = tema.clone();
            settings.connect_gtk_theme_name_notify(move |_| {
                if tema.borrow().modo == Modo::Sistema {
                    aplicar_tema(&tela, &tema.borrow());
                }
            });
        }
    }

    atualizar_trilha(&tela, &doc);
    carregar_scripts(&tela, &doc);
    janela.present();
}

fn botao_icone(nome: &str, dica: &str) -> gtk::Button {
    let b = gtk::Button::from_icon_name(nome);
    b.add_css_class("icone");
    b.set_tooltip_text(Some(dica));
    b
}

/// Devolve o motor ao compartimento sem atropelar um "Anexar" que tenha
/// conectado outro enquanto o script rodava.
fn devolver(motor: &Arc<Mutex<Option<Motor>>>, m: Motor) {
    if let Ok(mut guardo) = motor.lock() {
        if guardo.is_none() {
            *guardo = Some(m);
        }
    }
}

fn iniciar_conexao(ctx: &MotorCtx, tx: &Sender<(bool, Result<Motor, String>)>, anunciar: bool) {
    if ctx.conectando.swap(true, Ordering::SeqCst) {
        return;
    }
    let tx = tx.clone();
    std::thread::spawn(move || {
        let r = Motor::conectar().map_err(|e| e.to_string());
        let _ = tx.send((anunciar, r));
    });
}

fn pid_vivo(pid: i32) -> bool {
    Path::new(&format!("/proc/{pid}")).exists()
}

fn definir_status(tela: &Tela, estado: Conexao) {
    tela.visual.set(estado);
    tela.ponto.remove_css_class("on");
    tela.ponto.remove_css_class("busy");
    tela.ponto.set_visible(true);
    tela.spinner.set_visible(false);
    tela.botao_anexar.remove_css_class("primario");
    tela.botao_anexar.add_css_class("secundario");
    tela.botao_exec.remove_css_class("primario");
    tela.botao_exec.add_css_class("secundario");
    match estado {
        Conexao::Off => {
            tela.status.set_text("disconnected");
            tela.botao_exec.set_sensitive(false);
            tela.botao_anexar.set_sensitive(true);
            // desconectado: o CTA é anexar
            tela.botao_anexar.remove_css_class("secundario");
            tela.botao_anexar.add_css_class("primario");
        }
        Conexao::Conectando => {
            tela.status.set_text("connecting…");
            tela.ponto.set_visible(false);
            tela.spinner.set_visible(true);
            tela.botao_exec.set_sensitive(false);
            tela.botao_anexar.set_sensitive(false);
        }
        Conexao::Conectado(pid) => {
            tela.status.set_text(&format!("connected · pid {pid}"));
            tela.ponto.add_css_class("on");
            tela.botao_exec.set_sensitive(true);
            tela.botao_exec.remove_css_class("secundario");
            tela.botao_exec.add_css_class("primario");
            tela.botao_anexar.set_sensitive(true);
        }
        Conexao::Executando => {
            tela.status.set_text("running…");
            tela.ponto.set_visible(false);
            tela.ponto.add_css_class("busy");
            tela.spinner.set_visible(true);
            tela.botao_exec.set_sensitive(false);
            tela.botao_anexar.set_sensitive(false);
        }
    }
}

fn aplicar_tema(tela: &Tela, tema: &Tema) {
    tela.provider.load_from_data(&tema.css());
    if tema.densidade == Densidade::Compacta {
        tela.janela.add_css_class("compacta");
    } else {
        tela.janela.remove_css_class("compacta");
    }
    let (margem_v, margem_h) = if tema.densidade == Densidade::Compacta {
        (8, 18)
    } else {
        (18, 28)
    };
    tela.editor.set_top_margin(margem_v);
    tela.editor.set_bottom_margin(margem_v);
    tela.editor.set_left_margin(margem_h);
    tela.editor.set_right_margin(margem_h);
    tela.cores.atualizar(tema);
}

fn montar_config(tela: &Tela, tema: &Rc<RefCell<Tema>>) -> gtk::Popover {
    let caixa = gtk::Box::new(gtk::Orientation::Vertical, 10);
    caixa.add_css_class("cfg-secao");
    caixa.set_size_request(290, -1);

    let titulo = gtk::Label::new(Some("APPEARANCE"));
    titulo.add_css_class("config-titulo");
    titulo.set_xalign(0.0);
    caixa.append(&titulo);

    let botao_cor = gtk::ColorDialogButton::new(Some(gtk::ColorDialog::new()));
    botao_cor.set_rgba(
        &gdk::RGBA::parse(tema.borrow().acento.as_str())
            .unwrap_or_else(|_| gdk::RGBA::new(0.298, 0.553, 1.0, 1.0)),
    );
    botao_cor.set_tooltip_text(Some("interface accent color"));
    caixa.append(&linha_config("Accent color", &botao_cor));

    let seletor_tema = gtk::DropDown::from_strings(&["Dark", "Light", "System"]);
    seletor_tema.set_selected(match tema.borrow().modo {
        Modo::Escuro => 0,
        Modo::Claro => 1,
        Modo::Sistema => 2,
    });
    caixa.append(&linha_config("Theme", &seletor_tema));

    let fonte = gtk::SpinButton::with_range(10.0, 20.0, 1.0);
    fonte.set_numeric(true);
    fonte.set_value(tema.borrow().fonte_editor as f64);
    caixa.append(&linha_config("Editor font", &fonte));

    let seletor_dens = gtk::DropDown::from_strings(&["Comfortable", "Compact"]);
    seletor_dens.set_selected(match tema.borrow().densidade {
        Densidade::Confortavel => 0,
        Densidade::Compacta => 1,
    });
    caixa.append(&linha_config("Density", &seletor_dens));

    let separador = gtk::Separator::new(gtk::Orientation::Horizontal);
    caixa.append(&separador);

    let restaurar = gtk::Button::with_label("Restore defaults");
    restaurar.add_css_class("secundario");
    caixa.append(&restaurar);

    {
        let tela = tela.clone();
        let tema = tema.clone();
        botao_cor.connect_rgba_notify(move |b| {
            let cor = hex_de_rgba(&b.rgba());
            let mudou = {
                let mut t = tema.borrow_mut();
                if t.acento == cor {
                    false
                } else {
                    t.acento = cor;
                    true
                }
            };
            if mudou {
                persistir(&tela, &tema);
            }
        });
    }
    {
        let tela = tela.clone();
        let tema = tema.clone();
        seletor_tema.connect_selected_notify(move |d| {
            let modo = match d.selected() {
                1 => Modo::Claro,
                2 => Modo::Sistema,
                _ => Modo::Escuro,
            };
            let mudou = {
                let mut t = tema.borrow_mut();
                if t.modo == modo {
                    false
                } else {
                    t.modo = modo;
                    true
                }
            };
            if mudou {
                persistir(&tela, &tema);
            }
        });
    }
    {
        let tela = tela.clone();
        let tema = tema.clone();
        fonte.connect_value_changed(move |f| {
            let tamanho = f.value().round() as i32;
            let mudou = {
                let mut t = tema.borrow_mut();
                if t.fonte_editor == tamanho {
                    false
                } else {
                    t.fonte_editor = tamanho;
                    true
                }
            };
            if mudou {
                persistir(&tela, &tema);
            }
        });
    }
    {
        let tela = tela.clone();
        let tema = tema.clone();
        seletor_dens.connect_selected_notify(move |d| {
            let densidade = if d.selected() == 1 {
                Densidade::Compacta
            } else {
                Densidade::Confortavel
            };
            let mudou = {
                let mut t = tema.borrow_mut();
                if t.densidade == densidade {
                    false
                } else {
                    t.densidade = densidade;
                    true
                }
            };
            if mudou {
                persistir(&tela, &tema);
            }
        });
    }
    {
        let tela = tela.clone();
        let tema = tema.clone();
        let botao_cor = botao_cor.clone();
        let seletor_tema = seletor_tema.clone();
        let fonte = fonte.clone();
        let seletor_dens = seletor_dens.clone();
        restaurar.connect_clicked(move |_| {
            let padrao = Tema::default();
            *tema.borrow_mut() = padrao.clone();
            // mexer nos controles dispara os notifies; eles regravam o padrão
            botao_cor.set_rgba(
                &gdk::RGBA::parse(padrao.acento.as_str())
                    .unwrap_or_else(|_| gdk::RGBA::new(0.298, 0.553, 1.0, 1.0)),
            );
            seletor_tema.set_selected(0);
            fonte.set_value(padrao.fonte_editor as f64);
            seletor_dens.set_selected(0);
            persistir(&tela, &tema);
        });
    }

    let popover = gtk::Popover::new();
    popover.add_css_class("config");
    popover.set_child(Some(&caixa));
    popover
}

fn linha_config(rotulo: &str, controle: &impl IsA<gtk::Widget>) -> gtk::Box {
    let linha = gtk::Box::new(gtk::Orientation::Horizontal, 16);
    let l = gtk::Label::new(Some(rotulo));
    l.add_css_class("cfg-rotulo");
    l.set_xalign(0.0);
    l.set_hexpand(true);
    linha.append(&l);
    linha.append(controle);
    linha
}

fn persistir(tela: &Tela, tema: &Rc<RefCell<Tema>>) {
    let t = tema.borrow().clone();
    t.salvar();
    aplicar_tema(tela, &t);
}

fn hex_de_rgba(c: &gdk::RGBA) -> String {
    let canal = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
    format!("#{:02X}{:02X}{:02X}", canal(c.red()), canal(c.green()), canal(c.blue()))
}

fn texto_do_editor(editor: &gtk::TextView) -> String {
    let b = editor.buffer();
    b.text(&b.start_iter(), &b.end_iter(), false).to_string()
}

/// Realce da linha do cursor: pinta o parágrafo inteiro, não só o texto.
fn pintar_linha(buffer: &gtk::TextBuffer, tag: &gtk::TextTag) {
    buffer.remove_tag(tag, &buffer.start_iter(), &buffer.end_iter());
    let pos = buffer.cursor_position();
    let mut inicio = buffer.iter_at_offset(pos);
    inicio.set_line_offset(0);
    let mut fim = inicio;
    if !fim.ends_line() {
        fim.forward_to_line_end();
    }
    fim.forward_char();
    buffer.apply_tag(tag, &inicio, &fim);
}

fn atualizar_trilha(tela: &Tela, doc: &Rc<RefCell<Documento>>) {
    let texto = match doc.borrow().caminho.as_ref().and_then(|p| p.file_name()) {
        Some(nome) => format!("scripts / {}", nome.to_string_lossy()),
        None => "no file".to_string(),
    };
    tela.trilha_nome.set_text(&texto);
}

fn proximo_caminho() -> PathBuf {
    let dir = scripts_dir();
    let _ = std::fs::create_dir_all(&dir);
    let mut n = 1;
    loop {
        let c = dir.join(format!("script{n}.lua"));
        if !c.exists() {
            return c;
        }
        n += 1;
    }
}

fn novo_script(tela: &Tela, doc: &Rc<RefCell<Documento>>) {
    let caminho = proximo_caminho();
    if let Err(e) = std::fs::write(&caminho, "-- new script\n") {
        tela.toast.mostrar(&format!("could not create: {e}"), true);
        return;
    }
    let buffer = tela.editor.buffer();
    buffer.set_text("-- new script\n");
    {
        let mut d = doc.borrow_mut();
        d.caminho = Some(caminho.clone());
        d.sujo = false;
    }
    tela.marca_sujo.set_visible(false);
    atualizar_trilha(tela, doc);
    carregar_scripts(tela, doc);
    selecionar_caminho(tela, &caminho);
    pintar_linha(&buffer, &tela.cores.linha);
}

fn salvar(tela: &Tela, doc: &Rc<RefCell<Documento>>) {
    let codigo = texto_do_editor(&tela.editor);
    let caminho = {
        let mut d = doc.borrow_mut();
        if d.caminho.is_none() {
            d.caminho = Some(proximo_caminho());
        }
        d.caminho.clone().expect("caminho definido acima")
    };
    match std::fs::write(&caminho, codigo) {
        Ok(()) => {
            doc.borrow_mut().sujo = false;
            tela.marca_sujo.set_visible(false);
            atualizar_trilha(tela, doc);
            carregar_scripts(tela, doc);
            selecionar_caminho(tela, &caminho);
            let nome = caminho
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_default();
            tela.toast.mostrar(&format!("saved · {nome}"), false);
        }
        Err(e) => tela.toast.mostrar(&format!("could not save: {e}"), true),
    }
}

fn excluir(tela: &Tela, doc: &Rc<RefCell<Documento>>) {
    let Some(caminho) = doc.borrow().caminho.clone() else {
        tela.toast.mostrar("no script open", true);
        return;
    };
    let nome = caminho
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_default();

    let alerta = gtk::AlertDialog::builder()
        .message("Delete script?")
        .detail(format!("{nome} will be removed from disk."))
        .build();
    alerta.set_buttons(&["Cancel", "Delete"]);
    alerta.set_cancel_button(0);
    alerta.set_default_button(0);

    let tela = tela.clone();
    let doc = doc.clone();
    glib::spawn_future_local(async move {
        let Ok(escolha) = alerta.choose_future(Some(&tela.janela)).await else {
            return;
        };
        if escolha != 1 {
            return;
        }
        if let Err(e) = std::fs::remove_file(&caminho) {
            tela.toast.mostrar(&format!("could not delete: {e}"), true);
            return;
        }
        {
            let mut d = doc.borrow_mut();
            if d.caminho.as_deref() == Some(caminho.as_path()) {
                d.caminho = None;
                d.sujo = false;
            }
        }
        tela.editor.buffer().set_text("");
        tela.marca_sujo.set_visible(false);
        atualizar_trilha(&tela, &doc);
        carregar_scripts(&tela, &doc);
        tela.toast.mostrar("script deleted", false);
    });
}

/// Puxa a mensagem do log do cliente e diz como ela deve aparecer.
fn classificar_log(l: &str) -> Option<(Estilo, String)> {
    let depois = |tag: &str| -> Option<String> {
        let p = l.find(tag)? + tag.len();
        Some(l[p..].trim().to_string())
    };
    if let Some(t) = depois("[FLog::CreatorOutput]") {
        return Some((Estilo::Script, format!("» {t}")));
    }
    if let Some(t) = depois("[FLog::Output]") {
        return Some((Estilo::Secundario, format!("  {t}")));
    }
    if l.contains("[FLog::Warning]") || l.contains("[FLog::Error]") {
        if let Some(t) = depois("[FLog::") {
            let texto = t.split_once(']').map(|(_, r)| r.trim()).unwrap_or("");
            if !texto.is_empty() {
                return Some((Estilo::Secundario, format!("  {texto}")));
            }
        }
    }
    if l.contains("LUARIA") {
        return Some((Estilo::Info, format!("  {}", l.trim())));
    }
    None
}

fn scripts_dir() -> PathBuf {
    if let Ok(dir) = std::env::var("LUARIA_SCRIPTS") {
        return PathBuf::from(dir);
    }
    let home = std::env::var("HOME").unwrap_or_else(|_| ".".into());
    PathBuf::from(home).join(".local/share/luaria/scripts")
}

fn selecionar_caminho(tela: &Tela, caminho: &Path) {
    let alvo = caminho.to_string_lossy().to_string();
    let mut i = 0;
    while let Some(row) = tela.lista.row_at_index(i) {
        if row.widget_name().as_str() == alvo.as_str() {
            tela.lista.select_row(Some(&row));
            return;
        }
        i += 1;
    }
}

/// Relê a pasta de scripts sem tocar no texto nem na seleção atual.
fn carregar_scripts(tela: &Tela, doc: &Rc<RefCell<Documento>>) {
    let atual = doc.borrow().caminho.clone();
    while let Some(filho) = tela.lista.first_child() {
        tela.lista.remove(&filho);
    }
    let dir = scripts_dir();
    let _ = std::fs::create_dir_all(&dir);
    let mut nomes: Vec<PathBuf> = std::fs::read_dir(&dir)
        .map(|entradas| {
            entradas
                .flatten()
                .map(|e| e.path())
                .filter(|p| p.extension().map(|e| e == "lua").unwrap_or(false))
                .collect()
        })
        .unwrap_or_default();
    nomes.sort();

    if nomes.is_empty() {
        tela.pilha_lista.set_visible_child_name("vazio");
        tela.total.set_text("0 scripts");
        return;
    }
    tela.pilha_lista.set_visible_child_name("lista");
    tela.total.set_text(&format!(
        "{} script{}",
        nomes.len(),
        if nomes.len() == 1 { "" } else { "s" }
    ));

    for caminho in &nomes {
        let rotulo = caminho
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .to_string();
        let linha = gtk::ListBoxRow::new();
        linha.set_widget_name(&caminho.to_string_lossy());
        linha.set_tooltip_text(Some(&caminho.to_string_lossy()));
        let l = gtk::Label::new(Some(&rotulo));
        l.set_xalign(0.0);
        l.set_ellipsize(gtk::pango::EllipsizeMode::End);
        linha.set_child(Some(&l));
        tela.lista.append(&linha);
    }
    tela.lista.invalidate_filter();

    let alvo = atual
        .filter(|p| nomes.iter().any(|n| n == p))
        .or_else(|| nomes.first().cloned());
    if let Some(alvo) = alvo {
        selecionar_caminho(tela, &alvo);
    }
}
