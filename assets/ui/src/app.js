// Ponto de entrada da UI. Liga os módulos, carrega o tema e sobe as sondagens.

import { $, api, postar } from "./util.js";
import { estado } from "./estado.js";
import { criarEditor, focarEditor } from "./editor.js";
import { abaAtiva, abrirRascunho, acoesEditor, atualizarVazio, ligarMenuAbas, renderAbas } from "./abas.js";
import { aplicarTema, carregarTema } from "./tema.js";
import { VISTAS, irPara, ligarNavegacao, fecharMenu } from "./navegacao.js";
import { ligarArquivos, carregarScripts, abrirScript, salvarAba, novoScript } from "./arquivos.js";
import { ligarConsole, renderConsole, renderProblemas, trocarPainel, pollLog } from "./console.js";
import { anexar, executar, pollStatus, iniciarSondagens } from "./execucao.js";
import { ligarClientes } from "./clientes.js";
import { carregarAtividade, ligarHome, renderRecentes } from "./home.js";
import { ligarScripts, renderGrade } from "./scripts.js";
import { ligarHub } from "./hub.js";
import { ligarAjustes, renderAjustes } from "./ajustes.js";
import { abrirConta, ligarDialogos } from "./conta.js";
import { toast } from "./dialogo.js";
import { atalhoDeAbas } from "./atalhos.js";

const acoesMenu = {
  novo: () => novoScript(),
  anexar: () => anexar(),
  ajustes: () => irPara("settings"),
  conta: () => abrirConta(),
};

/** Atalhos globais. Dentro do editor o próprio CodeMirror cuida deles. */
function ligarTeclado() {
  document.addEventListener("keydown", (evento) => {
    if (evento.key === "Escape") fecharMenu();
    if (atalhoDeAbas(evento)) return;
    if (!(evento.ctrlKey || evento.metaKey) || evento.target.closest(".cm-editor")) return;
    const tecla = evento.key.toLowerCase();
    if (evento.key === "Enter") {
      evento.preventDefault();
      executar();
    } else if (tecla === "s") {
      evento.preventDefault();
      salvarAba(abaAtiva());
    } else if (tecla === "n") {
      evento.preventDefault();
      novoScript();
    }
  });
}

function ligarBotoesEditor() {
  $("#btn-executar").addEventListener("click", executar);
  $("#btn-salvar").addEventListener("click", () => salvarAba(abaAtiva()));
  $("#conexao").addEventListener("click", () => {
    const s = estado.status || {};
    toast(s.erro && !s.conectado ? s.erro : s.conectado ? "client attached" : "client not found");
  });
}

function ligarEditor() {
  criarEditor($("#editor"), acoesEditor);
  ligarBotoesEditor();
  ligarArquivos();
  ligarClientes();
  ligarMenuAbas();
  ligarConsole();
}

function ligarTudo() {
  ligarEditor();
  ligarScripts();
  ligarHub();
  ligarAjustes();
  ligarHome();
  ligarDialogos();
  ligarTeclado();
  ligarNavegacao((acao) => acoesMenu[acao]?.());
}

async function carregarEstado() {
  try {
    estado.tema = await carregarTema();
  } catch {
    estado.tema = { tokens: {}, temas_personalizados: [] };
  }
  aplicarTema(estado.tema);
}

/** Vista e arquivo vindos da linha de comando (LUARIA_VISTA / LUARIA_ARQUIVO). */
async function abrirPelaUrl() {
  const parametros = new URLSearchParams(location.search);
  const vista = parametros.get("vista");
  irPara(VISTAS.includes(vista) ? vista : "home");
  const arquivo = parametros.get("arquivo");
  if (arquivo) await abrirScript(arquivo);
}

async function iniciar() {
  if (new URLSearchParams(location.search).has("cantos")) document.documentElement.classList.add("cantos");
  ligarTudo();
  await carregarEstado();
  renderAjustes();
  await Promise.all([carregarScripts(), pollStatus(), carregarAtividade()]);
  await pollLog();
  renderConsole();
  renderProblemas();
  trocarPainel("output");
  await abrirPelaUrl();
  if (!estado.abas.length) abrirRascunho("Untitled-1");
  renderRecentes();
  renderAbas();
  atualizarVazio();
  iniciarSondagens();
  window.__luariaPronto = true;
}

iniciar().catch((erro) => {
  console.error(erro);
  toast(`failed to start the interface: ${erro.message}`, "erro");
});

// Gancho de inspeção: só com ?dev=1 na URL (usado nos testes da UI).
if (new URLSearchParams(location.search).has("dev")) {
  window.__luaria = { estado, focarEditor, api, postar };
}
