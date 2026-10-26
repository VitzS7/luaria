//! Conexão com o cliente: valida os offsets fixos do payload e reconecta
//! quando o jogo troca (processo novo, pid reaproveitado, payload remapeado).

use crate::eh_frame::Indice;
use crate::engine::{
    ErroMotor, Motor, FN_CARREGAR, FN_CORPO_CALL, FN_EXECUTAR, FN_PRECALL, SITE_VERIFICACAO,
};
use crate::estado_jogo::{decidir, Decisao, Identidade, Motivo};
use crate::process::Alvo;

/// Offsets medidos por payload e o começo esperado de cada um. Payload novo =
/// bytes diferentes = falha alta antes de qualquer chamada.
const PROLOGOS: [(&str, u64, &[u8]); 4] = [
    ("luau_load", FN_CARREGAR, &[0x55, 0x48, 0x89, 0xe5, 0x41, 0x57, 0x41, 0x56, 0x41, 0x55]),
    ("corpo de luaD_call", FN_CORPO_CALL, &[0x55, 0x48, 0x89, 0xe5, 0x41, 0x57, 0x41, 0x56]),
    ("luaD_precall", FN_PRECALL, &[0x55, 0x48, 0x89, 0xe5, 0x41, 0x57, 0x41, 0x56, 0x41, 0x55]),
    ("luaV_execute", FN_EXECUTAR, &[0x80, 0x7f, 0x06, 0x00]),
];

impl Motor {
    /// Confere, no processo vivo, que os offsets fixos pertencem a funções do
    /// payload e começam como o esperado.
    pub(crate) fn validar_ancoras(alvo: &Alvo, indice: &Indice) -> Result<(), ErroMotor> {
        for (nome, off, esperado) in PROLOGOS {
            if indice.contendo(off).is_none() {
                return Err(ErroMotor::Io(format!("{nome} (+{off:#x}) outside any function in the payload")));
            }
            let vivo = alvo
                .bytes(alvo.base + off, esperado.len())
                .map_err(|e| ErroMotor::Io(format!("{nome}: {e}")))?;
            if vivo != esperado {
                return Err(ErroMotor::Io(format!(
                    "{nome} (+{off:#x}) does not start as expected; payload differs from the measured one"
                )));
            }
        }
        if indice.contendo(SITE_VERIFICACAO).is_none() {
            return Err(ErroMotor::Io("verification site outside any function".into()));
        }
        Ok(())
    }

    fn identidade_de(pid: i32) -> Option<Identidade> {
        let alvo = Alvo::novo(pid).ok()?;
        Some(Identidade {
            pid,
            inicio: crate::process::inicio_do_processo(pid)?,
            base: alvo.base,
        })
    }

    /// Garante que o motor aponta para o cliente que está vivo agora. Troca de
    /// jogo costuma significar processo novo: tudo que dependia do processo
    /// antigo (base, hook, estados rejeitados) é descartado e as âncoras são
    /// revalidadas do zero. Falha alto com `SemCliente` se não há cliente.
    pub fn revalidar(&mut self, log: &mut Vec<String>) -> Result<(), ErroMotor> {
        self.revalidar_excluindo(log, &[])
    }

    /// `outros` são pids de clientes que já têm motor: o substituto nunca cai neles.
    pub fn revalidar_excluindo(&mut self, log: &mut Vec<String>, outros: &[i32]) -> Result<(), ErroMotor> {
        let atual = Self::identidade_de(self.pid);
        let substituto = match atual {
            Some(_) => None,
            None => {
                let excluidos = [outros, &[self.pid]].concat();
                let pid = crate::process::escolher_candidato(&crate::process::candidatos_payload(), &excluidos);
                pid.and_then(Self::identidade_de)
            }
        };
        match decidir(&self.identidade, atual.as_ref(), substituto.as_ref()) {
            Decisao::Manter => Ok(()),
            Decisao::ClienteSumiu => Err(ErroMotor::SemCliente),
            Decisao::Reconectar(motivo) => {
                let novo_pid = substituto.map(|i| i.pid).unwrap_or(self.pid);
                log.push(format!("client switched ({}): reconnecting to pid {novo_pid}", descrever(motivo)));
                *self = Motor::conectar_pid(novo_pid)?;
                Ok(())
            }
        }
    }
}

fn descrever(m: Motivo) -> &'static str {
    match m {
        Motivo::PidNovo => "old process died",
        Motivo::ProcessoReaproveitado => "pid reused",
        Motivo::PayloadRemapeado => "payload remapped",
    }
}
