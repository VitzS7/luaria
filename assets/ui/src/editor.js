// CodeMirror 6 com Luau: realce, marcas de erro no gutter e atalhos do
// executor. O módulo guarda a instância única; abas trocam o documento.

import { EditorView, keymap, lineNumbers, highlightActiveLine, highlightActiveLineGutter,
  drawSelection, highlightSpecialChars, gutter, GutterMarker } from "@codemirror/view";
import { EditorState, StateField, StateEffect, RangeSetBuilder, RangeSet, Compartment } from "@codemirror/state";
import { defaultKeymap, history, historyKeymap, indentWithTab } from "@codemirror/commands";
import { StreamLanguage, syntaxHighlighting, HighlightStyle, bracketMatching,
  indentOnInput, foldGutter, foldKeymap } from "@codemirror/language";
import { searchKeymap, highlightSelectionMatches, search } from "@codemirror/search";
import { tags as t } from "@lezer/highlight";
import { luau } from "./luau.js";

let view = null;
let trocando = false;
const leitura = new Compartment();
const definirProblemas = StateEffect.define();

class Marcador extends GutterMarker {
  constructor(tipo) {
    super();
    this.tipo = tipo;
  }
  eq(outro) {
    return outro.tipo === this.tipo;
  }
  toDOM() {
    const ponto = document.createElement("span");
    ponto.className = `cm-marca ${this.tipo}`;
    return ponto;
  }
}

/** Uma marca por linha; erro vence aviso quando caem na mesma linha. */
const campoMarcas = StateField.define({
  create: () => RangeSet.empty,
  update(marcas, tr) {
    for (const efeito of tr.effects) {
      if (!efeito.is(definirProblemas)) continue;
      const porLinha = new Map();
      for (const p of efeito.value) {
        if (!p.linha) continue;
        const atual = porLinha.get(p.linha);
        if (!atual || p.tipo === "erro") porLinha.set(p.linha, p.tipo);
      }
      const construtor = new RangeSetBuilder();
      [...porLinha.entries()]
        .sort((a, b) => a[0] - b[0])
        .forEach(([linha, tipo]) => {
          const n = Math.min(Math.max(1, linha), tr.state.doc.lines);
          const pos = tr.state.doc.line(n).from;
          construtor.add(pos, pos, new Marcador(tipo));
        });
      return construtor.finish();
    }
    return marcas.map(tr.changes);
  },
});

const destaque = HighlightStyle.define([
  { tag: t.keyword, color: "var(--sin-keyword)" },
  { tag: [t.controlKeyword, t.moduleKeyword], color: "var(--sin-keyword)" },
  { tag: [t.string, t.special(t.string)], color: "var(--sin-string)" },
  { tag: t.comment, color: "var(--sin-comment)", fontStyle: "italic" },
  { tag: [t.number, t.bool, t.null, t.atom], color: "var(--sin-number)" },
  { tag: [t.operator, t.punctuation, t.separator], color: "var(--sin-operador)" },
  { tag: [t.definition(t.variableName), t.function(t.variableName)], color: "var(--sin-funcao)" },
  { tag: [t.standard(t.variableName), t.special(t.variableName)], color: "var(--sin-builtin)" },
  { tag: t.propertyName, color: "var(--sin-funcao)" },
  { tag: t.typeName, color: "var(--sin-builtin)" },
  { tag: t.invalid, color: "var(--erro)" },
]);

const temaEditor = EditorView.theme(
  {
    "&": { height: "100%", backgroundColor: "transparent", color: "var(--texto)" },
    ".cm-content": {
      fontFamily: "var(--fonte-mono)",
      fontSize: "var(--fonte-editor)",
      padding: "14px 0 32px",
      caretColor: "var(--acento)",
      lineHeight: "1.65",
    },
    ".cm-line": { padding: "0 18px 0 8px" },
    ".cm-gutters": {
      backgroundColor: "transparent",
      color: "color-mix(in srgb, var(--texto-suave) 70%, transparent)",
      border: "none",
      fontFamily: "var(--fonte-mono)",
      fontSize: "calc(var(--fonte-editor) - 1px)",
      paddingLeft: "10px",
    },
    ".cm-lineNumbers .cm-gutterElement": { padding: "0 8px 0 4px", minWidth: "28px" },
    ".cm-activeLine": { backgroundColor: "color-mix(in srgb, var(--texto) 4%, transparent)" },
    ".cm-activeLineGutter": { backgroundColor: "transparent", color: "var(--texto)" },
    ".cm-cursor, .cm-dropCursor": { borderLeftColor: "var(--acento)", borderLeftWidth: "2px" },
    ".cm-selectionBackground, &.cm-focused .cm-selectionBackground, ::selection": {
      backgroundColor: "color-mix(in srgb, var(--acento) 26%, transparent) !important",
    },
    ".cm-matchingBracket, .cm-nonmatchingBracket": {
      backgroundColor: "color-mix(in srgb, var(--acento) 20%, transparent)",
      outline: "1px solid color-mix(in srgb, var(--acento) 42%, transparent)",
    },
    ".cm-foldPlaceholder": {
      backgroundColor: "var(--superficie)",
      border: "1px solid var(--borda)",
      color: "var(--texto-suave)",
      borderRadius: "4px",
      padding: "0 4px",
    },
    ".cm-panels": { backgroundColor: "var(--painel-solido)", color: "var(--texto)" },
    ".cm-searchMatch": { backgroundColor: "color-mix(in srgb, var(--acento) 28%, transparent)" },
    ".cm-searchMatch.cm-searchMatch-selected": {
      backgroundColor: "color-mix(in srgb, var(--acento) 46%, transparent)",
    },
    ".cm-tooltip": {
      backgroundColor: "var(--superficie)",
      border: "1px solid var(--borda)",
      borderRadius: "8px",
      color: "var(--texto)",
    },
  },
  { dark: true },
);

/** Atalhos ficam no editor porque o CodeMirror engole o teclado quando tem foco. */
function atalhos(acoes) {
  const tecla = (key, run) => ({ key, preventDefault: true, run: () => (run(), true) });
  return keymap.of([
    tecla("Mod-s", acoes.salvar),
    tecla("Mod-Enter", acoes.executar),
    tecla("Mod-n", acoes.novo),
    indentWithTab,
    ...defaultKeymap,
    ...historyKeymap,
    ...foldKeymap,
    ...searchKeymap,
  ]);
}

function extensoes(acoes, somenteLeitura = false) {
  return [
    lineNumbers(),
    highlightActiveLineGutter(),
    highlightSpecialChars(),
    history(),
    foldGutter({
      markerDOM: (aberto) => {
        const el = document.createElement("span");
        el.textContent = aberto ? "⌄" : "›";
        el.style.color = "var(--texto-suave)";
        el.style.fontSize = "11px";
        return el;
      },
    }),
    drawSelection(),
    indentOnInput(),
    bracketMatching(),
    highlightActiveLine(),
    highlightSelectionMatches(),
    search({ top: false }),
    syntaxHighlighting(destaque),
    StreamLanguage.define(luau),
    campoMarcas,
    gutter({
      class: "cm-gutter-diagnostico",
      markers: (v) => v.state.field(campoMarcas),
      initialSpacer: () => new Marcador(""),
    }),
    atalhos(acoes),
    temaEditor,
    leitura.of(EditorState.readOnly.of(somenteLeitura)),
    EditorView.updateListener.of((atualizacao) => {
      if (!atualizacao.docChanged || trocando) return;
      acoes.aoEditar(atualizacao.state.doc.toString());
    }),
  ];
}

/** Cria a instância única; `acoes` chega do app para evitar import circular. */
export function criarEditor(elemento, acoes) {
  view = new EditorView({
    parent: elemento,
    state: EditorState.create({ doc: "", extensions: extensoes(acoes) }),
  });
  return view;
}

/** Troca o documento inteiro (outra aba). Undo de uma aba não vaza para a outra. */
export function mostrarDocumento(texto, { somenteLeitura, acoes }) {
  trocando = true;
  view.setState(
    EditorState.create({
      doc: texto,
      extensions: extensoes(acoes, Boolean(somenteLeitura)),
    }),
  );
  trocando = false;
}

export function definirLeitura(ligado) {
  view.dispatch({ effects: leitura.reconfigure(EditorState.readOnly.of(ligado)) });
}

export function aplicarMarcas(problemas) {
  if (!view) return;
  view.dispatch({ effects: definirProblemas.of(problemas) });
}

export function irParaLinha(linha) {
  if (!linha || !view) return;
  const doc = view.state.doc;
  const n = Math.min(Math.max(1, linha), doc.lines);
  const pos = doc.line(n).from;
  view.dispatch({
    selection: { anchor: pos },
    effects: EditorView.scrollIntoView(pos, { y: "center" }),
  });
  view.focus();
}

export const focarEditor = () => view && view.focus();
