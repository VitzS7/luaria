//! CLI do executor: resolve âncoras no payload, inspeciona funções e (mais adiante)
//! anexa no cliente. Sem GTK — é a bancada de medição.

use std::collections::HashSet;
use std::time::{Duration, Instant};

use luaria::elf::Elf;
use luaria::scan;
use luaria::vm;

const OCIOSO_PONTE: Duration = Duration::from_secs(5);

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.is_empty() {
        uso();
        std::process::exit(2);
    }
    match args[0].as_str() {
        "resolve" => cmd_resolve(&args[1..]),
        "fn" => cmd_fn(&args[1..]),
        "xrefs" => cmd_xrefs(&args[1..]),
        "strings" => cmd_strings(&args[1..]),
        "watch" | "run" => cmd_watch(&args[1..]),
        "cands" => cmd_cands(&args[1..]),
        "plt" => cmd_plt(&args[1..]),
        "rawrun" => cmd_rawrun(&args[1..]),
        "fns" => cmd_fns(&args[1..]),
        "cands2" => cmd_cands2(&args[1..]),
        "fcall" => cmd_fcall(&args[1..]),
        "around" => cmd_around(&args[1..]),
        "findglobal" => cmd_findglobal(&args[1..]),
        "prof" => cmd_prof(&args[1..]),
        "exec" => cmd_exec(&args[1..]),
        "servir" => cmd_servir(&args[1..]),
        "captura" => cmd_captura(&args[1..]),
        "opcodes" => cmd_opcodes(&args[1..]),
        "chunk" => cmd_chunk(&args[1..]),
        "callers" => cmd_callers(&args[1..]),
        _ => {
            uso();
            std::process::exit(2);
        }
    }
}

fn uso() {
    eprintln!(
        "usage:\n  \
         luaria-cli resolve [payload.so] [--json]\n  \
         luaria-cli fn <payload.so> <offset_hex> [n_bytes]\n  \
         luaria-cli xrefs <payload.so> <string|0xalvo>\n  \
         luaria-cli strings <payload.so> <texto>\n\n\
         No payload: uses the Mocktail current.json."
    );
}

fn carregar(arg: Option<&String>) -> Elf {
    match arg {
        Some(p) => Elf::open(p).unwrap_or_else(|e| {
            eprintln!("error opening {p}: {e}");
            std::process::exit(1);
        }),
        None => {
            let p = luaria::payload_ativo().unwrap_or_else(|| {
                eprintln!("no active payload (current.json missing); pass the path");
                std::process::exit(1);
            });
            Elf::open(&p).unwrap_or_else(|e| {
                eprintln!("error opening {}: {e}", p.display());
                std::process::exit(1);
            })
        }
    }
}

fn cmd_resolve(args: &[String]) {
    let json = args.iter().any(|a| a == "--json");
    let caminho = args.iter().find(|a| !a.starts_with("--"));
    let t0 = std::time::Instant::now();
    let elf = carregar(caminho);
    eprintln!("[luaria] {} ({} MB) in {:.0?}", elf.path, elf.data.len() / 1_048_576, t0.elapsed());

    let t1 = std::time::Instant::now();
    let a = vm::resolve(&elf);
    eprintln!("[luaria] resolution in {:.1?}", t1.elapsed());

    if json {
        println!("{}", serde_json::to_string_pretty(&a.to_json()).unwrap());
    } else {
        print!("{}", a.resumo());
    }
}

fn cmd_fn(args: &[String]) {
    if args.len() < 2 {
        uso();
        std::process::exit(2);
    }
    let elf = carregar(Some(&args[0]));
    let off = parse_hex(&args[1]);
    let n: u64 = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(0x400);
    let bytes = match elf.bytes_at(off, n as usize) {
        Some(b) => b,
        None => {
            eprintln!("offset outside the file");
            std::process::exit(1);
        }
    };
    use iced_x86::{Decoder, DecoderOptions, Formatter, NasmFormatter};
    let mut dec = Decoder::with_ip(64, bytes, off, DecoderOptions::NONE);
    let mut fmt = NasmFormatter::new();
    let mut s = String::new();
    while dec.can_decode() {
        let instr = dec.decode();
        s.clear();
        fmt.format(&instr, &mut s);
        println!("{:#010x}  {s}", instr.ip());
        if instr.flow_control() == iced_x86::FlowControl::Return {
            break;
        }
    }
}

fn cmd_xrefs(args: &[String]) {
    if args.len() < 2 {
        uso();
        std::process::exit(2);
    }
    let elf = carregar(Some(&args[0]));
    let alvo = if args[1].starts_with("0x") {
        parse_hex(&args[1])
    } else {
        let hits = scan::find_cstr(&elf, &args[1]);
        if hits.is_empty() {
            eprintln!("string not found");
            std::process::exit(1);
        }
        for h in &hits {
            println!("string at {h:#x}");
        }
        hits[0]
    };
    let mut alvos = HashSet::new();
    alvos.insert(alvo);
    let xs = scan::xrefs_to(&elf, &alvos);
    for x in &xs {
        let f = scan::find_fn_start(&elf, x.from);
        println!("{:#x} -> {:#x} {:?}  fn_start={}", x.from, x.target, x.kind, f.map(|v| format!("{v:#x}")).unwrap_or_default());
    }
    println!("total: {}", xs.len());
}

fn cmd_strings(args: &[String]) {
    if args.len() < 2 {
        uso();
        std::process::exit(2);
    }
    let elf = carregar(Some(&args[0]));
    for h in scan::find_cstr(&elf, &args[1]) {
        println!("{h:#x}  {:?}", elf.read_cstr_at(h, 200).unwrap_or_default());
    }
}

/// Procura funções com a assinatura de `lua_pcall`: pequena, exatamente uma
/// chamada direta, lê `L->top` (+0x10) e desloca o topo por TValue de 16 bytes.
fn cmd_cands(args: &[String]) {
    let elf = carregar(args.first());
    let padrao: [u8; 4] = [0x48, 0x8B, 0x47, 0x10];
    let mut vistos = std::collections::HashSet::new();
    println!("[cands] scanning lua_pcall signature...");
    for (ini, fim) in elf.exec_ranges() {
        let off = match elf.va_to_off(ini) {
            Some(o) => o as usize,
            None => continue,
        };
        let len = (fim - ini) as usize;
        if off + len > elf.data.len() {
            continue;
        }
        for hit in memchr::memmem::find_iter(&elf.data[off..off + len], &padrao) {
            let va = ini + hit as u64;
            let start = match luaria::scan::find_fn_start(&elf, va) {
                Some(s) => s,
                None => continue,
            };
            if !vistos.insert(start) {
                continue;
            }
            let instrs = luaria::scan::decode_function(&elf, start, 0x300);
            if instrs.is_empty() || instrs.len() > 40 {
                continue;
            }
            let calls: Vec<u64> = instrs
                .iter()
                .filter(|i| i.flow_control() == iced_x86::FlowControl::Call)
                .map(|i| i.near_branch64())
                .collect();
            if calls.len() != 1 || !elf.in_exec(calls[0]) {
                continue;
            }
            // deslocamento de TValue (16 bytes): aparece como multiplicação por 16 no código
            let mexe_top = instrs.iter().any(|i| {
                let m = i.mnemonic();
                let nome = format!("{m:?}");
                (nome.contains("Shl") && i.immediate8() == 4) || nome.contains("Neg")
            });
            if !mexe_top {
                continue;
            }
            println!(
                "[cand] {start:#x}  instrs={}  call->{:#x}  tamanho~{}",
                instrs.len(),
                calls[0],
                (instrs.last().unwrap().next_ip()).saturating_sub(start)
            );
            for i in instrs.iter().take(24) {
                use iced_x86::{Formatter, NasmFormatter};
                let mut f = NasmFormatter::new();
                let mut s = String::new();
                f.format(i, &mut s);
                println!("     {:#x}  {s}", i.ip());
            }
        }
    }
}

fn cmd_plt(args: &[String]) {
    let elf = carregar(args.first());
    let imp = luaria::plt::Imports::load(&elf);
    let filtro = args.iter().skip(1).find(|a| !a.starts_with("--"));
    let mut v: Vec<(&u64, &String)> = imp.plt.iter().collect();
    v.sort_by_key(|(a, _)| **a);
    for (a, n) in v {
        if let Some(f) = filtro {
            if !n.contains(f.as_str()) {
                continue;
            }
        }
        println!("{a:#x}  {n}");
    }
}

fn cmd_callers(args: &[String]) {
    let elf = carregar(Some(&args[0]));
    let imp = luaria::plt::Imports::load(&elf);
    let alvo = if args[1].starts_with("0x") {
        parse_hex(&args[1])
    } else {
        imp.stub(&args[1]).unwrap_or_else(|| erro(&format!("import not found: {}", args[1])))
    };
    println!("[callers] alvo {alvo:#x}");
    let tabela = luaria::xref::XrefTable::build(&elf);
    let mut fns = std::collections::BTreeSet::new();
    for c in tabela.chamadores(alvo) {
        if let Some(f) = luaria::scan::find_fn_start(&elf, c) {
            fns.insert(f);
        }
        println!("  chamada em {c:#x} (fn {:#x})", luaria::scan::find_fn_start(&elf, c).unwrap_or(0));
    }
    println!("[callers] functions: {}", fns.len());
    for f in fns.iter().take(40) {
        println!("  fn {f:#x}");
    }
}

fn cmd_watch(args: &[String]) {
    let mut pid: Option<i32> = None;
    let mut off: Option<u64> = None;
    let mut segs: i64 = 120;
    let mut src: Option<String> = None;
    let mut so_armar = false;
    let mut disarmar = false;
    let mut dump = false;
    let mut sempre = false;
    let mut so_com_f = false;
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--src" => {
                src = args.get(i + 1).cloned();
                i += 2;
            }
            "--set-src" => {
                src = args.get(i + 1).cloned();
                so_armar = true;
                i += 2;
            }
            "--disarm" => {
                disarmar = true;
                i += 1;
            }
            "--dump" => {
                dump = true;
                i += 1;
            }
            "--f-nulo" => {
                so_com_f = true;
                i += 1;
            }
            "--sempre" => {
                sempre = true;
                i += 1;
            }
            a => {
                if pid.is_none() {
                    pid = a.parse().ok();
                } else if off.is_none() {
                    off = Some(parse_hex(a));
                } else if let Ok(v) = a.parse() {
                    segs = v;
                }
                i += 1;
            }
        }
    }

    let pid = pid
        .or_else(luaria::process::achar_pid)
        .unwrap_or_else(|| erro("Mocktail client not found (libroblox has no map)"));
    let alvo = luaria::process::Alvo::novo(pid)
        .unwrap_or_else(|e| erro(&format!("could not open pid {pid}: {e}")));
    let elf = luaria::elf::Elf::open(&alvo.caminho)
        .unwrap_or_else(|e| erro(&format!("unreadable payload ({}): {e}", alvo.caminho)));
    let off = match off {
        Some(o) => o,
        None => {
            let tabela = luaria::xref::XrefTable::build(&elf);
            match luaria::vm::resolve_luau_load(&elf, &tabela) {
                Some((load, wrapper)) => {
                    println!("[watch] luau_load +{load:#x} (wrapper +{wrapper:#x})");
                    load
                }
                None => erro("luau_load not resolved; pass the offset"),
            }
        }
    };

    let fonte = src.as_ref().map(|p| {
        std::fs::read(p).unwrap_or_else(|e| erro(&format!("could not read {p}: {e}")))
    });

    let hook = match luaria::hook::Hook::adotar(&alvo, off) {
        Ok(h) => {
            println!("[watch] hook already installed (stub {:#x})", h.stub);
            if disarmar {
                h.desarmar(&alvo).ok();
                println!("[watch] fonte desarmada");
                return;
            }
            if let Some(f) = &fonte {
                h.armar(&alvo, f).ok();
                println!("[watch] source armed ({} bytes) on existing hook", f.len());
            }
            h
        }
        Err(_) => {
            let h = luaria::hook::Hook::instalar(&alvo, &elf, off, fonte.as_deref(), sempre)
                .unwrap_or_else(|e| erro(&format!("failed to install hook: {e}")));
            println!(
                "[watch] hook installed at {:#x} (prologue {} bytes, stub {:#x})",
                h.fn_addr, h.prologo, h.stub
            );
            if let Some(f) = &fonte {
                println!("[watch] fonte armada ({} bytes)", f.len());
            }
            h
        }
    };
    if so_armar && fonte.is_none() {
        erro("--set-src needs a file");
    }

    let indice = luaria::eh_frame::Indice::novo(&elf);
    println!("[watch] watching for {segs}s — join an experience to generate script load");
    let inicio = std::time::Instant::now();
    let mut ultima_conta = hook.contador(&alvo).unwrap_or(0);
    while inicio.elapsed().as_secs() < segs as u64 {
        let conta = match hook.contador(&alvo) {
            Ok(c) => c,
            Err(_) => break,
        };
        if conta == ultima_conta {
            std::thread::sleep(std::time::Duration::from_millis(20));
            continue;
        }
        ultima_conta = conta;
        let a = match hook.args(&alvo) {
            Ok(a) => a,
            Err(_) => continue,
        };
        let l = a[0];
        if l == 0 {
            continue;
        }
        // --f-nulo: só interessa a chamada que INICIA uma função (rsi != 0);
        // as de rsi == 0 são retomada de script já carregado
        if so_com_f && a[1] == 0 {
            continue;
        }
        let tam = a[3];
        let rv = hook.retval(&alvo).unwrap_or(0);
        let ret = hook.retorno(&alvo).unwrap_or(0);
        let chamador = if ret > alvo.base && ret < alvo.exec.1 {
            let off = ret - alvo.base;
            format!("chamador=+{off:#x} (fn +{:#x})", indice.inicio(off).unwrap_or(0))
        } else {
            String::from("caller=outside the payload")
        };
        if dump {
            for (k, nome) in ["rdi", "rsi", "rdx", "rcx", "r8", "r9"].iter().enumerate() {
                let p = a[k];
                let b = alvo.bytes(p, 32).unwrap_or_default();
                let hex: Vec<String> = b.iter().take(24).map(|x| format!("{x:02x}")).collect();
                println!("  {nome}={p:#x} {}", hex.join(" "));
            }
        }
        println!(
            "[{:>6.2}s] call L={:#x} tam={} ret={:#x} {chamador}",
            inicio.elapsed().as_secs_f64(),
            l,
            tam,
            rv
        );
        uso_stdout();
    }
}

fn uso_stdout() {
    use std::io::Write;
    std::io::stdout().flush().ok();
}

fn erro(msg: &str) -> ! {
    eprintln!("[erro] {msg}");
    std::process::exit(1);
}

fn parse_hex(s: &str) -> u64 {    let s = s.trim().trim_start_matches("0x");
    u64::from_str_radix(s, 16).unwrap_or_else(|_| {
        eprintln!("invalid hex: {s}");
        std::process::exit(2);
    })
}

/// Acha `luaD_rawrunprotected` (chama setjmp e depois a função de trabalho por
/// ponteiro) e sobe a cadeia até `luaD_pcall` / `lua_pcall`.
fn cmd_rawrun(args: &[String]) {
    let elf = carregar(args.iter().find(|a| !a.starts_with("--") && !a.starts_with("0x")));
    let imp = luaria::plt::Imports::load(&elf);
    let setjmp = imp.stub("setjmp").unwrap_or_else(|| erro("no setjmp import"));
    let tabela = luaria::xref::XrefTable::build(&elf);
    let idx = luaria::eh_frame::Indice::novo(&elf);
    let mut fns = std::collections::BTreeSet::new();
    for c in tabela.chamadores(setjmp) {
        if let Some(f) = idx.inicio(c) {
            fns.insert(f);
        }
    }
    println!("[rawrun] {} functions call setjmp (.eh_frame limits)", fns.len());
    for f in fns {
        let fim = idx.fim(f).unwrap_or(f + 0x400);
        let instrs = luaria::scan::decode_bytes(
            elf.bytes_at(f, (fim - f) as usize).unwrap_or(&[]),
            f,
        );
        let indireto: Vec<u64> = instrs
            .iter()
            .filter(|i| {
                i.flow_control() == iced_x86::FlowControl::IndirectBranch
                    || i.flow_control() == iced_x86::FlowControl::IndirectCall
            })
            .map(|i| i.ip())
            .collect();
        if indireto.is_empty() {
            continue;
        }
        let args = {
            // conta pushes de registradores salvos como pista de número de args
            let mut r = std::collections::BTreeSet::new();
            for i in instrs.iter().take(12) {
                if i.op_count() > 0 && i.op0_kind() == iced_x86::OpKind::Register {
                    r.insert(format!("{:?}", i.op0_register()));
                }
            }
            r.len()
        };
        println!(
            "[rawrun] {f:#x} instrs={} call_indireto={:?} regs={args}",
            instrs.len(),
            indireto.iter().map(|a| format!("{a:#x}")).collect::<Vec<_>>()
        );
        for i in instrs.iter().take(12) {
            use iced_x86::{Formatter, NasmFormatter};
            let mut fm = NasmFormatter::new();
            let mut s = String::new();
            fm.format(i, &mut s);
            println!("      {:#x}  {s}", i.ip());
        }
    }
}

fn cmd_fns(args: &[String]) {
    let elf = carregar(args.iter().find(|a| !a.starts_with("--") && !a.starts_with("0x")));
    let idx = luaria::eh_frame::Indice::novo(&elf);
    println!("[fns] {} functions in the .eh_frame table", idx.funcoes.len());
    for a in args.iter().filter(|a| a.starts_with("0x")) {
        let v = parse_hex(a);
        match idx.contendo(v) {
            Some(f) => println!("  {v:#x} -> inicio {:#x} fim {:#x} ({} bytes)", f.ini, f.fim, f.fim - f.ini),
            None => println!("  {v:#x} -> outside a function"),
        }
    }
    // estatística de tamanhos
    let mut pequenas = 0;
    for f in &idx.funcoes {
        if f.fim - f.ini < 0x100 {
            pequenas += 1;
        }
    }
    println!("[fns] functions < 256 bytes: {pequenas}");
}

/// Candidatos a `lua_pcall`: função pequena, exatamente uma chamada direta,
/// lê `L->top` (+0x10) e monta um struct de chamada na pilha.
fn cmd_cands2(args: &[String]) {
    let elf = carregar(args.first().filter(|a| !a.starts_with("--")));
    let idx = luaria::eh_frame::Indice::novo(&elf);
    let tabela = luaria::xref::XrefTable::build(&elf);
    let padrao: [u8; 4] = [0x48, 0x8B, 0x47, 0x10];
    let mut vistos = std::collections::BTreeSet::new();
    let mut achados: Vec<(usize, u64, u64, u64, u64)> = Vec::new();
    for (ini, fim) in elf.exec_ranges() {
        let off = match elf.va_to_off(ini) {
            Some(o) => o as usize,
            None => continue,
        };
        let len = (fim - ini) as usize;
        if off + len > elf.data.len() {
            continue;
        }
        for hit in memchr::memmem::find_iter(&elf.data[off..off + len], &padrao) {
            let va = ini + hit as u64;
            let f = match idx.contendo(va) {
                Some(f) => f,
                None => continue,
            };
            if f.fim - f.ini > 0x120 {
                continue;
            }
            if !vistos.insert(f.ini) {
                continue;
            }
            let instrs = luaria::scan::decode_bytes(
                elf.bytes_at(f.ini, (f.fim - f.ini) as usize).unwrap_or(&[]),
                f.ini,
            );
            let calls: Vec<u64> = instrs
                .iter()
                .filter(|i| i.flow_control() == iced_x86::FlowControl::Call)
                .map(|i| i.near_branch64())
                .collect();
            if calls.len() != 1 || !elf.in_exec(calls[0]) {
                continue;
            }
            let alvo_tam = idx
                .contendo(calls[0])
                .map(|c| c.fim - c.ini)
                .unwrap_or(0);
            let chamadores = tabela.chamadores(f.ini).len();
            achados.push((chamadores, f.ini, f.fim - f.ini, calls[0], alvo_tam));
        }
    }
    achados.sort_unstable_by(|a, b| b.0.cmp(&a.0));
    for (n, ini, tam, alvo, alvo_tam) in achados.iter().take(25) {
        println!("[cand2] chamadores={n:<5} {ini:#x} tam={tam} alvo={alvo:#x} (alvo {alvo_tam} bytes)");
    }
}

/// Acha `f_call` (o callback estático que `lua_pcall` passa para `luaD_pcall`):
/// função minúscula que lê `[rsi]` e `[rsi+8]` e faz uma única chamada.
fn cmd_fcall(args: &[String]) {
    let elf = carregar(args.first().filter(|a| !a.starts_with("--")));
    let idx = luaria::eh_frame::Indice::novo(&elf);
    let tabela = luaria::xref::XrefTable::build(&elf);
    let mut achados = Vec::new();
    for f in &idx.funcoes {
        let tam = f.fim - f.ini;
        if tam == 0 || tam > 0x60 {
            continue;
        }
        let bytes = match elf.bytes_at(f.ini, tam as usize) {
            Some(b) => b,
            None => continue,
        };
        let instrs = luaria::scan::decode_bytes(bytes, f.ini);
        let calls: Vec<u64> = instrs
            .iter()
            .filter(|i| i.flow_control() == iced_x86::FlowControl::Call)
            .map(|i| i.near_branch64())
            .collect();
        if calls.len() != 1 || !elf.in_exec(calls[0]) {
            continue;
        }
        // lê dois campos de [rsi] (struct { func, nresults })
        let mut leu0 = false;
        let mut leu8 = false;
        for i in &instrs {
            use iced_x86::{Mnemonic, OpKind, Register};
            if !matches!(i.mnemonic(), Mnemonic::Mov | Mnemonic::Lea) {
                continue;
            }
            for k in 0..i.op_count() {
                if i.op_kind(k) != OpKind::Memory {
                    continue;
                }
                if i.memory_base() == Register::RSI || i.memory_base() == Register::RDX
                    || i.memory_base() == Register::RCX
                {
                    let d = i.memory_displacement64();
                    if d == 0 {
                        leu0 = true;
                    } else if d == 8 {
                        leu8 = true;
                    }
                }
            }
        }
        if !(leu0 && leu8) {
            continue;
        }
        let tam_alvo = idx.contendo(calls[0]).map(|c| c.fim - c.ini).unwrap_or(0);
        achados.push((f.ini, tam, calls[0], tam_alvo));
    }
    println!("[fcall] {} candidatos", achados.len());
    for (ini, tam, alvo, tam_alvo) in &achados {
        let refs = tabela.referencias(*ini);
        println!(
            "[fcall] {ini:#x} tam={tam} chama {alvo:#x} ({tam_alvo} bytes) refs={}",
            refs.len()
        );
        for r in refs.iter().take(6) {
            let quem = idx.inicio(*r).unwrap_or(0);
            println!("        ref at {r:#x} (fn {quem:#x})");
        }
    }
}

/// Lista as funções da tabela num raio de endereço (para mapear vizinhanças).
fn cmd_around(args: &[String]) {
    let elf = carregar(args.iter().find(|a| !a.starts_with("--") && !a.starts_with("0x")));
    let alvo = parse_hex(&args.iter().find(|a| a.starts_with("0x")).cloned().unwrap_or_else(|| erro("pass 0xaddress")));
    let raio: u64 = args
        .iter()
        .find(|a| a.starts_with("--raio="))
        .and_then(|a| a.trim_start_matches("--raio=").parse().ok())
        .unwrap_or(0x4000);
    let idx = luaria::eh_frame::Indice::novo(&elf);
    for f in &idx.funcoes {
        if f.ini + raio < alvo || f.ini > alvo + raio {
            continue;
        }
        let tam = f.fim - f.ini;
        let primeira = elf
            .bytes_at(f.ini, 8)
            .map(|b| b.iter().map(|x| format!("{x:02x}")).collect::<Vec<_>>().join(" "))
            .unwrap_or_default();
        println!("{:#x} tam={:<6} {primeira}", f.ini, tam);
    }
}

/// Acha uma função C da lib base lendo a memória viva:
/// string "nome" -> TString -> nó da tabela de globais -> closure -> ponteiro C.
fn cmd_findglobal(args: &[String]) {
    let nome = args
        .iter()
        .find(|a| !a.starts_with("--") && !a.starts_with("0x"))
        .cloned()
        .unwrap_or_else(|| erro("pass the global name (e.g. pcall)"));
    let pid = args
        .iter()
        .find(|a| a.starts_with("--pid="))
        .and_then(|a| a.trim_start_matches("--pid=").parse().ok())
        .or_else(luaria::process::achar_pid)
        .unwrap_or_else(|| erro("client not found"));
    let alvo = luaria::process::Alvo::novo(pid).unwrap_or_else(|e| erro(&format!("{e}")));

    println!("[glob] pid={pid} base={:#x} searching for {nome:?}", alvo.base);

    // 1. todas as ocorrências da string na memória (candidatas a TString)
    let needle = nome.as_bytes();
    let mut tstrings = Vec::new();
    let mut total_lido = 0u64;
    for f in alvo.faixas.iter().filter(|f| f.perm.starts_with('r')) {
        let mut a = f.ini;
        while a < f.fim {
            let n = (f.fim - a).min(1 << 20) as usize;
            let mut buf = vec![0u8; n];
            if alvo.ler(a, &mut buf).is_ok() {
                total_lido += n as u64;
                for hit in memchr::memmem::find_iter(&buf, needle) {
                    let va = a + hit as u64;
                    // confere o TString: len em +0xc, dados em +0x10
                    if let Ok(len) = alvo.u32_em(va.wrapping_sub(0x10) + 0xc) {
                        if len as usize == nome.len() {
                            tstrings.push(va - 0x10);
                        }
                    }
                }
            }
            a += n as u64;
        }
    }
    tstrings.sort_unstable();
    tstrings.dedup();
    println!("[glob] li {} MB, {} TStrings candidatas", total_lido / 1_048_576, tstrings.len());

    // 2. nós de tabela: qword == TString* seguido de tt == 5 (LUA_TSTRING)
    for ts in tstrings.iter().take(64) {
        let pat = ts.to_le_bytes();
        for f in alvo.faixas.iter().filter(|f| f.perm.starts_with('r')) {
            let mut a = f.ini;
            while a < f.fim {
                let n = (f.fim - a).min(1 << 20) as usize;
                let mut buf = vec![0u8; n];
                if alvo.ler(a, &mut buf).is_ok() {
                    for hit in memchr::memmem::find_iter(&buf, &pat) {
                        let pos = a + hit as u64;
                        // tt da chave em pos+8
                        if alvo.u32_em(pos + 8).unwrap_or(0) != 5 {
                            continue;
                        }
                        // valor em pos+0x10
                        let vtt = alvo.u32_em(pos + 0x18).unwrap_or(0);
                        let vptr = alvo.u64_em(pos + 0x10).unwrap_or(0);
                        if vtt != 6 && vtt != 7 {
                            continue; // função (C ou Lua)
                        }
                        println!("[glob] node {pos:#x}: value tt={vtt} ptr={vptr:#x}");
                        // closure: procura um ponteiro de código nos primeiros 0x40 bytes
                        for off in (0..0x40).step_by(8) {
                            if let Ok(p) = alvo.u64_em(vptr + off) {
                                if p > alvo.base && p < alvo.exec.1 {
                                    println!("        closure+{off:#x} -> code {p:#x} (offset +{:#x})", p - alvo.base);
                                }
                            }
                        }
                    }
                }
                a += n as u64;
            }
        }
    }
}

/// Amostra o RIP de todas as threads e monta um histograma por função.
/// Serve para achar o interpretador (função mais quente) sem símbolos.
fn cmd_prof(args: &[String]) {
    let segs: u64 = args
        .iter()
        .find(|a| !a.starts_with("--") && !a.starts_with("0x"))
        .and_then(|a| a.parse().ok())
        .unwrap_or(10);
    let pid = args
        .iter()
        .find(|a| a.starts_with("--pid="))
        .and_then(|a| a.trim_start_matches("--pid=").parse().ok())
        .or_else(luaria::process::achar_pid)
        .unwrap_or_else(|| erro("client not found"));
    let alvo = luaria::process::Alvo::novo(pid).unwrap_or_else(|e| erro(&format!("{e}")));
    let elf = luaria::elf::Elf::open(&alvo.caminho).unwrap_or_else(|e| erro(&format!("{e}")));
    let idx = luaria::eh_frame::Indice::novo(&elf);

    println!("[prof] sampling pid={pid} for {segs}s");
    let mut hist: std::collections::HashMap<u64, u64> = std::collections::HashMap::new();
    let mut retornos: std::collections::HashMap<u64, u64> = std::collections::HashMap::new();
    let mut total = 0u64;
    let fim = std::time::Instant::now() + std::time::Duration::from_secs(segs);
    while std::time::Instant::now() < fim {
        for tid in alvo.tids() {
            if let Some((rip, rsp)) = regs_rip(tid) {
                if rip >= alvo.exec.0 && rip < alvo.exec.1 {
                    // a tabela .eh_frame é indexada por endereço de arquivo
                    let off = rip - alvo.base;
                    let base = idx.inicio(off).unwrap_or(off);
                    *hist.entry(base).or_insert(0) += 1;
                    total += 1;
                    if base == 0x66fa0df {
                        let mut buf = [0u8; 512];
                        if alvo.ler(rsp, &mut buf).is_ok() {
                            for i in 0..64 {
                                let q = u64::from_le_bytes(buf[i * 8..i * 8 + 8].try_into().unwrap());
                                if q > alvo.exec.0 && q < alvo.exec.1 {
                                    let off = q - alvo.base;
                                    let f = idx.inicio(off).unwrap_or(off);
                                    *retornos.entry(f).or_insert(0) += 1;
                                }
                            }
                        }
                    }
                }
            }
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
    let mut v: Vec<(u64, u64)> = hist.into_iter().collect();
    v.sort_unstable_by(|a, b| b.1.cmp(&a.1));
    println!("[prof] {} samples in payload", total);
    for (f, n) in v.iter().take(15) {
        let tam = idx.fim(*f).unwrap_or(0).saturating_sub(*f);
        let pct = 100.0 * *n as f64 / total.max(1) as f64;
        println!("  {f:#x} tam={tam:<7} {n:>6} amostras ({pct:.1}%)");
    }
    let mut r: Vec<(u64, u64)> = retornos.into_iter().collect();
    r.sort_unstable_by(|a, b| b.1.cmp(&a.1));
    println!("[prof] returns on the luau_execute stack (who calls the interpreter):");
    for (f, n) in r.iter().take(10) {
        let tam = idx.fim(*f).unwrap_or(0).saturating_sub(*f);
        println!("  {f:#x} tam={tam:<7} {n:>5}x");
    }
}

fn regs_rip(tid: i32) -> Option<(u64, u64)> {
    #[repr(C)]
    #[derive(Default)]
    struct UserRegs {
        r15: u64, r14: u64, r13: u64, r12: u64, rbp: u64, rbx: u64, r11: u64, r10: u64,
        r9: u64, r8: u64, rax: u64, rcx: u64, rdx: u64, rsi: u64, rdi: u64, orig_rax: u64,
        rip: u64, cs: u64, eflags: u64, rsp: u64, ss: u64, fs_base: u64, gs_base: u64,
        ds: u64, es: u64, fs: u64, gs: u64,
    }
    extern "C" {
        fn ptrace(req: i32, pid: i32, addr: *mut u8, data: *mut u8) -> i64;
    }
    let mut regs = UserRegs::default();
    unsafe {
        if ptrace(16, tid, std::ptr::null_mut(), std::ptr::null_mut()) != 0 {
            return None;
        }
        let mut st = 0;
        extern "C" {
            fn waitpid(pid: i32, status: *mut i32, options: i32) -> i32;
        }
        waitpid(tid, &mut st, 0);
        let ok = ptrace(
            12,
            tid,
            std::ptr::null_mut(),
            &mut regs as *mut UserRegs as *mut u8,
        ) == 0;
        ptrace(17, tid, std::ptr::null_mut(), std::ptr::null_mut());
        if ok {
            Some((regs.rip, regs.rsp))
        } else {
            None
        }
    }
}

/// Executa um script Lua de verdade: luau_load + luaD_call por chamada remota,
/// com todas as threads paradas. O script é embrulhado em `pcall` pelo lado do
/// Lua, então erro de script não derruba o cliente.
fn cmd_exec(args: &[String]) {
    let caminho = args
        .iter()
        .find(|a| !a.starts_with("--") && !a.starts_with("0x"))
        .cloned()
        .unwrap_or_else(|| erro("usage: luaria-cli exec <script.lua> [--pid=N]"));
    let codigo = std::fs::read_to_string(&caminho)
        .unwrap_or_else(|e| erro(&format!("could not read {caminho}: {e}")));
    let mut motor = match args.iter().find(|a| a.starts_with("--pid=")) {
        Some(p) => {
            let pid: i32 = p.trim_start_matches("--pid=").parse().unwrap_or(0);
            luaria::engine::Motor::conectar_pid(pid)
        }
        None => luaria::engine::Motor::conectar(),
    }
    .unwrap_or_else(|e| erro(&e.to_string()));
    println!(
        "[exec] pid={} luau_load=+{:#x} luaD_call=+{:#x}",
        motor.pid,
        motor.anchors.luau_load.unwrap_or(0),
        motor.anchors.lua_d_call.unwrap_or(0)
    );
    match motor.executar(&codigo) {
        Ok(log) => {
            for l in log {
                println!("[exec] {l}");
            }
            println!("[exec] ok");
            servir_ponte(&motor, args.iter().any(|a| a == "--ficar"));
        }
        Err(e) => erro(&e.to_string()),
    }
}

/// Atende os pedidos HTTP do jogo até ficar ocioso. Sem `--ficar`, sai após `OCIOSO_PONTE`
/// sem pedido novo; com ele, só Ctrl-C encerra.
fn servir_ponte(motor: &luaria::engine::Motor, ficar: bool) {
    let Some(anel) = motor.anel_ponte() else { return };
    let vigia = match luaria::ponte::Vigia::iniciar(motor.pid, anel, luaria::ponte::atender_padrao) {
        Ok(vigia) => vigia,
        Err(luaria::ponte::ErroPonte::JaAtendido) => {
            println!("[ponte] the luaria UI already serves pid {}", motor.pid);
            return;
        }
        Err(e) => {
            println!("[ponte] did not start: {e:?}");
            return;
        }
    };
    let fim = if ficar { "Ctrl-C encerra" } else { "sai ocioso" };
    println!("[ponte] serving pid {} ({fim})", motor.pid);
    let mut ultimo_pedido = Instant::now();
    let mut atendidos = vigia.pedidos_atendidos();
    while !vigia.terminou() {
        std::thread::sleep(Duration::from_millis(100));
        if vigia.pedidos_atendidos() != atendidos {
            atendidos = vigia.pedidos_atendidos();
            ultimo_pedido = Instant::now();
        }
        if !ficar && ultimo_pedido.elapsed() >= OCIOSO_PONTE {
            break;
        }
    }
    match vigia.parar() {
        Ok(()) => println!("[ponte] ended after {atendidos} request(s)"),
        Err(e) => println!("[ponte] stopped: {e:?}"),
    }
}

fn cmd_captura(args: &[String]) {
    let pid: i32 = args
        .iter()
        .find_map(|a| a.strip_prefix("--pid="))
        .and_then(|v| v.parse().ok())
        .unwrap_or_else(|| erro("usage: luaria-cli captura --pid=<pid>"));
    let mut motor = luaria::engine::Motor::conectar_pid(pid).unwrap_or_else(|e| erro(&e.to_string()));
    let mut log = Vec::new();
    match motor.capturar_load(&mut log) {
        Ok(()) => {
            for l in log {
                println!("[captura] {l}");
            }
        }
        Err(e) => erro(&e.to_string()),
    }
}

/// Compila um script e mostra o chunk remapeado, sem precisar do cliente.
/// Serve para conferir o parser/remapeamento offline.
fn cmd_chunk(args: &[String]) {
    let caminho = args
        .iter()
        .find(|a| !a.starts_with("--"))
        .cloned()
        .unwrap_or_else(|| erro("usage: luaria-cli chunk <script.lua> [--sem-remapear]"));
    let codigo = std::fs::read_to_string(&caminho)
        .unwrap_or_else(|e| erro(&format!("could not read {caminho}: {e}")));
    let fonte = format!("return pcall(function()\n{codigo}\nend)\n");
    let bruto = luaria::engine::compilar(&fonte).unwrap_or_else(|e| erro(&e));
    println!("compilado: {} bytes", bruto.len());
    if args.iter().any(|a| a == "--sem-remapear") {
        hex_dump(&bruto);
        return;
    }
    let mapa = match args.iter().find(|a| a.starts_with("--mapa=")) {
        Some(m) => {
            // mapa lido de um arquivo "idx byte" por linha (saída de opcodes)
            let txt = std::fs::read_to_string(m.trim_start_matches("--mapa=")).unwrap_or_else(|e| erro(&e.to_string()));
            luaria::opcodes::Mapa::do_texto(&txt).unwrap_or_else(|e| erro(&e))
        }
        None => erro("missing --mapa=<file> (output of `luaria-cli opcodes`)"),
    };
    let saida = luaria::opcodes::remapear(&bruto, &mapa).unwrap_or_else(|e| erro(&e));
    let protos = luaria::opcodes::percorrer(&saida).unwrap_or_else(|e| erro(&e));
    for (i, p) in protos.iter().enumerate() {
        println!("proto {i}: {} palavras", p.sizecode);
        hex_dump(&saida[p.codigo.clone()]);
    }
}

fn hex_dump(b: &[u8]) {
    for linha in b.chunks(16) {
        let s: Vec<String> = linha.iter().map(|x| format!("{x:02x}")).collect();
        println!("  {}", s.join(" "));
    }
}

/// Mostra o embaralhamento de opcodes deste build: o byte que a VM do cliente
/// espera para cada opcode do compilador.
fn cmd_opcodes(args: &[String]) {
    let motor = match args.iter().find(|a| a.starts_with("--pid=")) {
        Some(p) => {
            let pid: i32 = p.trim_start_matches("--pid=").parse().unwrap_or(0);
            luaria::engine::Motor::conectar_pid(pid)
        }
        None => luaria::engine::Motor::conectar(),
    }
    .unwrap_or_else(|e| erro(&e.to_string()));
    let mapa = motor.mapa_opcodes().unwrap_or_else(|e| erro(&e.to_string()));
    for (i, nome) in luaria::opcodes::OPS_ENUM.iter().enumerate() {
        println!("{i:2} {nome:16} -> {:#04x}", mapa.por_enum[i]);
    }
}
/// Executa o script a cada `--intervalo` segundos com UM Motor só, até `--max`
/// execuções. Serve para medir a troca de jogo: o cliente é fechado e reaberto
/// por fora e o motor tem que se recuperar sozinho. `{N}` no script vira o
/// número da execução.
fn cmd_servir(args: &[String]) {
    let caminho = args
        .iter()
        .find(|a| !a.starts_with("--"))
        .cloned()
        .unwrap_or_else(|| erro("usage: luaria-cli servir <script.lua> [--intervalo=s] [--max=n]"));
    let modelo = std::fs::read_to_string(&caminho)
        .unwrap_or_else(|e| erro(&format!("could not read {caminho}: {e}")));
    let numero = |nome: &str, padrao: u64| {
        args.iter()
            .find_map(|a| a.strip_prefix(nome))
            .and_then(|v| v.parse().ok())
            .unwrap_or(padrao)
    };
    let intervalo = numero("--intervalo=", 5);
    let maximo = numero("--max=", 100);
    let mut motor: Option<luaria::engine::Motor> = None;
    for n in 1..=maximo {
        let codigo = modelo.replace("{N}", &n.to_string());
        let resultado = match motor.as_mut() {
            Some(m) => m.executar(&codigo).map(|_| ()),
            None => match luaria::engine::Motor::conectar() {
                Ok(mut m) => {
                    let r = m.executar(&codigo).map(|_| ());
                    motor = Some(m);
                    r
                }
                Err(e) => Err(e),
            },
        };
        match resultado {
            Ok(()) => println!("[servir] n={n} ok pid={}", motor.as_ref().map_or(0, |m| m.pid)),
            Err(e) => println!("[servir] n={n} error: {e}"),
        }
        std::thread::sleep(std::time::Duration::from_secs(intervalo));
    }
}
