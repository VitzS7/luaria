// Atalhos de abas. Valem também com o foco no CodeMirror: o editor não usa
// Ctrl+T, Ctrl+W, Ctrl+1..9 nem Ctrl+Tab, então não há conflito.

import { ativarPorPosicao, ciclarAba, fecharAbaAtiva, novoRascunho } from "./abas.js";
import { irPara } from "./navegacao.js";

/** Trata o atalho e devolve true quando ele é de abas (o chamador para aqui). */
export function atalhoDeAbas(evento) {
  if (!(evento.ctrlKey || evento.metaKey)) return false;
  const tecla = evento.key.toLowerCase();
  if (tecla === "tab") ciclarAba(evento.shiftKey ? -1 : 1);
  else if (/^[1-9]$/.test(evento.key)) ativarPorPosicao(Number(evento.key));
  else if (tecla === "t" && !evento.shiftKey) novoRascunho();
  else if (tecla === "w" && !evento.shiftKey) fecharAbaAtiva();
  else return false;
  evento.preventDefault();
  irPara("editor");
  return true;
}
