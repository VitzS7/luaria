// Tokenizador Luau para o StreamLanguage do CodeMirror. Cobre o que um
// executor precisa colorir: comentários longos, strings com escape, números
// hexadecimais e os globais do Roblox mais usados.

const palavras = new Set([
  "and", "break", "continue", "do", "else", "elseif", "end", "export", "false",
  "for", "function", "if", "in", "local", "nil", "not", "or", "repeat", "return",
  "then", "true", "type", "until", "while",
]);

const globais = new Set([
  "print", "warn", "error", "assert", "pcall", "xpcall", "select", "tonumber",
  "tostring", "type", "typeof", "next", "pairs", "ipairs", "unpack", "require",
  "setmetatable", "getmetatable", "rawget", "rawset", "rawequal", "collectgarbage",
  "loadstring", "load", "task", "wait", "delay", "spawn", "tick", "game", "workspace",
  "script", "shared", "Instance", "Vector2", "Vector3", "CFrame", "Color3", "UDim",
  "UDim2", "Enum", "Ray", "Region3", "TweenInfo", "NumberRange", "NumberSequence",
  "BrickColor", "math", "string", "table", "coroutine", "os", "bit32", "utf8",
  "buffer", "debug", "getfenv", "setfenv", "newproxy", "gcinfo", "ypcall",
]);

/** Consome até o fecho `]=]` do mesmo nível que abriu; sem fecho, vai até o fim da linha. */
function lerLongo(stream, abre) {
  const nivel = abre.slice(1, -1);
  const fecho = `]${nivel}]`;
  while (!stream.eol()) {
    if (stream.match(fecho)) return true;
    stream.next();
  }
  return false;
}

function lerComentario(stream, st) {
  st.ultimo = "comentario";
  if (stream.match(/^\[=*\[/)) {
    lerLongo(stream, stream.current().slice(2));
    return "comment";
  }
  stream.skipToEnd();
  return "comment";
}

function lerTexto(stream, st, quote) {
  let escapado = false;
  while (!stream.eol()) {
    const ch = stream.next();
    if (ch === quote && !escapado) break;
    escapado = !escapado && ch === "\\";
  }
  st.ultimo = "texto";
  return "string";
}

function lerNumero(stream, st) {
  if (!stream.match(/^0[xX][0-9a-fA-F_]+/)) {
    stream.match(/^\d[\d_]*(\.\d+)?([eE][+-]?\d+)?/);
  }
  st.ultimo = "numero";
  return "number";
}

function lerPalavra(stream, st) {
  const antesDePonto = st.ultimo === "ponto";
  stream.match(/^[\w_]+/);
  const palavra = stream.current();
  st.ultimo = "palavra";
  if (antesDePonto) return "propertyName";
  if (palavras.has(palavra)) {
    st.esperaNome = palavra === "function";
    return "keyword";
  }
  if (st.esperaNome) {
    st.esperaNome = false;
    return "def";
  }
  return globais.has(palavra) ? "builtin" : "variableName";
}

export const luau = {
  name: "luau",
  startState: () => ({ ultimo: null, esperaNome: false }),
  token(stream, st) {
    if (stream.eatSpace()) return null;
    const c = stream.peek();

    if (c === "-" && stream.match("--")) return lerComentario(stream, st);
    if (c === '"' || c === "'") return lerTexto(stream, st, stream.next());

    if (c === "[") {
      const m = stream.match(/^\[=*\[/);
      if (m) {
        lerLongo(stream, m[0]);
        st.ultimo = "texto";
        return "string";
      }
      stream.next();
      st.ultimo = "operador";
      return "operator";
    }

    if (/\d/.test(c)) return lerNumero(stream, st);
    if (/[\w_]/.test(c)) return lerPalavra(stream, st);

    if (c === "." || c === ":") {
      const doispontos = stream.match(/^::/);
      stream.next();
      st.ultimo = doispontos ? "operador" : "ponto";
      return "operator";
    }

    if (stream.match(/^[+\-*/%^#=<>~&|!]+/)) {
      st.ultimo = "operador";
      return "operator";
    }

    stream.next();
    st.ultimo = "outro";
    return null;
  },
};
