//! Acesso a um processo vivo: memória via `process_vm_*`, parada de threads via
//! `ptrace`, chamada remota e mapeamento do payload.

use std::fs;
use std::io;

pub const MODULO: &str = "libroblox.so";

#[derive(Debug, Clone)]
pub struct Faixa {
    pub ini: u64,
    pub fim: u64,
    pub perm: String,
    pub offset: u64,
    pub caminho: String,
}

#[derive(Debug, Clone)]
pub struct Alvo {
    pub pid: i32,
    pub base: u64,
    pub exec: (u64, u64),
    pub caminho: String,
    pub faixas: Vec<Faixa>,
}

fn ler_maps(pid: i32) -> io::Result<Vec<Faixa>> {
    let txt = fs::read_to_string(format!("/proc/{pid}/maps"))?;
    let mut v = Vec::new();
    for l in txt.lines() {
        let mut it = l.splitn(6, ' ');
        let (faixa, perm, offset) = match (it.next(), it.next(), it.next()) {
            (Some(a), Some(b), Some(c)) => (a, b, c),
            _ => continue,
        };
        let (ini, fim) = match faixa.split_once('-') {
            Some((a, b)) => (u64::from_str_radix(a, 16).unwrap_or(0), u64::from_str_radix(b, 16).unwrap_or(0)),
            None => continue,
        };
        // o caminho pode ter espaços e o formato alinha colunas com espaços extras;
        // então reconstruímos do 6º token em diante
        let caminho: String = l
            .trim_end()
            .split_whitespace()
            .skip(5)
            .collect::<Vec<_>>()
            .join(" ");
        v.push(Faixa {
            ini,
            fim,
            perm: perm.to_string(),
            offset: u64::from_str_radix(offset, 16).unwrap_or(0),
            caminho,
        });
    }
    Ok(v)
}

/// Processo com `libroblox.so` mapeado e o tamanho do maior segmento executável
/// dele (o cliente de verdade tem o maior; helper de preload tem menos).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Candidato {
    pub pid: i32,
    pub tam_exec: u64,
}

/// Todos os processos com o payload mapeado, em ordem crescente de pid.
pub fn candidatos_payload() -> Vec<Candidato> {
    let Ok(dir) = fs::read_dir("/proc") else { return Vec::new() };
    let mut v: Vec<Candidato> = dir
        .flatten()
        .filter_map(|e| e.file_name().to_string_lossy().parse::<i32>().ok())
        .filter_map(|pid| {
            let txt = fs::read_to_string(format!("/proc/{pid}/maps")).ok()?;
            let tam_exec = maior_exec_do_modulo(&txt);
            (tam_exec > 0).then_some(Candidato { pid, tam_exec })
        })
        .collect();
    v.sort_by_key(|c| c.pid);
    v
}

fn maior_exec_do_modulo(maps: &str) -> u64 {
    maps.lines()
        .filter(|l| l.contains(MODULO))
        .filter_map(|l| {
            let mut it = l.splitn(3, ' ');
            let faixa = it.next()?;
            if !it.next()?.contains('x') {
                return None;
            }
            let (a, b) = faixa.split_once('-')?;
            let ini = u64::from_str_radix(a, 16).ok()?;
            let fim = u64::from_str_radix(b, 16).ok()?;
            Some(fim.saturating_sub(ini))
        })
        .max()
        .unwrap_or(0)
}

/// Escolhe o cliente padrão: maior segmento executável; empate fica com o menor
/// pid (determinístico, não depende da ordem do readdir). `excluidos` são os
/// pids que já pertencem a outro motor.
pub fn escolher_candidato(candidatos: &[Candidato], excluidos: &[i32]) -> Option<i32> {
    candidatos
        .iter()
        .filter(|c| !excluidos.contains(&c.pid))
        .max_by_key(|c| (c.tam_exec, std::cmp::Reverse(c.pid)))
        .map(|c| c.pid)
}

/// Procura o processo que tem `libroblox.so` mapeado, preferindo o que tem o
/// maior segmento executável (o cliente de verdade, não um helper de preload).
pub fn achar_pid() -> Option<i32> {
    escolher_candidato(&candidatos_payload(), &[])
}

/// Instante de criação do processo (`starttime`, campo 22 de `/proc/<pid>/stat`).
/// O comm pode ter espaço e parêntese, então o parse parte do último `)`.
pub fn inicio_do_processo(pid: i32) -> Option<u64> {
    let txt = fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
    starttime_de(&txt)
}

/// Região só serve de esconderijo para o stub se nenhum byte nela tiver valor.
fn regiao_toda_zerada(bytes: &[u8]) -> bool {
    bytes.iter().all(|b| *b == 0)
}

fn starttime_de(stat: &str) -> Option<u64> {
    let resto = &stat[stat.rfind(')')? + 1..];
    // depois do ')' o primeiro campo é o 3º (estado); starttime é o 22º
    resto.split_whitespace().nth(19)?.parse().ok()
}

#[cfg(test)]
mod testes {
    use super::{
        escolher_candidato, regiao_toda_zerada, Alvo, Faixa, maior_exec_do_modulo, starttime_de,
        Candidato,
    };

    #[test]
    fn regiao_zerada_rejeita_tabela_com_zeros_espalhados() {
        assert!(regiao_toda_zerada(&[0u8; 4096]));
        let mut tabela = [0u8; 4096];
        // ~44% zeros (4 de cada 9 bytes), não-zero espalhados como na tabela de floats do .rodata
        for (i, b) in tabela.iter_mut().enumerate() {
            if i % 9 >= 4 {
                *b = 0x3f;
            }
        }
        assert!(!regiao_toda_zerada(&tabela));
        let mut so_o_ultimo = [0u8; 4096];
        so_o_ultimo[4095] = 1;
        assert!(!regiao_toda_zerada(&so_o_ultimo));
    }

    fn c(pid: i32, tam_exec: u64) -> Candidato {
        Candidato { pid, tam_exec }
    }

    #[test]
    fn escolhe_o_maior_segmento() {
        assert_eq!(escolher_candidato(&[c(10, 0x1000), c(20, 0x9000), c(30, 0x5000)], &[]), Some(20));
    }

    #[test]
    fn empate_fica_com_menor_pid() {
        assert_eq!(escolher_candidato(&[c(30, 0x9000), c(10, 0x9000), c(20, 0x9000)], &[]), Some(10));
    }

    #[test]
    fn exclui_pids_de_outros_motores() {
        let todos = [c(10, 0x9000), c(20, 0x5000)];
        assert_eq!(escolher_candidato(&todos, &[10]), Some(20));
        assert_eq!(escolher_candidato(&todos, &[10, 20]), None);
        assert_eq!(escolher_candidato(&[], &[]), None);
    }

    #[test]
    fn maps_so_conta_executavel_do_modulo() {
        let maps = "7f00-7f80 r-xp 0 00:00 0 /x/libroblox.so\n\
                    7f80-8f80 r-xp 0 00:00 0 /x/libroblox.so\n\
                    9000-ffff00 r-xp 0 00:00 0 /x/outra.so\n\
                    a000-b000 rw-p 0 00:00 0 /x/libroblox.so";
        assert_eq!(maior_exec_do_modulo(maps), 0x1000);
        assert_eq!(maior_exec_do_modulo("9000-a000 r-xp 0 00:00 0 /x/outra.so"), 0);
    }

    #[test]
    fn starttime_com_parenteses_no_comm() {
        let stat = "4242 (Main (x) y) S 1 4242 4242 0 -1 4194560 100 0 0 0 1 2 0 0 20 0 65 0 987654 1000 2 3";
        assert_eq!(starttime_de(stat), Some(987654));
    }

    #[test]
    fn regiao_gravavel_exige_faixa_rw_que_contenha() {
        let faixa = |ini, fim, perm: &str| Faixa {
            ini,
            fim,
            perm: perm.into(),
            offset: 0,
            caminho: String::new(),
        };
        let alvo = Alvo {
            pid: 1,
            base: 0,
            exec: (0, 0),
            caminho: String::new(),
            faixas: vec![faixa(0x1000, 0x5000, "rw-p"), faixa(0x8000, 0xa000, "r-xp")],
        };
        assert!(alvo.regiao_gravavel(0x1000, 0x4000));
        assert!(!alvo.regiao_gravavel(0x1000, 0x5000));
        assert!(alvo.regiao_gravavel(0x2000, 0x1000));
        assert!(!alvo.regiao_gravavel(0x4800, 0x1000));
        assert!(!alvo.regiao_gravavel(u64::MAX - 1, 0x1000));
        assert!(!alvo.regiao_gravavel(0x8000, 0x1000));
    }

    #[test]
    fn starttime_ausente() {
        assert_eq!(starttime_de("lixo"), None);
    }
}

/// Threads paradas por ptrace que são soltas quando o valor sai de escopo, em
/// qualquer caminho (erro, `?` ou pânico). Deixar uma thread anexada congela o
/// cliente inteiro.
pub struct Paradas<'a> {
    alvo: &'a Alvo,
    pub tids: Vec<i32>,
}

impl Drop for Paradas<'_> {
    fn drop(&mut self) {
        self.alvo.soltar_threads(&self.tids);
    }
}

impl Alvo {
    pub fn parar_com_guarda(&self) -> Paradas<'_> {
        Paradas { alvo: self, tids: self.parar_threads() }
    }

    pub fn novo(pid: i32) -> io::Result<Alvo> {
        let faixas = ler_maps(pid)?;
        let mut base = u64::MAX;
        let mut caminho = String::new();
        let mut exec = (0u64, 0u64);
        for f in &faixas {
            if !f.caminho.contains(MODULO) {
                continue;
            }
            // base = início do mapa menos o offset do arquivo; cobre o caso em que
            // o primeiro mapa do módulo não começa em offset 0.
            if f.ini >= f.offset {
                base = base.min(f.ini - f.offset);
            }
            if caminho.is_empty() {
                caminho = f.caminho.clone();
            }
            if f.perm.contains('x') && (f.fim - f.ini) > (exec.1 - exec.0) {
                exec = (f.ini, f.fim);
            }
        }
        if base == u64::MAX {
            return Err(io::Error::new(io::ErrorKind::NotFound, "payload not mapped"));
        }
        Ok(Alvo { pid, base, exec, caminho, faixas })
    }

    /// Relê `/proc/<pid>/maps`: o heap cresce e o mmap remoto cria faixas novas,
    /// e os testes de ponteiro (`dentro_do_processo`/`gravavel`) usam o snapshot.
    pub fn atualizar_faixas(&mut self) -> io::Result<()> {
        self.faixas = ler_maps(self.pid)?;
        Ok(())
    }

    pub fn ler(&self, addr: u64, buf: &mut [u8]) -> io::Result<()> {
        let local = Iovec { base: buf.as_mut_ptr(), len: buf.len() };
        let remoto = Iovec { base: addr as *mut u8, len: buf.len() };
        let n = unsafe { process_vm_readv(self.pid, &local, 1, &remoto, 1, 0) };
        if n == buf.len() as isize {
            Ok(())
        } else {
            Err(io::Error::last_os_error())
        }
    }

    pub fn escrever(&self, addr: u64, buf: &[u8]) -> io::Result<()> {
        let local = Iovec { base: buf.as_ptr() as *mut u8, len: buf.len() };
        let remoto = Iovec { base: addr as *mut u8, len: buf.len() };
        let n = unsafe { process_vm_writev(self.pid, &local, 1, &remoto, 1, 0) };
        if n == buf.len() as isize {
            Ok(())
        } else {
            Err(io::Error::last_os_error())
        }
    }

    pub fn u64_em(&self, addr: u64) -> io::Result<u64> {
        let mut b = [0u8; 8];
        self.ler(addr, &mut b)?;
        Ok(u64::from_le_bytes(b))
    }

    pub fn u32_em(&self, addr: u64) -> io::Result<u32> {
        let mut b = [0u8; 4];
        self.ler(addr, &mut b)?;
        Ok(u32::from_le_bytes(b))
    }

    pub fn bytes(&self, addr: u64, n: usize) -> io::Result<Vec<u8>> {
        let mut b = vec![0u8; n];
        self.ler(addr, &mut b)?;
        Ok(b)
    }

    /// RIP da thread, com ela já parada por ptrace.
    pub fn rip(&self, tid: i32) -> Option<u64> {
        #[repr(C)]
        struct R { v: [u64; 27] }
        extern "C" { fn ptrace(req: i32, pid: i32, addr: *mut u8, data: *mut u8) -> i64; }
        let mut r = R { v: [0; 27] };
        unsafe {
            if ptrace(12 /* GETREGS */, tid, std::ptr::null_mut(), &mut r as *mut R as *mut u8) != 0 {
                return None;
            }
        }
        Some(r.v[16])
    }

    pub fn tids(&self) -> Vec<i32> {
        let mut v = Vec::new();
        if let Ok(d) = fs::read_dir(format!("/proc/{}/task", self.pid)) {
            for e in d.flatten() {
                if let Ok(t) = e.file_name().to_string_lossy().parse() {
                    v.push(t);
                }
            }
        }
        v
    }

    /// Nome da thread (`comm`, até 15 caracteres). Serve para não escolher a
    /// thread principal/render para a chamada remota.
    pub fn nome_thread(&self, tid: i32) -> Option<String> {
        fs::read_to_string(format!("/proc/{}/task/{tid}/comm", self.pid))
            .ok()
            .map(|s| s.trim().to_string())
    }

    /// Para todas as threads (PTRACE_ATTACH) e devolve os tids parados.
    pub fn parar_threads(&self) -> Vec<i32> {
        let mut paradas = Vec::new();
        for tid in self.tids() {
            if unsafe { ptrace_attach(tid) } == 0 {
                let mut st = 0;
                unsafe { waitpid(tid, &mut st, 0) };
                if unsafe { wifstopped(st) } {
                    paradas.push(tid);
                } else {
                    unsafe { ptrace_detach(tid) };
                }
            }
        }
        paradas
    }

    pub fn soltar_threads(&self, tids: &[i32]) {
        for t in tids {
            unsafe { ptrace_detach(*t) };
        }
    }

    /// Primeira página inteira zerada dentro do segmento executável RWX do payload.
    pub fn pagina_livre(&self) -> Option<u64> {
        self.pagina_livre_contigua(1)
    }

    /// Primeiro bloco de `paginas` páginas consecutivas zeradas. O prelude da
    /// API ocupa vários KB, então o stub precisa de uma região contígua — não
    /// basta achar uma página solta e escrever além dela. Inteiro e não
    /// amostrado: tabelas de float do .rodata têm muitos zeros e enganam a amostra.
    pub fn pagina_livre_contigua(&self, paginas: usize) -> Option<u64> {
        let total = paginas * 4096;
        let mut buf = vec![0u8; total];
        let mut a = self.exec.0;
        while a + total as u64 <= self.exec.1 {
            if self.ler(a, &mut buf).is_ok() && regiao_toda_zerada(&buf) {
                return Some(a);
            }
            a += 4096;
        }
        None
    }

    /// Endereço cai numa faixa de memória do processo? Usado antes de seguir
    /// ponteiros lidos da memória (um valor podre viraria leitura de lixo).
    pub fn dentro_do_processo(&self, addr: u64) -> bool {
        self.faixas.iter().any(|f| addr >= f.ini && addr < f.fim)
    }

    /// Endereço cai numa faixa gravável? É o filtro do patch de código: o
    /// `code` de uma proto está no heap, nunca no segmento executável.
    pub fn gravavel(&self, addr: u64) -> bool {
        self.faixas
            .iter()
            .any(|f| addr >= f.ini && addr < f.fim && f.perm.starts_with("rw"))
    }

    /// `[addr, addr+tam)` cabe inteira numa faixa gravável? Serve para readotar
    /// um mmap nosso gravado antes sem confiar num valor podre. Contém em vez de
    /// casar o início: o kernel funde mmap anônimo vizinho numa faixa só.
    pub fn regiao_gravavel(&self, addr: u64, tam: u64) -> bool {
        addr.checked_add(tam).is_some_and(|fim| {
            self.faixas
                .iter()
                .any(|f| f.ini <= addr && fim <= f.fim && f.perm.starts_with("rw"))
        })
    }

    pub fn read_cstr(&self, addr: u64, max: usize) -> Option<String> {
        let b = self.bytes(addr, max).ok()?;
        let end = b.iter().position(|&c| c == 0).unwrap_or(b.len());
        Some(String::from_utf8_lossy(&b[..end]).to_string())
    }
}

// --- ligações mínimas de libc (evita depender do crate `libc`) ---------------

#[repr(C)]
struct Iovec {
    base: *mut u8,
    len: usize,
}

extern "C" {
    fn process_vm_readv(
        pid: i32,
        local: *const Iovec,
        liovcnt: usize,
        remote: *const Iovec,
        riovcnt: usize,
        flags: u64,
    ) -> isize;
    fn process_vm_writev(
        pid: i32,
        local: *const Iovec,
        liovcnt: usize,
        remote: *const Iovec,
        riovcnt: usize,
        flags: u64,
    ) -> isize;
    fn ptrace(req: i32, pid: i32, addr: *mut u8, data: *mut u8) -> i64;
    fn waitpid(pid: i32, status: *mut i32, options: i32) -> i32;
}

unsafe fn ptrace_attach(pid: i32) -> i64 {
    ptrace(16 /* PTRACE_ATTACH */, pid, std::ptr::null_mut(), std::ptr::null_mut())
}
unsafe fn ptrace_detach(pid: i32) -> i64 {
    ptrace(17 /* PTRACE_DETACH */, pid, std::ptr::null_mut(), std::ptr::null_mut())
}
unsafe fn wifstopped(st: i32) -> bool {
    (st & 0xff) == 0x7f
}
