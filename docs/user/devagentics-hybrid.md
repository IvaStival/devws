# Arquitetura: DevAgentics híbrido e economia de tokens

## Visão geral

O DevWS inicia o workspace e os agentes no tmux. O DevAgentics coordena
o plano aprovado, as ordens, a execução isolada e a integração. O objetivo
é reduzir **o custo por tarefa entregue**, incluindo tentativas e revisões,
com os mesmos critérios de qualidade e segurança.

```text
Líder (Claude, Codex ou OpenCode) ── CLI de leitura ─┬── DevAgentics ── estado com lock
                                   └── MCP ──────────┘        │
                                       │ ordens e evidências
Worker CLI de provedor ── Worker MCP ──┴── Docker e Git
                                       │
                            Dispatcher ── aviso no tmux
```

O MCP usa JSON-RPC sobre stdio. CLI e MCP compartilham os leitores de
progresso, ordens e inbox. A operação fica restrita à tarefa e os agentes
trocam mensagens pela fila do coordenador. Para acordar o Líder, o dispatcher
manda um aviso no tmux. O Líder consulta a fila e confirma os IDs tratados.

---

## Escolha da interface

| Situação | Interface |
| --- | --- |
| Progresso e ordem específica do Líder com a CLI ativa | CLI de leitura |
| Inbox sem confirmação pendente | CLI de leitura |
| Confirmar IDs e buscar o próximo lote | MCP `application_workflow`, ação `leader_inbox` com `input.ack` |
| Apenas confirmar IDs | MCP `application_workflow`, ação `inbox_ack` com `input.ids` |
| Estado completo, planejamento, ordens e revisão | MCP do Líder |
| Execução, checkpoint, testes e relatório | MCP do Worker |

O catálogo do Líder no piloto anuncia `task_status` para estado completo.
Os clientes MCP existentes continuam compatíveis com as visualizações
anteriores. Workers usam o mesmo MCP em todos os modos do piloto.

## Como usar

Em uma tarefa revisada, com o Líder em Claude, Codex ou OpenCode (OpenCode
só em tarefas revisadas), habilite por tarefa e reabra o Líder para
carregar as instruções de inicialização.
Execute as leituras a partir da raiz do worktree principal.

```bash
devagentics task leader-cli <task> on
devagentics leader-read <task> progress
devagentics leader-read <task> order <order-id>
devagentics leader-read <task> inbox
```

As leituras retornam JSON e usam o lock compartilhado. Cada programa
restringe o shell do Líder a esse único comando:

- **Codex:** o sandbox de leitura do Codex.
- **Claude Code:** só a regra `Bash(<devagentics> leader-read <task>:*)`,
  em `dontAsk`, que recusa qualquer outro comando sem perguntar.
- **OpenCode:** configuração isolada, com regras de bash que permitem o
  comando e recusam encadeamentos, pipes, substituições e redirecionamentos.

Uma versão do programa que não foi verificada desse jeito inicia o Líder
só com MCP, e o log da tarefa diz o motivo. Para Claude, valem só as versões
em que `scripts/probe_claude_leader.sh` passou (2.1.331). Para OpenCode,
vale a 1.19.6, verificada com `scripts/probe_opencode_leader.py`. O inbox contém mensagens do Líder, equipe e trabalho
pronto. Uma leitura deixa os IDs pendentes. O fallback de leitura é MCP.
`devagentics task leader-cli <task> off` desativa a CLI imediatamente.
Reabra o Líder para recarregar o catálogo e as instruções padrão.

---

## Respostas compactas

- **Worker status.** Use `input.view: "brief"` para a ordem atual e
  referências. Use `full` para contratos e histórico.
- **Execução.** Use `input.view: "compact"` para comandos ruidosos no
  container. O retorno inclui código de saída, prévia, diagnósticos
  reconhecidos e ID quando há captura. Use `result_read` com ID, stream
  e offset para ler o restante retido. Cada stream retém até 2 MiB e
  sinaliza truncamento. O resultado pertence à geração da identidade do
  Worker. Consulte a captura antes de repetir um comando.
- **Plano.** Use `view: "section"` com o título exato quando ele for
  conhecido. A seção inclui os subtítulos até o próximo título de nível
  igual ou superior. Continue com `next_line` como `start_line` e o
  mesmo título. A página é limitada a 120 linhas e 8 KiB. Para títulos
  ausentes ou repetidos, use `outline` e `excerpt`. Quando precisar de
  todo o contexto, use `full`, que também é o padrão sem visualização.

Exemplo de argumentos do tool `worker`.

```json
{"action":"plan_read","input":{"step":"baseline-plan","view":"section","heading":"Validation and release milestone"}}
```

## Execução, estado e recuperação

1. O Líder registra a confirmação e a aprovação da revisão exata do plano.
1. O coordenador registra a ordem. O supervisor lança o processo do
   provedor e retoma sua conversa quando disponível.
1. O Worker usa execução em snapshots Docker. Checkpoint valida os
   arquivos permitidos e protegidos e registra as mudanças no Git.
1. Checks e relatórios registram evidências da ordem. Candidatos passam por
   revisão independente e validação antes da integração. A validação da
   milestone confirma a entrega integrada.
1. O coordenador atualiza o estado e a fila. O dispatcher acorda o Líder
   quando há novas mensagens.

O estado e as operações usam locks de arquivo. Um turno interrompido pode
deixar uma ordem em estado incerto. O operador verifica a ordem, a evidência
e os efeitos já registrados antes de confirmar um reenvio por
`recover_turn`. Claude Code retoma por sessão, Codex por ID de thread e
OpenCode por ID de sessão. O piloto mantém esse ciclo e a memória
compartilhada via ai-memory.

---

## Resultados e limites

| Experimento | Resultado observado |
| --- | --- |
| Notes com MCP de respostas completas e compactas | US$ 3,41 e US$ 1,78 por entrega, redução observada de 47,9% |
| Notes com MCP compacto e híbrido | US$ 4,13 e US$ 3,20 por entrega, redução observada de 22,6%. As 15 leituras CLI funcionaram |
| Inbox combinado e confirmação separada da leitura CLI | Uma chamada e 34.212 tokens de entrada contra duas chamadas e 69.931. O cache variou |
| Leitura completa e seção nomeada em uma chamada | 75.539 e 65.846 tokens de entrada, redução de 12,8% com o mesmo cache de entrada |
| Replay com seções disponíveis | 13 testes passaram e a milestone ficou pronta. US$ 2,87 contra US$ 3,20 na entrega híbrida anterior. Foram 12 leituras completas e zero seções |

Os valores em dólares são equivalentes estimados a preços de API. Nas
entregas variaram as revisões, correções, testes escritos pelos agentes e
cache. O replay validou o fluxo de entrega, mas não demonstrou adoção de
seções nem economia atribuível a elas. O pré-teste Publisher validou o
ambiente de frontend. A comparação de entrega foi encerrada na fase de
planejamento.

Para medir uma próxima mudança, fixe a árvore inicial, o provedor, o
modelo, a imagem, os checks e a aceitação. Registre chamadas, bytes,
tokens do provedor, cache, tentativas, revisão e recuperação. Compare o
custo de todas as tentativas com o número de entregas concluídas. Ative a
telemetria apenas para as tarefas do experimento. Os bytes do protocolo
medem tráfego local, enquanto os tokens vêm do provedor.

## Escopo atual

O híbrido permanece a opção de trabalho estudada. A CLI de leitura é
opt-in por tarefa, em qualquer programa de Líder verificado. As visualizações compactas coexistem com as
completas. Seções nomeadas ficam disponíveis para títulos conhecidos.
Migração de escrita para a CLI, descoberta mais restrita de ferramentas,
cursores de inbox e mudanças na rotação de sessão são experimentos
futuros. Uma ampliação precisa preservar autorização, isolamento,
recuperação e evidência de integração.

## Referências

| Arquivo | Responsabilidade |
| --- | --- |
| [main.rs](../../agentic/src/main.rs) e [mcp.rs](../../agentic/src/mcp.rs) | CLI e MCP do Líder |
| [status.rs](../../agentic/src/status.rs) e [workflow.rs](../../agentic/src/workflow.rs) | Leitores compartilhados, inbox e confirmação |
| [task.rs](../../agentic/src/task.rs) e [watch.rs](../../agentic/src/watch.rs) | Locks, estado e dispatcher |
| [worker_tools.rs](../../agentic/src/worker_tools.rs) e [plans.rs](../../agentic/src/plans.rs) | Supervisor, MCP do Worker e visualizações de plano |
| [runner.rs](../../agentic/src/runner.rs) e [integration.rs](../../agentic/src/integration.rs) | Execução isolada, checkpoints e integração |
| [Guia de benchmarks](devagentics-bench.md) | Telemetria, contratos e protocolo de medição |
| [Plano e evidências](../../specs/plan/2026-11-21-214851_token-optimization/plan.md) | Auditoria, experimentos e resultados detalhados |
