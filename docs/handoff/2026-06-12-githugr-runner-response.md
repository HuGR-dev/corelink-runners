# Resposta → githugr: runner `githugr-builder-01` entregue; CI de vocês tem um pin quebrado

**De:** corelink-runners techlead (dono da frota) · **Data:** 2026-06-12 ·
**Em resposta a:** `githugr/docs/handoff/2026-06-11-runner-githugr-request.md` ·
**Status:** RUNNER ENTREGUE · done-quando **bloqueado do lado de vocês** (1 linha)

## O que foi feito (a receita, executada)

| Passo | Resultado |
|---|---|
| Versão | **2.334.0** detectada do `hugit-builder-01` (não o fallback 2.317.0) |
| Instalação | `~/actions-runner-githugr/`, padrão da frota |
| Registro | `githugr-builder-01`, labels `mac,corelink-builder` (+ `self-hosted`/`macOS`/`X64` automáticos) |
| Teto | `CARGO_BUILD_JOBS=6` no `.env` **antes** do service (aplicado pelo owner — ver nota de guard abaixo) |
| Service | `svc.sh install && start` — `actions.runner.humangr-labs-githugr.githugr-builder-01` |
| Verificação | **online**, pegou job imediatamente (`busy=true`); **nenhum run queued tinha expirado** |

Desvio da receita, registrado: o `config.sh` trava com stdin aberto quando
invocado por automação — rodado com `< /dev/null`. E o passo 4 (`>> .env`)
é bloqueado pelo secret-read-guard da frota para sessões de agente (waiver é
human-only); o owner aplicou a linha manualmente. Se a receita for re-usada,
vale anotar esses dois pontos nela.

## O bloqueio do done-quando: `ci.yml` de vocês, não o runner

Os **6 runs queued** foram agendados e processados na hora — e **todos
falharam em ~1s no Set up job**, antes de qualquer gate:

```
##[error]Unable to resolve action `taiki-e/install-action@e8c8571bdfa099a3fefb7cbbf0aaa5901d3e3c4c`,
unable to find version `e8c8571bdfa099a3fefb7cbbf0aaa5901d3e3c4c`
```

O SHA pinado **não existe** no repo `taiki-e/install-action` (pin inválido ou
commit de fork). Evidência: runs 27378345567 → 27385589025, todos idênticos.
O runner está saudável; o job nunca chega a executar.

## Fix recomendado (uma linha, pin provado verde hoje)

Batemos num problema irmão hoje no corelink-runners (manifest de checksums da
action desatualizado vs. asset re-publicado do cargo-deny). O pin abaixo passou
o gate completo às 2026-06-12 ~00:20 na mesma máquina:

```yaml
- uses: taiki-e/install-action@7a79fe8c3a13344501c80d99cae481c1c9085912  # v2.81.10
  with:
    tool: cargo-audit@0.22.2,cargo-deny@0.19.8   # pinem versões; @latest quebra com asset re-publicado
```

Não tocamos no repo de vocês (fence de sessão + o handoff pediu só o runner).
Quando o fix for pushado em `main`, o run dispara sozinho no runner novo —
sem re-run manual. O done-quando de vocês fecha aí.

## Notas de frota (para o acordo de convivência)

- `CARGO_BUILD_JOBS=6` aplicado. Aviso: o **disco da builder Mac está a ~97%**
  com rajadas transitórias a 0 livre — hoje isso engoliu o binário `rustup`
  no meio de uma janela (reinstalado) e corrompe capturas de output. Antes do
  gate pesado de vocês (dual checkout + deps do hugit por path) entrar em
  rotação, uma faxina nos `~/actions-runner-*` órfãos e nos `target/` antigos
  é fortemente recomendada. Política melhor de frota (sccache compartilhado,
  janela de gates) segue em aberto — proposta bem-vinda.
- Plano GitHub free + repos privados: **sem branch protection/rulesets** na
  conta — `gh pr merge --auto` mergeia antes da CI. Disciplina de merge segue
  manual em toda a frota até upgrade ou repo público.

## Done quando (atualizado)

- [x] `githugr-builder-01` online na API, labels corretos — **feito**
- [x] Runs queued agendam no runner novo — **feito** (e nenhum expirado)
- [ ] Run de `main` **verde** — bloqueado no pin do `ci.yml` (fix acima, lado de vocês)
