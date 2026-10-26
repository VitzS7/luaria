//! Codec dos campos que trafegam no anel host<->jogo para pedidos e respostas HTTP.
//!
//! Formato: cada campo é `<len decimal>:<bytes>`, concatenados. Não há escape porque
//! o len diz onde o campo acaba, então o corpo aceita qualquer byte (inclusive `:`).

use std::fmt;

const METODOS: [&str; 7] = ["GET", "POST", "PUT", "DELETE", "PATCH", "HEAD", "OPTIONS"];
const MAX_HEADERS: usize = 64;
const MAX_HEADER_BYTES: usize = 8 * 1024;
/// Cabe em `usize` de 32 bits, então um len maior que isso já é lixo.
const MAX_DIGITOS: usize = 10;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ErroCodec {
    Truncado,
    NumeroInvalido,
    CamposDemais,
    MuitosHeaders,
    MetodoInvalido,
    EsquemaInvalido,
    UrlInvalida,
    HeaderInvalido,
    Utf8,
}

impl fmt::Display for ErroCodec {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        let txt = match self {
            ErroCodec::Truncado => "truncated or missing field",
            ErroCodec::NumeroInvalido => "invalid field size",
            ErroCodec::CamposDemais => "campos demais",
            ErroCodec::MuitosHeaders => "headers demais",
            ErroCodec::MetodoInvalido => "HTTP method not allowed",
            ErroCodec::EsquemaInvalido => "only http and https",
            ErroCodec::UrlInvalida => "URL with space or control character",
            ErroCodec::HeaderInvalido => "invalid header (name, value or size)",
            ErroCodec::Utf8 => "text is not UTF-8",
        };
        f.write_str(txt)
    }
}

pub fn codificar(campos: &[&[u8]]) -> Vec<u8> {
    let mut out = Vec::new();
    for c in campos {
        out.extend_from_slice(c.len().to_string().as_bytes());
        out.push(b':');
        out.extend_from_slice(c);
    }
    out
}

/// Só dígitos, sem sinal, sem zeros à esquerda. `+3` ou `03` seriam aceitos pelo
/// `parse` e o jogo poderia gravar um len que o host lê diferente.
fn numero(digitos: &[u8]) -> Result<usize, ErroCodec> {
    let valido = !digitos.is_empty()
        && digitos.len() <= MAX_DIGITOS
        && digitos.iter().all(u8::is_ascii_digit)
        && (digitos.len() == 1 || digitos[0] != b'0');
    if !valido {
        return Err(ErroCodec::NumeroInvalido);
    }
    std::str::from_utf8(digitos).ok().and_then(|s| s.parse().ok()).ok_or(ErroCodec::NumeroInvalido)
}

fn ler_len(buf: &[u8], i: &mut usize) -> Result<usize, ErroCodec> {
    let ini = *i;
    while *i < buf.len() && buf[*i].is_ascii_digit() {
        *i += 1;
    }
    match buf.get(*i) {
        Some(b':') => {}
        Some(_) => return Err(ErroCodec::NumeroInvalido),
        None => return Err(ErroCodec::Truncado),
    }
    let len = numero(&buf[ini..*i])?;
    *i += 1;
    Ok(len)
}

/// Campo que passa do fim do buffer é rejeitado antes de copiar, então um len
/// gigante não aloca nada.
pub fn decodificar(buf: &[u8], max_campos: usize) -> Result<Vec<Vec<u8>>, ErroCodec> {
    let mut campos = Vec::new();
    let mut i = 0;
    while i < buf.len() {
        if campos.len() == max_campos {
            return Err(ErroCodec::CamposDemais);
        }
        let len = ler_len(buf, &mut i)?;
        let fim = i.checked_add(len).filter(|f| *f <= buf.len()).ok_or(ErroCodec::Truncado)?;
        campos.push(buf[i..fim].to_vec());
        i = fim;
    }
    Ok(campos)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PedidoHttp {
    pub metodo: String,
    pub url: String,
    pub headers: Vec<(String, String)>,
    pub corpo: Vec<u8>,
}

fn texto(b: &[u8]) -> Result<String, ErroCodec> {
    std::str::from_utf8(b).map(str::to_owned).map_err(|_| ErroCodec::Utf8)
}

fn validar_header(nome: &str, valor: &str) -> Result<(), ErroCodec> {
    // token da RFC 7230: qualquer outro caractere no nome vira linha extra no curl
    let token = !nome.is_empty()
        && nome.bytes().all(|b| b.is_ascii_alphanumeric() || b"!#$%&'*+-.^_`|~".contains(&b));
    let valor_ok = valor.bytes().all(|b| b == b'\t' || (b >= 0x20 && b != 0x7f));
    if !token || !valor_ok || nome.len() + valor.len() > MAX_HEADER_BYTES {
        return Err(ErroCodec::HeaderInvalido);
    }
    Ok(())
}

impl PedidoHttp {
    /// Também rodada por `pedir_http`, porque a struct tem campos públicos e pode
    /// vir montada direto no Rust.
    pub fn validar(&self) -> Result<(), ErroCodec> {
        if !METODOS.contains(&self.metodo.as_str()) {
            return Err(ErroCodec::MetodoInvalido);
        }
        let minusc = self.url.to_ascii_lowercase();
        if !(minusc.starts_with("http://") || minusc.starts_with("https://")) {
            return Err(ErroCodec::EsquemaInvalido);
        }
        if self.url.bytes().any(|b| b <= b' ' || b == 0x7f) {
            return Err(ErroCodec::UrlInvalida);
        }
        if self.headers.len() > MAX_HEADERS {
            return Err(ErroCodec::MuitosHeaders);
        }
        self.headers.iter().try_for_each(|(nome, valor)| validar_header(nome, valor))
    }

    /// Formato: método, url, nHeaders, (nome, valor)*, corpo.
    pub fn decodificar(buf: &[u8]) -> Result<PedidoHttp, ErroCodec> {
        let mut campos = decodificar(buf, 4 + 2 * MAX_HEADERS)?;
        if campos.len() < 3 {
            return Err(ErroCodec::Truncado);
        }
        // o limite de campos em `decodificar` já barra n > MAX_HEADERS
        let n = numero(&campos[2])?;
        let esperado = 4 + 2 * n;
        if campos.len() < esperado {
            return Err(ErroCodec::Truncado);
        }
        if campos.len() > esperado {
            return Err(ErroCodec::CamposDemais);
        }
        let metodo = texto(&campos[0])?;
        let url = texto(&campos[1])?;
        let mut headers = Vec::with_capacity(n);
        for par in campos[3..3 + 2 * n].chunks(2) {
            headers.push((texto(&par[0])?, texto(&par[1])?));
        }
        let corpo = std::mem::take(&mut campos[3 + 2 * n]);
        let pedido = PedidoHttp { metodo, url, headers, corpo };
        pedido.validar()?;
        Ok(pedido)
    }
}

/// `status` não entra nos campos: vai no campo status do slot (u32), não no corpo.
/// Status 0 é erro local (resposta truncada), nunca um HTTP real.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RespostaHttp {
    pub status: u32,
    pub mensagem: String,
    pub headers: Vec<(String, String)>,
    pub corpo: Vec<u8>,
}

impl RespostaHttp {
    /// Formato: mensagem, nHeaders, (nome, valor)*, corpo.
    pub fn codificar(&self) -> Vec<u8> {
        let n = self.headers.len().to_string();
        let mut campos: Vec<&[u8]> = vec![self.mensagem.as_bytes(), n.as_bytes()];
        for (nome, valor) in &self.headers {
            campos.push(nome.as_bytes());
            campos.push(valor.as_bytes());
        }
        campos.push(&self.corpo);
        codificar(&campos)
    }
}

#[cfg(test)]
mod testes {
    use super::*;

    fn bytes_pedido(metodo: &str, url: &str, headers: &[(&str, &str)], corpo: &[u8]) -> Vec<u8> {
        let n = headers.len().to_string();
        let mut campos: Vec<&[u8]> = vec![metodo.as_bytes(), url.as_bytes(), n.as_bytes()];
        for (nome, valor) in headers {
            campos.push(nome.as_bytes());
            campos.push(valor.as_bytes());
        }
        campos.push(corpo);
        codificar(&campos)
    }

    #[test]
    fn ida_e_volta_de_campos() {
        let campos: [&[u8]; 3] = [b"ola", b"", b"a:b\n:c"];
        let buf = codificar(&campos);
        assert_eq!(buf, b"3:ola0:6:a:b\n:c");
        let lido = decodificar(&buf, 10).unwrap();
        assert_eq!(lido, vec![b"ola".to_vec(), b"".to_vec(), b"a:b\n:c".to_vec()]);
    }

    #[test]
    fn buffer_vazio_sao_zero_campos() {
        assert_eq!(decodificar(b"", 4), Ok(vec![]));
    }

    #[test]
    fn truncado() {
        assert_eq!(decodificar(b"5:abc", 4), Err(ErroCodec::Truncado));
        assert_eq!(decodificar(b"12", 4), Err(ErroCodec::Truncado));
        assert_eq!(decodificar(b"1:", 4), Err(ErroCodec::Truncado));
    }

    #[test]
    fn total_maior_que_buffer() {
        assert_eq!(decodificar(b"3:abc3:", 4), Err(ErroCodec::Truncado));
    }

    #[test]
    fn len_gigante_nao_aloca() {
        assert_eq!(decodificar(b"99999999:x", 4), Err(ErroCodec::Truncado));
        assert_eq!(decodificar(b"99999999999:", 4), Err(ErroCodec::NumeroInvalido));
    }

    #[test]
    fn len_com_zeros_ou_sinal_recusado() {
        assert_eq!(decodificar(b"03:abc", 4), Err(ErroCodec::NumeroInvalido));
        assert_eq!(decodificar(b"+3:abc", 4), Err(ErroCodec::NumeroInvalido));
        assert_eq!(decodificar(b"-1:", 4), Err(ErroCodec::NumeroInvalido));
        assert_eq!(decodificar(b" 3:abc", 4), Err(ErroCodec::NumeroInvalido));
        assert_eq!(decodificar(b":", 4), Err(ErroCodec::NumeroInvalido));
        assert_eq!(decodificar(b"3x:abc", 4), Err(ErroCodec::NumeroInvalido));
    }

    #[test]
    fn zero_e_campo_vazio_valido() {
        assert_eq!(decodificar(b"0:0:", 4), Ok(vec![vec![], vec![]]));
    }

    #[test]
    fn campos_demais_pelo_limite() {
        assert_eq!(decodificar(b"1:a1:b", 1), Err(ErroCodec::CamposDemais));
    }

    #[test]
    fn pedido_ida_e_volta() {
        let buf = bytes_pedido("POST", "https://a.teste/x?y=1", &[("X-A", "1"), ("Accept", "*/*")], b"c:orpo\n");
        let p = PedidoHttp::decodificar(&buf).unwrap();
        assert_eq!(p.metodo, "POST");
        assert_eq!(p.url, "https://a.teste/x?y=1");
        assert_eq!(p.headers, vec![("X-A".into(), "1".into()), ("Accept".into(), "*/*".into())]);
        assert_eq!(p.corpo, b"c:orpo\n");
    }

    #[test]
    fn pedido_sem_headers_e_corpo_vazio() {
        let p = PedidoHttp::decodificar(&bytes_pedido("GET", "http://a.teste/", &[], b"")).unwrap();
        assert!(p.headers.is_empty() && p.corpo.is_empty());
    }

    #[test]
    fn header_com_quebra_de_linha_recusado() {
        let buf = bytes_pedido("GET", "https://a.teste/", &[("X-A", "1\r\nHost: evil")], b"");
        assert_eq!(PedidoHttp::decodificar(&buf), Err(ErroCodec::HeaderInvalido));
        let buf = bytes_pedido("GET", "https://a.teste/", &[("X-A\n", "1")], b"");
        assert_eq!(PedidoHttp::decodificar(&buf), Err(ErroCodec::HeaderInvalido));
    }

    #[test]
    fn nome_de_header_fora_do_token() {
        let buf = bytes_pedido("GET", "https://a.teste/", &[("X A", "1")], b"");
        assert_eq!(PedidoHttp::decodificar(&buf), Err(ErroCodec::HeaderInvalido));
        let buf = bytes_pedido("GET", "https://a.teste/", &[("", "1")], b"");
        assert_eq!(PedidoHttp::decodificar(&buf), Err(ErroCodec::HeaderInvalido));
    }

    #[test]
    fn header_grande_demais() {
        let grande = "x".repeat(MAX_HEADER_BYTES);
        let buf = bytes_pedido("GET", "https://a.teste/", &[("X-A", &grande)], b"");
        assert_eq!(PedidoHttp::decodificar(&buf), Err(ErroCodec::HeaderInvalido));
        let limite = "x".repeat(MAX_HEADER_BYTES - 3);
        let buf = bytes_pedido("GET", "https://a.teste/", &[("X-A", &limite)], b"");
        assert!(PedidoHttp::decodificar(&buf).is_ok());
    }

    #[test]
    fn muitos_headers() {
        let nomes: Vec<String> = (0..=MAX_HEADERS).map(|i| format!("X-{i}")).collect();
        let pares: Vec<(&str, &str)> = nomes.iter().map(|n| (n.as_str(), "1")).collect();
        let buf = bytes_pedido("GET", "https://a.teste/", &pares, b"");
        assert_eq!(PedidoHttp::decodificar(&buf), Err(ErroCodec::CamposDemais));
        let pedido = PedidoHttp { metodo: "GET".into(), url: "https://a.teste/".into(), headers: vec![("A".into(), "1".into()); MAX_HEADERS + 1], corpo: vec![] };
        assert_eq!(pedido.validar(), Err(ErroCodec::MuitosHeaders));
    }

    #[test]
    fn metodo_fora_da_lista() {
        let buf = bytes_pedido("TRACE", "https://a.teste/", &[], b"");
        assert_eq!(PedidoHttp::decodificar(&buf), Err(ErroCodec::MetodoInvalido));
        let buf = bytes_pedido("get", "https://a.teste/", &[], b"");
        assert_eq!(PedidoHttp::decodificar(&buf), Err(ErroCodec::MetodoInvalido));
    }

    #[test]
    fn esquema_file_recusado() {
        let buf = bytes_pedido("GET", "file:///etc/passwd", &[], b"");
        assert_eq!(PedidoHttp::decodificar(&buf), Err(ErroCodec::EsquemaInvalido));
        let buf = bytes_pedido("GET", "gopher://a/", &[], b"");
        assert_eq!(PedidoHttp::decodificar(&buf), Err(ErroCodec::EsquemaInvalido));
    }

    #[test]
    fn url_com_espaco_recusada() {
        let buf = bytes_pedido("GET", "https://a.teste/ x", &[], b"");
        assert_eq!(PedidoHttp::decodificar(&buf), Err(ErroCodec::UrlInvalida));
    }

    #[test]
    fn campo_extra_depois_do_corpo_recusado() {
        let mut buf = bytes_pedido("GET", "https://a.teste/", &[], b"");
        buf.extend_from_slice(b"1:z");
        assert_eq!(PedidoHttp::decodificar(&buf), Err(ErroCodec::CamposDemais));
    }

    #[test]
    fn contagem_de_headers_maior_que_os_campos() {
        let buf = codificar(&[b"GET", b"https://a.teste/", b"2", b"A", b"1", b""]);
        assert_eq!(PedidoHttp::decodificar(&buf), Err(ErroCodec::Truncado));
    }

    #[test]
    fn utf8_invalido_recusado() {
        let buf = codificar(&[b"GET", b"https://a.teste/\xff", b"0", b""]);
        assert_eq!(PedidoHttp::decodificar(&buf), Err(ErroCodec::Utf8));
    }

    #[test]
    fn resposta_codificada_tem_mensagem_headers_e_corpo() {
        let resp = RespostaHttp {
            status: 201,
            mensagem: "Created".into(),
            headers: vec![("Content-Type".into(), "text/plain".into())],
            corpo: b"ola".to_vec(),
        };
        let lido = decodificar(&resp.codificar(), 8).unwrap();
        assert_eq!(
            lido,
            vec![b"Created".to_vec(), b"1".to_vec(), b"Content-Type".to_vec(), b"text/plain".to_vec(), b"ola".to_vec()]
        );
    }
}
