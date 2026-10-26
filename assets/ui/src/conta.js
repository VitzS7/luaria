// Modal de conta e os diálogos globais (confirmação, renomear, Escape). A
// "conta" é só um nome de exibição guardado no ui.json: não há login de verdade.

import { $, $$ } from "./util.js";
import { estado } from "./estado.js";
import { mudarTema } from "./tema.js";
import { toast, abrirModal, fecharModal, modalAberto, confirmarDialogo, fecharDialogo } from "./dialogo.js";

export function abrirConta() {
  $("#conta-nome").value = estado.tema.nome_usuario || "";
  abrirModal($("#modal-conta"));
  $("#conta-nome").focus();
}

function entrarConta(evento) {
  evento.preventDefault();
  const nome = $("#conta-nome").value.trim();
  if (!nome) return toast("type a display name", "erro");
  mudarTema({ nome_usuario: nome });
  fecharModal($("#modal-conta"));
  toast(`signed in as ${nome}`, "ok");
}

function sairConta() {
  mudarTema({ nome_usuario: "" });
  fecharModal($("#modal-conta"));
  toast("local account removed", "info");
}

function ligarDialogoPadrao() {
  $("#dialogo-confirmar").addEventListener("click", confirmarDialogo);
  $("#dialogo-cancelar").addEventListener("click", () => fecharDialogo(false));
  $("#dialogo-fundo").addEventListener("click", (e) => {
    if (e.target.id === "dialogo-fundo") fecharDialogo(false);
  });
  $("#dialogo-campo").addEventListener("keydown", (e) => {
    if (e.key === "Enter") confirmarDialogo();
    if (e.key === "Escape") fecharDialogo(false);
  });
}

/** Escape fecha o modal aberto: diálogo resolve "cancelado", os outros só somem. */
function ligarEscape() {
  document.addEventListener("keydown", (e) => {
    if (e.key !== "Escape") return;
    const aberto = modalAberto();
    if (!aberto) return;
    if (aberto.id === "dialogo-fundo") fecharDialogo(false);
    else fecharModal(aberto);
  });
}

export function ligarDialogos() {
  ligarDialogoPadrao();
  ligarEscape();
  $("#conta-form").addEventListener("submit", entrarConta);
  $("#conta-sair").addEventListener("click", sairConta);
  $("#modal-conta").addEventListener("click", (e) => {
    if (e.target.id === "modal-conta") fecharModal($("#modal-conta"));
  });
  $$("#modal-conta [data-fechar]").forEach((b) =>
    b.addEventListener("click", () => fecharModal($("#modal-conta"))),
  );
}
