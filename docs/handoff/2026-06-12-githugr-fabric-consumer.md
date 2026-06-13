# Status + Opções — githugr: runner CI entregue; nova opção de fabric disponível

**De:** corelink-runners techlead · **Data:** 2026-06-12 ·
**Para:** githugr techlead · **Ref.:** `docs/handoff/2026-06-12-githugr-runner-response.md` ·
**Status:** RUNNER ENTREGUE (bloqueado neles, 1 linha) · FABRIC BUILD+PROVEN (deploy owner-gated)

---

## 1. Status do runner CI — sem mudança, resumo curto

Tudo isso está documentado em detalhe no handoff anterior
(`docs/handoff/2026-06-12-githugr-runner-response.md`); reproduzo só o
essencial para que o thread fique autocontido.

`githugr-builder-01` foi registrado, está online, e pegou os 6 runs queued
imediatamente. Todos falharam em ~1 s no **Set up job** — antes de qualquer
gate — com erro:

```
Unable to resolve action `taiki-e/install-action@e8c8571b…`,
unable to find version `e8c8571b…`
```

O SHA pinado não existe no repo `taiki-e/install-action`. O runner está
saudável; o job nunca chega a executar.

**Fix: uma linha no `ci.yml` de vocês** — substituir o SHA inválido pelo pin
abaixo, provado verde nesta mesma máquina em 2026-06-12:

```yaml
- uses: taiki-e/install-action@7a79fe8c3a13344501c80d99cae481c1c9085912  # v2.81.10
```

Quando pusharem o fix em `main`, o runner dispara automaticamente — sem re-run
manual. Não tocamos no repo de vocês (fence de sessão).

---

## 2. Novidade: githugr pode ser consumidor do fabric CoreLink Runners

Enquanto entregávamos o runner CI de vocês, o time concluiu e provou
end-to-end o **corelink-fabricd** — o binário de produção que executa compute
isolado e cache-warm via microVM em provider gerenciado (Northflank). O
ciclo completo foi validado ao vivo:

```
acquire lease → provision microVM → exec job real → exit 0
  → attestação assinada → teardown → RunnerLease liberada
```

Isso abre uma **segunda opção de integração** para o githugr, independente do
runner self-hosted: tornar-se um **consumidor do fabric** — da mesma forma que
o hugit já é.

### O que o fabric entrega (vs. o runner self-hosted)

| | Runner self-hosted (`githugr-builder-01`) | Fabric CoreLink Runners |
|---|---|---|
| Isolamento | Processo compartilhado na builder Mac | microVM por job (fail-closed) |
| Adequado para código não-confiável / agente | Não recomendado | Sim — design goal |
| Boot | Cold (deps do workspace no disco) | Cache-warm (CAS/AC pré-aquecido) |
| Re-runs com resultado já cacheado | Reexecuta tudo | Cache hit = custo ~0 |
| Pricing | Sem custo direto (infra HuGR) | Flat por concorrência, não por minuto |
| Attestação por job | Não | Sim (assinada, chain-of-custody) |
| Disponível agora | **Sim** (após o pin fix) | **Não ainda** (deploy owner-gated) |

A seam que o githugr implementaria é exatamente a mesma que o hugit usa:
`RunnerLease` + `FenceManifest` — contrato definido em
`docs/spec/hugit-integration-contract.md` v1.2.0. O produto e o racional de
pricing estão em `docs/product/product.md`.

### Honestidade sobre o estado atual

O fabric está **construído e provado ao vivo**, mas **não deployado em endpoint
público**. O deploy é owner-gated (`deploy/RUNBOOK.md`). A opção
"githugr como consumidor do fabric" está disponível para integrar assim que o
deploy acontecer — não hoje. O runner self-hosted está disponível **agora**
(com o pin fix).

---

## 3. A decisão para o githugr techlead

Duas opções não excludentes:

**A) Continuar no runner self-hosted** (simples, disponível agora)
Apply o pin fix → CI verde → pronto. Nenhuma mudança de arquitetura. Adequado
enquanto os jobs são confiáveis e o volume não justifica o overhead de integrar
o fabric. Risco a monitorar: disco da builder Mac está ~97% (detalhe no
handoff anterior) — limpeza recomendada antes de carga pesada.

**B) Migrar para o fabric** (memoizado, isolado, uma vez deployado)
Implementar a seam `RunnerLease` do lado do githugr (mesmo contrato do hugit —
reutilização direta). Resultado: jobs isolados em microVM, re-runs que já
foram cacheados não reexecutam, pricing flat. Depende do deploy do fabric —
sequenciaríamos juntos.

As duas podem coexistir durante uma transição.

---

## AÇÃO SOLICITADA

**(a)** Apliquem o fix de uma linha no `ci.yml` (`taiki-e/install-action@7a79fe8c…`
— detalhe acima e no handoff anterior) para desbloquear o runner atual.

**(b)** Nos digam se o githugr quer ser **consumidor do fabric** (opção B acima),
para que sequenciemos o deploy e a seam de integração de acordo. Sem pressão —
a opção A funciona e é completamente válida para o momento.

Roteie via owner.
