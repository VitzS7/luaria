//! Stub de hook escrito à mão (x86-64) + instalação com as threads paradas.
//!
//! Por que à mão: o stub não pode chamar C (o cliente morre se a pilha mudar de
//! forma inesperada dentro do prólogo). Ele só grava registradores em slots,
//! opcionalmente troca a fonte do chunk e devolve o controle com rax capturado.

use std::io;

use crate::process::Alvo;

pub const OFF_FLAG: u64 = 0x700;
pub const OFF_CONTADOR: u64 = 0x710;
pub const OFF_RETSAVE: u64 = 0x740;
pub const OFF_RETVAL: u64 = 0x748;
pub const OFF_ARGS: u64 = 0x800;
pub const OFF_SRC: u64 = 0x900;
pub const PAGINAS_ESTADO: usize = 5;
pub const OFF_CATCHER: u64 = 0x180;

/// Faixa do laço de pausa dentro do stub, relativa ao início da página. Só a
/// thread que realmente parou executa estas instruções: as demais saem na primeira
/// checagem e vão direto para o fim do laço (`SPIN_FIM`).
pub const SPIN_INICIO: u64 = 0x85;
pub const SPIN_FIM: u64 = 0x91;

#[derive(Debug, Clone)]
pub struct Hook {
    pub fn_addr: u64,
    pub stub: u64,
    pub prologo: usize,
    /// Quantas páginas contíguas o stub reservou (5 = cabe prelude + script).
    pub paginas: usize,
}


fn store6(out: &mut Vec<u8>, slot: u64) {
    // args SysV (rdi..r9) gravados em slots consecutivos para o host ler
    const REGS: [[u8; 3]; 6] = [
        [0x48, 0x89, 0x38],
        [0x48, 0x89, 0x30],
        [0x48, 0x89, 0x10],
        [0x48, 0x89, 0x08],
        [0x4C, 0x89, 0x00],
        [0x4C, 0x89, 0x08],
    ];
    for (i, enc) in REGS.iter().enumerate() {
        let alvo = slot + i as u64 * 8;
        let mut bloco = vec![0x50u8, 0x48, 0xB8];
        bloco.extend_from_slice(&alvo.to_le_bytes());
        bloco.extend_from_slice(enc);
        bloco.push(0x58);
        out.extend_from_slice(&bloco);
    }
}

/// Monta o stub. `fonte` = (ponteiro, tamanho) da fonte a injetar (opcional).
fn montar_stub(
    slot: u64,
    flag: u64,
    retsave: u64,
    catcher: u64,
    fonte: Option<(u64, usize)>,
    limpar_flag: bool,
    capturar_rax: bool,
    prologo: &[u8],
    volta: u64,
) -> Vec<u8> {
    let mut s = Vec::with_capacity(256);
    // contador de entradas: o host usa isso para não perder chamadas repetidas
    s.push(0x50);
    s.extend_from_slice(&[0x48, 0xB8]);
    s.extend_from_slice(&(slot - (OFF_ARGS - OFF_CONTADOR)).to_le_bytes());
    s.extend_from_slice(&[0x48, 0xFF, 0x00]);
    s.push(0x58);
    store6(&mut s, slot);

    // modo pausa: o host pede (flag=3), o stub confirma (flag=4) e espera
    // até o host liberar (flag=0). Timeout evita travar o cliente para sempre.
    s.push(0x50);
    s.extend_from_slice(&[0x48, 0xB8]);
    s.extend_from_slice(&(slot - (OFF_ARGS - OFF_FLAG)).to_le_bytes());
    s.extend_from_slice(&[0x83, 0x38, 0x03]);
    let jne = s.len();
    s.extend_from_slice(&[0x75, 0x00]);
    s.extend_from_slice(&[0xC7, 0x00, 0x04, 0x00, 0x00, 0x00]);
    s.extend_from_slice(&[0x41, 0xBA]);
    s.extend_from_slice(&0x4000_0000u32.to_le_bytes());
    let spin = s.len();
    s.extend_from_slice(&[0x83, 0x38, 0x04]);
    let jne2 = s.len();
    s.extend_from_slice(&[0x75, 0x00]);
    s.extend_from_slice(&[0xF3, 0x90]);
    s.extend_from_slice(&[0x41, 0xFF, 0xCA]);
    let jnz = s.len();
    s.extend_from_slice(&[0x75, 0x00]);
    let fim = s.len();
    debug_assert_eq!(spin as u64, SPIN_INICIO, "pause loop moved offset");
    debug_assert_eq!(fim as u64, SPIN_FIM, "end of pause loop moved offset");
    s[jne + 1] = (fim - (jne + 2)) as u8;
    s[jne2 + 1] = (fim - (jne2 + 2)) as u8;
    s[jnz + 1] = (spin as i64 - (jnz as i64 + 2)) as u8;
    s.push(0x58);

    // salva o endereço de retorno original e coloca o catcher no lugar.
    // Só quando alguém vai ler `rax` depois: o slot é global, então com duas
    // threads em luaD_call ao mesmo tempo uma volta para o endereço da outra.
    if capturar_rax {
        s.extend_from_slice(&[0x4C, 0x8B, 0x1C, 0x24]);
        s.extend_from_slice(&[0x49, 0xBA]);
        s.extend_from_slice(&retsave.to_le_bytes());
        s.extend_from_slice(&[0x4D, 0x89, 0x1A]);
        s.extend_from_slice(&[0x49, 0xBB]);
        s.extend_from_slice(&catcher.to_le_bytes());
        s.extend_from_slice(&[0x4C, 0x89, 0x1C, 0x24]);
    }

    if let Some((src, len)) = fonte {
        s.push(0x50);
        s.extend_from_slice(&[0x48, 0xB8]);
        s.extend_from_slice(&flag.to_le_bytes());
        s.extend_from_slice(&[0x83, 0x38, 0x00]);
        let je = s.len();
        s.extend_from_slice(&[0x74, 0x00]);
        s.extend_from_slice(&[0x48, 0xBE]); // args do luau_load: nome, dados, tamanho
        s.extend_from_slice(&src.to_le_bytes());
        s.extend_from_slice(&[0x48, 0xBA]);
        s.extend_from_slice(&src.to_le_bytes());
        s.extend_from_slice(&[0x48, 0xB9]);
        s.extend_from_slice(&(len as u64).to_le_bytes());
        if limpar_flag {
            s.extend_from_slice(&[0xC7, 0x00, 0, 0, 0, 0]);
        }
        s[je + 1] = (s.len() - (je + 2)) as u8;
        s.push(0x58);
    }

    s.extend_from_slice(prologo);
    s.extend_from_slice(&[0x48, 0xB8]);
    s.extend_from_slice(&volta.to_le_bytes());
    s.extend_from_slice(&[0xFF, 0xE0]);
    s
}

fn montar_catcher(retval: u64, retsave: u64) -> Vec<u8> {
    let mut c = Vec::with_capacity(32);
    c.extend_from_slice(&[0x49, 0xBA]);
    c.extend_from_slice(&retval.to_le_bytes());
    c.extend_from_slice(&[0x49, 0x89, 0x02]);
    c.extend_from_slice(&[0x49, 0xBA]);
    c.extend_from_slice(&retsave.to_le_bytes());
    c.extend_from_slice(&[0x4D, 0x8B, 0x12]);
    c.extend_from_slice(&[0x41, 0xFF, 0xE2]);
    c
}

impl Hook {
    /// Adota um hook já instalado (prólogo já trocado pelo salto para o stub).
    pub fn adotar(alvo: &Alvo, offset: u64) -> io::Result<Hook> {
        let fn_addr = alvo.base + offset;
        let pro = alvo.bytes(fn_addr, 14)?;
        if !(pro[0] == 0xFF && pro[1] == 0x25) {
            return Err(io::Error::new(io::ErrorKind::NotFound, "no hook at the address"));
        }
        let mut d = [0u8; 8];
        d.copy_from_slice(&pro[6..14]);
        let stub = u64::from_le_bytes(d);
        // stub fora do payload ou que não começa com o nosso prólogo não é hook nosso
        let dentro = stub >= alvo.exec.0 && stub < alvo.exec.1;
        if !dentro || alvo.bytes(stub, 1)? != [0x50] {
            return Err(io::Error::new(io::ErrorKind::InvalidData, "hook found is not Luaria's"));
        }
        Ok(Hook { fn_addr, stub, prologo: 16, paginas: PAGINAS_ESTADO })
    }

    /// Instala o hook em `base + offset` do payload. `fonte` opcional já arma a substituição.
    pub fn instalar(
        alvo: &Alvo,
        elf: &crate::elf::Elf,
        offset: u64,
        fonte: Option<&[u8]>,
        sempre: bool,
    ) -> io::Result<Hook> {
        let fn_addr = alvo.base + offset;
        let pro = alvo.bytes(fn_addr, 32)?;
        if pro[0] == 0xFF && pro[1] == 0x25 {
            return Err(io::Error::new(io::ErrorKind::AlreadyExists, "hook already installed"));
        }
        if pro[0] != 0x55 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("unexpected prologue: {:02x} {:02x} {:02x}", pro[0], pro[1], pro[2]),
            ));
        }
        // O prólogo só pode conter instruções "seguras de copiar": um `call rel32`
        // copiado para o stub aponta para o lugar errado e mata o processo.
        let n = crate::scan::prologo_seguro(elf, offset).ok_or_else(|| {
            io::Error::new(io::ErrorKind::InvalidData, "prologue not copyable (relative branch)")
        })?;
        // prologue_len mede no arquivo; conferir que os bytes batem com o runtime
        let no_arquivo = elf.bytes_at(offset, n).unwrap_or(&[]);
        let n = if no_arquivo == &pro[..n] {
            n
        } else {
            // payload em memória divergente do arquivo: mede direto nos bytes vivos
            medir_prologo(&pro)?
        };

        // Nem todo payload tem um bloco de 5 páginas zeradas (o medido tem um só).
        // Com 1 página o motor ainda funciona, só sem prelude grande.
        let (pg, paginas) = alvo
            .pagina_livre_contigua(PAGINAS_ESTADO)
            .map(|p| (p, PAGINAS_ESTADO))
            .or_else(|| alvo.pagina_livre_contigua(1).map(|p| (p, 1)))
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "no free page in the payload"))?;
        let slot = pg + OFF_ARGS;
        let flag = pg + OFF_FLAG;
        let retsave = pg + OFF_RETSAVE;
        let retval = pg + OFF_RETVAL;
        let catcher = pg + OFF_CATCHER;
        let src_ptr = pg + OFF_SRC;

        let fonte_meta = fonte.map(|f| (src_ptr, f.len()));
        // capturar rax só quando o chamador vai ler `retval` (modo watch)
        let capturar = fonte.is_some();
        let stub = montar_stub(
            slot,
            flag,
            retsave,
            catcher,
            fonte_meta,
            !sempre,
            capturar,
            &pro[..n],
            fn_addr + n as u64,
        );
        let cat = montar_catcher(retval, retsave);

        let mut patch = vec![0x90u8; n];
        if n >= 14 {
            // salto indireto: o destino de 8 bytes fica logo depois do opcode
            patch[0] = 0xFF;
            patch[1] = 0x25;
            patch[2] = 0;
            patch[3] = 0;
            patch[4] = 0;
            patch[5] = 0;
            patch[6..14].copy_from_slice(&pg.to_le_bytes());
        } else {
            // a página do stub está no mesmo módulo, então rel32 sempre alcança
            let rel = pg as i64 - (fn_addr as i64 + 5);
            if !(-0x8000_0000..0x8000_0000).contains(&rel) {
                return Err(io::Error::new(io::ErrorKind::InvalidData, "stub longe demais"));
            }
            patch[0] = 0xE9;
            patch[1..5].copy_from_slice(&(rel as i32).to_le_bytes());
        }

        let paradas = alvo.parar_com_guarda();
        // Se alguma thread está DENTRO dos bytes que vamos sobrescrever, retomar
        // depois do patch cai no meio do `jmp` e mata o processo. Isso acontece
        // em jogo, onde luaD_call é quente; no menu quase nunca.
        for tid in &paradas.tids {
            if let Some(rip) = alvo.rip(*tid) {
                if rip >= fn_addr && rip < fn_addr + n as u64 {
                    return Err(io::Error::new(
                        io::ErrorKind::WouldBlock,
                        format!("thread {tid} inside the function (rip +{:#x})", rip - fn_addr),
                    ));
                }
            }
        }
        let r = (|| {
            alvo.escrever(catcher, &cat)?;
            if let Some(f) = fonte {
                alvo.escrever(src_ptr, f)?;
                let um = 1u32;
                alvo.escrever(flag, &um.to_le_bytes())?;
            }
            alvo.escrever(pg, &stub)?;
            alvo.escrever(fn_addr, &patch)
        })();
        drop(paradas);
        r?;
        Ok(Hook { fn_addr, stub: pg, prologo: n, paginas })
    }

    /// Lê os slots de argumentos (rdi, rsi, rdx, rcx, r8, r9).
    pub fn args(&self, alvo: &Alvo) -> io::Result<[u64; 6]> {
        let mut a = [0u64; 6];
        for (i, v) in a.iter_mut().enumerate() {
            *v = alvo.u64_em(self.stub + OFF_ARGS + i as u64 * 8)?;
        }
        Ok(a)
    }

    /// Pede que a próxima entrada no hook pare a thread até `liberar`.
    pub fn pedir_pausa(&self, alvo: &Alvo) -> io::Result<()> {
        alvo.escrever(self.stub + OFF_FLAG, &3u32.to_le_bytes())
    }

    pub fn em_pausa(&self, alvo: &Alvo) -> bool {
        alvo.u32_em(self.stub + OFF_FLAG).map(|v| v == 4).unwrap_or(false)
    }

    pub fn liberar(&self, alvo: &Alvo) -> io::Result<()> {
        alvo.escrever(self.stub + OFF_FLAG, &0u32.to_le_bytes())
    }

    pub fn contador(&self, alvo: &Alvo) -> io::Result<u64> {
        alvo.u64_em(self.stub + OFF_CONTADOR)
    }

    /// Endereço de retorno salvo pelo stub (quem chamou a função hookada).
    pub fn retorno(&self, alvo: &Alvo) -> io::Result<u64> {
        alvo.u64_em(self.stub + OFF_RETSAVE)
    }

    pub fn retval(&self, alvo: &Alvo) -> io::Result<u64> {
        alvo.u64_em(self.stub + OFF_RETVAL)
    }

    pub fn limpar_args(&self, alvo: &Alvo) -> io::Result<()> {
        alvo.escrever(self.stub + OFF_ARGS, &[0u8; 48])
    }

    /// Arma uma fonte para a próxima chamada ao alvo.
    pub fn armar(&self, alvo: &Alvo, fonte: &[u8]) -> io::Result<()> {
        alvo.escrever(self.stub + OFF_SRC, fonte)?;
        let um = 1u32;
        alvo.escrever(self.stub + OFF_FLAG, &um.to_le_bytes())
    }

    pub fn desarmar(&self, alvo: &Alvo) -> io::Result<()> {
        alvo.escrever(self.stub + OFF_FLAG, &0u32.to_le_bytes())
    }
}

fn medir_prologo(bytes: &[u8]) -> io::Result<usize> {
    use iced_x86::{Decoder, DecoderOptions};
    let mut dec = Decoder::with_ip(64, bytes, 0, DecoderOptions::NONE);
    let mut total = 0usize;
    while total < 14 {
        if !dec.can_decode() {
            return Err(io::Error::new(io::ErrorKind::InvalidData, "prologue too short"));
        }
        let i = dec.decode();
        total += i.len();
        if total > 32 {
            return Err(io::Error::new(io::ErrorKind::InvalidData, "prologue too long"));
        }
    }
    Ok(total)
}
