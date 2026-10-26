pub mod arquivos;
pub mod clientes;
pub mod conexao;
pub mod eh_frame;
pub mod estado_jogo;
pub mod sonda;
pub mod engine;
pub mod nativas;
pub mod elf;
pub mod hook;
pub mod luau;
pub mod opcodes;
pub mod plt;
pub mod ponte;
pub mod ponte_codec;
pub mod ponte_layout;
pub mod ponte_nativas;
pub mod prelude;
pub mod process;
pub mod remote;
pub mod remoto;
pub mod scan;
pub mod vm;
pub mod xref;

pub use elf::Elf;
pub use vm::Anchors;

/// Descobre o payload ativo lendo o `current.json` do Mocktail.
pub fn payload_ativo() -> Option<std::path::PathBuf> {
    let home = std::env::var("HOME").ok()?;
    let base = std::path::PathBuf::from(home).join(".var/app/space.bigrat.mocktail/data/mocktail");
    let txt = std::fs::read_to_string(base.join("current.json")).ok()?;
    let j: serde_json::Value = serde_json::from_str(&txt).ok()?;
    let rel = j.get("payload_path")?.as_str()?;
    let so = base.join(rel).join("libroblox.so");
    if so.exists() {
        Some(so)
    } else {
        None
    }
}
