//! Sonda de estado: descobre se o `lua_State` capturado é de um jogo ou do
//! LuaApp (menu). A sonda só lê uma propriedade (`game.PlaceId`) e nunca chama
//! função do jogo: `GetService` reentra em `luaD_call`, o stub devolve pelo
//! catcher e o retorno vai parar no endereço de outra thread.

use crate::engine::{
    compilar, ErroMotor, Motor, FN_CARREGAR, OFF_NOME, OFF_STACK, OFF_TT, TAM_PROBE, TT_FUNCAO,
};
use crate::hook::Hook;
use crate::remote::Sessao;

/// Número em TValue do Luau.
const TT_NUMERO: u32 = 3;
const OFF_SONDA: u64 = crate::engine::OFF_PROBE;

/// `coroutine.resume` é a fronteira de erro: se o `game` do estado recusar a
/// leitura (sandbox do app), o erro volta como `false` em vez de subir pela
/// pilha falsa e derrubar o processo.
const FONTE_SONDA: &str = "local ok, v = coroutine.resume(coroutine.create(function() \
    return game.PlaceId end))\nreturn ok and v or 0";

/// Interpreta o resultado da sonda: PlaceId inteiro e positivo, senão `None`.
pub fn place_do_resultado(tt: u32, bits: u64) -> Option<u64> {
    if tt != TT_NUMERO {
        return None;
    }
    let v = f64::from_bits(bits);
    (v.is_finite() && v >= 1.0 && v.fract() == 0.0 && v < 1e15).then_some(v as u64)
}

impl Motor {
    /// `Some(placeId)` se o estado é de um jogo; `None` se for do LuaApp.
    pub(crate) fn sonda_jogo(
        &self,
        sessao: &Sessao,
        l: u64,
        top_antes: u64,
        freio: &Hook,
        trap: u64,
        log: &mut Vec<String>,
    ) -> Result<Option<u64>, ErroMotor> {
        let stack_antes = self.alvo.u64_em(l + OFF_STACK).unwrap_or(0);
        let cru = compilar(FONTE_SONDA).map_err(|e| ErroMotor::Io(format!("sonda: {e}")))?;
        let mapa = self.mapa_opcodes()?;
        let b = crate::opcodes::remapear(&cru, &mapa)
            .map_err(|e| ErroMotor::Io(format!("sonda remap: {e}")))?;
        if b.len() > TAM_PROBE {
            return Err(ErroMotor::Io(format!("probe of {} bytes does not fit in the page", b.len())));
        }
        let src = freio.stub + OFF_SONDA;
        self.alvo.escrever(src, &b).map_err(|e| ErroMotor::Io(e.to_string()))?;
        let nome = freio.stub + OFF_NOME;
        let status = sessao
            .chamar(&self.alvo, self.alvo.base + FN_CARREGAR, [l, nome, src, b.len() as u64, 0, 0], trap, 0)
            .map_err(|e| ErroMotor::Io(e.to_string()))?;
        let func = top_antes;
        let carregou = status == 0 && self.alvo.u32_em(func + OFF_TT).ok() == Some(TT_FUNCAO);
        if !carregou {
            self.restaurar_topo(l, top_antes, stack_antes, log);
            return Ok(None);
        }
        // sem o patch o undump deixa o código permutado e o despacho cai em rip=0
        let patch = self.patchar_codigo(func, &b, None, log);
        if let Err(e) = patch {
            log.push(format!("probe: patch failed: {e}"));
            self.restaurar_topo(l, top_antes, stack_antes, log);
            return Ok(None);
        }
        if let Err(e) = self.executar_estrategia(sessao, 1, 1, l, func, trap, log) {
            log.push(format!("probe: execution failed: {e}"));
            self.registrar_parada(sessao.tid, log);
            self.restaurar_topo(l, top_antes, stack_antes, log);
            return Ok(None);
        }
        let tt = self.alvo.u32_em(func + OFF_TT).unwrap_or(0xff);
        let bits = self.alvo.u64_em(func).unwrap_or(0);
        self.restaurar_topo(l, top_antes, stack_antes, log);
        let place = place_do_resultado(tt, bits);
        log.push(match place {
            Some(p) => format!("sonda: jogo, PlaceId {p}"),
            None => format!("sonda: LuaApp (tt={tt})"),
        });
        if std::env::var("LUARIA_VERBOSE").is_ok() {
            eprintln!("[sonda] L={l:#x} tt={tt} place={place:?}");
        }
        Ok(place)
    }
}

#[cfg(test)]
mod testes {
    use super::*;

    #[test]
    fn place_valido() {
        assert_eq!(place_do_resultado(3, 189707f64.to_bits()), Some(189707));
    }

    #[test]
    fn zero_e_app() {
        assert_eq!(place_do_resultado(3, 0f64.to_bits()), None);
    }

    #[test]
    fn tipo_errado_e_app() {
        assert_eq!(place_do_resultado(1, 1f64.to_bits()), None);
        assert_eq!(place_do_resultado(8, 189707f64.to_bits()), None);
    }

    #[test]
    fn lixo_nao_vira_place() {
        assert_eq!(place_do_resultado(3, f64::NAN.to_bits()), None);
        assert_eq!(place_do_resultado(3, 1.5f64.to_bits()), None);
        assert_eq!(place_do_resultado(3, (-4f64).to_bits()), None);
    }
}
