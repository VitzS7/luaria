//! Lista de clientes Roblox por /proc, sem anexar nem ptrace.
use crate::process::{candidatos_payload, inicio_do_processo};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StatusCliente {
    Morto,
    NaoAnexado,
    /// Anexado, nenhum estado de jogo visto ainda.
    Anexado,
    Menu,
    /// Último PlaceId visto; só atualiza quando uma execução passa pela sonda.
    Jogo(u64),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InfoCliente {
    pub pid: i32,
    pub inicio: u64,
    pub tam_exec: u64,
}

pub fn listar_clientes() -> Vec<InfoCliente> {
    candidatos_payload()
        .into_iter()
        .filter_map(|c| {
            Some(InfoCliente { pid: c.pid, inicio: inicio_do_processo(c.pid)?, tam_exec: c.tam_exec })
        })
        .collect()
}

/// `anexado`: `Some((viu_app_sem_jogo, place))` do Estados do motor daquele pid; `None` = sem motor.
pub fn classificar(vivo: bool, anexado: Option<(bool, Option<u64>)>) -> StatusCliente {
    if !vivo {
        return StatusCliente::Morto;
    }
    match anexado {
        None => StatusCliente::NaoAnexado,
        Some((_, Some(p))) => StatusCliente::Jogo(p),
        Some((true, None)) => StatusCliente::Menu,
        Some((false, None)) => StatusCliente::Anexado,
    }
}

#[cfg(test)]
mod testes {
    use super::*;

    #[test]
    fn morto_ignora_o_resto() {
        assert_eq!(classificar(false, Some((true, Some(1)))), StatusCliente::Morto);
    }

    #[test]
    fn sem_motor_nao_anexado() {
        assert_eq!(classificar(true, None), StatusCliente::NaoAnexado);
    }

    #[test]
    fn place_visto_vira_jogo() {
        assert_eq!(classificar(true, Some((true, Some(42)))), StatusCliente::Jogo(42));
    }

    #[test]
    fn so_app_vira_menu() {
        assert_eq!(classificar(true, Some((true, None))), StatusCliente::Menu);
    }

    #[test]
    fn anexado_sem_estado_ainda() {
        assert_eq!(classificar(true, Some((false, None))), StatusCliente::Anexado);
    }
}
