//! Chamada remota de função dentro do processo: ptrace para parar a thread,
//! montar a pilha, trocar RIP e esperar o `int3` de retorno.
//!
//! Estado salvo e restaurado corretamente, com alinhamento de pilha explícito (SysV: na
//! entrada de função `rsp % 16 == 8`).

use std::io;

use crate::process::Alvo;

pub const PTRACE_ATTACH: i32 = 16;
pub const PTRACE_DETACH: i32 = 17;
pub const PTRACE_GETREGS: i32 = 12;
pub const PTRACE_SETREGS: i32 = 13;
pub const PTRACE_CONT: i32 = 7;
pub const PTRACE_KILL: i32 = 8;

#[repr(C)]
#[derive(Clone, Copy, Default, Debug)]
pub struct Regs {
    pub r15: u64,
    pub r14: u64,
    pub r13: u64,
    pub r12: u64,
    pub rbp: u64,
    pub rbx: u64,
    pub r11: u64,
    pub r10: u64,
    pub r9: u64,
    pub r8: u64,
    pub rax: u64,
    pub rcx: u64,
    pub rdx: u64,
    pub rsi: u64,
    pub rdi: u64,
    pub orig_rax: u64,
    pub rip: u64,
    pub cs: u64,
    pub eflags: u64,
    pub rsp: u64,
    pub ss: u64,
    pub fs_base: u64,
    pub gs_base: u64,
    pub ds: u64,
    pub es: u64,
    pub fs: u64,
    pub gs: u64,
}

extern "C" {
    fn ptrace(req: i32, pid: i32, addr: *mut u8, data: *mut u8) -> i64;
    fn waitpid(pid: i32, status: *mut i32, options: i32) -> i32;

}

fn regs(tid: i32) -> io::Result<Regs> {
    let mut r = Regs::default();
    let ok = unsafe { ptrace(PTRACE_GETREGS, tid, std::ptr::null_mut(), &mut r as *mut Regs as *mut u8) };
    if ok != 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(r)
}

fn setregs(tid: i32, r: &Regs) -> io::Result<()> {
    let ok = unsafe {
        ptrace(
            PTRACE_SETREGS,
            tid,
            std::ptr::null_mut(),
            r as *const Regs as *mut u8,
        )
    };
    if ok != 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

pub fn parar(tid: i32) -> io::Result<()> {
    if unsafe { ptrace(PTRACE_ATTACH, tid, std::ptr::null_mut(), std::ptr::null_mut()) } != 0 {
        return Err(io::Error::last_os_error());
    }
    let mut st = 0;
    unsafe { waitpid(tid, &mut st, 0) };
    if (st & 0xff) != 0x7f {
        return Err(io::Error::new(io::ErrorKind::Other, "thread did not stop"));
    }
    Ok(())
}

pub fn soltar(tid: i32) {
    unsafe { ptrace(PTRACE_DETACH, tid, std::ptr::null_mut(), std::ptr::null_mut()) };
}

pub struct Sessao {
    pub tid: i32,
    pub regs_originais: Regs,
    /// true quando fomos nós que anexamos (e portanto devemos soltar)
    proprio: bool,
}

impl Sessao {
    pub fn nova(tid: i32) -> io::Result<Sessao> {
        parar(tid)?;
        let regs_originais = regs(tid)?;
        Ok(Sessao { tid, regs_originais, proprio: true })
    }

    /// Usa uma thread que já está parada por `Alvo::parar_threads`.
    pub fn ja_parada(tid: i32) -> io::Result<Sessao> {
        let regs_originais = regs(tid)?;
        Ok(Sessao { tid, regs_originais, proprio: false })
    }

    /// Chama `fn_addr(args[0..6])` e devolve `rax`. A thread precisa estar parada.
    pub fn chamar(
        &self,
        alvo: &Alvo,
        fn_addr: u64,
        args: [u64; 6],
        trap: u64,
        scratch: u64,
    ) -> io::Result<u64> {
        let mut r = self.regs_originais;
        // pilha: 16 bytes alinhados, menos 8 para o endereço de retorno
        let mut rsp = (r.rsp & !0xF) - 0x200;
        rsp &= !0xF;
        rsp -= 8;
        alvo.escrever(rsp, &trap.to_le_bytes())?;
        // área de rascunho fica acima, sem pisar no que a função vai usar
        let _ = scratch;

        r.rdi = args[0];
        r.rsi = args[1];
        r.rdx = args[2];
        r.rcx = args[3];
        r.r8 = args[4];
        r.r9 = args[5];
        r.rsp = rsp;
        r.rip = fn_addr;
        r.rax = 0;
        setregs(self.tid, &r)?;
        unsafe { ptrace(PTRACE_CONT, self.tid, std::ptr::null_mut(), std::ptr::null_mut()) };

        let mut st = 0;
        unsafe { waitpid(self.tid, &mut st, 0) };
        let sig = st & 0xff;
        if sig != 0x7f || ((st >> 8) & 0xff) != 5 {
            return Err(io::Error::new(
                io::ErrorKind::Other,
                format!("call did not return via int3: status={st:#x} sig={sig}"),
            ));
        }
        let depois = regs(self.tid)?;
        Ok(depois.rax)
    }

    pub fn restaurar(&self) {
        let _ = setregs(self.tid, &self.regs_originais);
    }
}

impl Drop for Sessao {
    fn drop(&mut self) {
        self.restaurar();
        if self.proprio {
            soltar(self.tid);
        }
    }
}

/// Escreve um byte `int3` num endereço livre do payload e devolve o endereço.
pub fn preparar_trap(alvo: &Alvo, pg: u64) -> io::Result<u64> {
    let addr = pg + 0x110;
    alvo.escrever(addr, &[0xCC])?;
    Ok(addr)
}
