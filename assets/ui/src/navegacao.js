// Barra de título, navegação entre telas, menu e controles da janela. A janela
// não tem decoração do GTK, então arrastar e os botões passam pelo servidor.

import { $, $$, postar } from "./util.js";
import { estado } from "./estado.js";
import { toast } from "./dialogo.js";
import { focarEditor } from "./editor.js";

export const VISTAS = ["home", "editor", "scripts", "settings"];

/** A primeira posição não anima: senão o marcador desliza de x=0 ao abrir. */
function posicionarMarcador() {
  const ativo = $("#nav .nav-item.ativo");
  if (!ativo) return;
  const marcador = $("#nav-marcador");
  if (!marcador.dataset.pronto) {
    marcador.style.transition = "none";
    marcador.dataset.pronto = "sim";
    requestAnimationFrame(() => (marcador.style.transition = ""));
  }
  marcador.style.transform = `translateX(${ativo.offsetLeft}px)`;
  marcador.style.width = `${ativo.offsetWidth}px`;
}

export function irPara(vista) {
  if (!VISTAS.includes(vista)) return;
  estado.vista = vista;
  $$(".vista").forEach((v) => v.classList.toggle("ativa", v.dataset.vista === vista));
  $$(".nav-item").forEach((b) => b.classList.toggle("ativo", b.dataset.ir === vista));
  posicionarMarcador();
  if (vista === "editor" && estado.ativa) focarEditor();
}

export function abrirMenu() {
  $("#menu").hidden = false;
  $("#btn-menu").setAttribute("aria-expanded", "true");
  $("#menu-info").textContent = `Luaria v${estado.versao} · ${estado.tema?.nome_usuario || "local"}`;
}

export function fecharMenu() {
  $("#menu").hidden = true;
  $("#btn-menu").setAttribute("aria-expanded", "false");
}

/** Ações de janela vão para o servidor, que repassa à thread GTK. */
async function acaoJanela(corpo) {
  try {
    await postar("/api/janela", corpo);
  } catch (erro) {
    toast(`window: ${erro.message}`, "erro");
  }
}

/** Arrasta pelo espaço vazio da barra; duplo clique alterna maximizar. */
function ligarArraste() {
  $("#barra").addEventListener("mousedown", (evento) => {
    if (evento.button !== 0 || evento.target.closest("button, nav, .conexao, input")) return;
    if (evento.detail > 1) {
      acaoJanela({ acao: "maximizar" });
      return;
    }
    acaoJanela({ acao: "arrastar", x: evento.clientX, y: evento.clientY, botao: 1 });
  });
}

/** Alças de borda: o servidor repassa ao WM o redimensionamento a partir da borda pegada. */
function ligarRedimensionar() {
  document.querySelectorAll(".alca").forEach((alca) => {
    alca.addEventListener("mousedown", (evento) => {
      if (evento.button !== 0) return;
      evento.preventDefault();
      acaoJanela({ acao: "redimensionar", borda: alca.dataset.borda, x: evento.clientX, y: evento.clientY, botao: 1 });
    });
  });
}

function ligarMenu(aoAcao) {
  const botao = $("#btn-menu");
  const menu = $("#menu");
  botao.addEventListener("click", (evento) => {
    evento.stopPropagation();
    if (menu.hidden) abrirMenu();
    else fecharMenu();
  });
  menu.addEventListener("click", (evento) => {
    const item = evento.target.closest("[data-acao]");
    if (!item) return;
    fecharMenu();
    aoAcao(item.dataset.acao);
  });
  document.addEventListener("click", (evento) => {
    if (!menu.hidden && !evento.target.closest("#menu, #btn-menu")) fecharMenu();
  });
}

/** `aoAcao` recebe as ações do menu: novo, anexar, ajustes, conta. */
export function ligarNavegacao(aoAcao) {
  document.addEventListener("click", (evento) => {
    const alvo = evento.target.closest("[data-ir]");
    if (alvo) {
      evento.preventDefault();
      irPara(alvo.dataset.ir);
    }
  });
  ligarMenu(aoAcao);
  $$(".btn-janela").forEach((b) =>
    b.addEventListener("click", () => acaoJanela({ acao: b.dataset.janela })),
  );
  ligarArraste();
  ligarRedimensionar();
  window.addEventListener("resize", posicionarMarcador);
  document.fonts?.ready.then(posicionarMarcador).catch(() => {});
}
