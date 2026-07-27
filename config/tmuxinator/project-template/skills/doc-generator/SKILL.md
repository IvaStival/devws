---
name: doc-generator
description: >
  Gera e atualiza documentação arquitetural do projeto orbit-back seguindo os padrões estabelecidos em docs/.
  Use esta skill ao final de execuções de plano que envolveram mudanças arquiteturais significativas:
  novos padrões, novos fluxos de dados, novos services/repositories/DTOs, novas integrações, ou mudanças
  em camadas existentes. Também use quando o usuário pedir para documentar algo, mencionar "documentar",
  "criar doc", "atualizar documentação", ou quando completar uma feature que introduziu conceitos novos
  na codebase. NÃO use para correções de bugs simples, ajustes de estilo, ou mudanças que não alteram
  a arquitetura.
---

# Doc Generator — Documentação Arquitetural orbit-back

Esta skill gera e atualiza documentação técnica na pasta `docs/` seguindo os padrões do projeto orbit-back (Laravel/PHP).

## Quando usar

Após executar um plano que envolveu:

- Novo padrão arquitetural (ex: novo factory pattern, novo tipo de provider)
- Novo fluxo de dados (ex: nova integração com RabbitMQ, novo consumer)
- Nova feature com múltiplas camadas (DTO + Repository + Service + Controller)
- Mudança significativa em camadas existentes (ex: refatoração do sistema de auth JWT)
- Novos conceitos que futuros desenvolvedores precisam entender

NÃO documentar: bug fixes simples, ajustes de formatação (Pint), rename de variáveis, mudanças pontuais sem impacto arquitetural.

## Processo

### 1. Avaliar o que mudou

Antes de escrever, responda internamente:

- **O que foi criado/modificado?** Liste os arquivos e camadas envolvidas.
- **Existe doc que cobre isso?** Leia os arquivos em `docs/` e o `AGENTS.md` ou `CLAUDE.md` para verificar.
- **Atualizar ou criar?** Se o tema se encaixa numa doc existente, atualize-a. Se é um sistema novo com múltiplas camadas próprias, crie uma doc separada.

### 2. Seguir o padrão de escrita

A documentação do orbit-back segue um formato consistente. Siga estas regras:

**Idioma:** Português (pt-BR) para todo o conteúdo, incluindo títulos, descrições e comentários em código.

**Título:** Sempre `# Arquitetura: {Tópico}`

**Estrutura das seções** (adaptar conforme necessidade, nem todas são obrigatórias):

```text
# Arquitetura: {Tópico}
## Visão Geral              — Diagrama ASCII + parágrafo explicativo
## Por que esta abordagem?  — Justificativa com tabela comparativa (quando relevante)
## Estrutura de Diretórios   — Árvore dos arquivos envolvidos
## Camadas do Sistema        — Detalhe de cada camada com código real
## Fluxo de Dados            — Diagrama de sequência ASCII
## Guias Práticos            — "Como adicionar X", checklists
## Referências               — Links para docs relacionados e arquivos-fonte
```

**Regras de formatação:**

- Diagramas ASCII em blocos ` ```text ` (nunca ` ``` ` sem linguagem)
- Código PHP em blocos ` ```php `
- Tabelas com espaço nos separadores: `| --- |` e não `|---|`
- Listas precedidas por linha em branco
- Listas ordenadas sempre com prefixo `1.` (markdown renderiza a numeração)
- Separar seções com `---`
- Não usar emojis no conteúdo (exceto em tabelas comparativas onde fica claro)

**Conteúdo:**

- Usar código **real** do projeto, não exemplos genéricos — leia os arquivos antes de documentar
- Incluir interfaces PHP relevantes (ex: `TicketRepositoryInterface`)
- Explicar o "porquê" das decisões, não apenas o "como"
- Diagramas ASCII para fluxos de dados (boxes com `┌─┐`, setas com `│ ▼ →`)
- Tabela de "Arquivos Referenciados" no final com path + responsabilidade
- Incluir seção prática de "Como adicionar/estender" quando aplicável

### 3. Atualizar índices

Após criar ou atualizar uma doc:

1. Verificar se `AGENTS.md` ou `CLAUDE.md` lista a doc na seção de arquitetura (se existir)
1. Se não, mencionar ao usuário que a doc foi criada em `docs/NOME.md`

### 4. Validar

Antes de finalizar:

1. Todos os paths de arquivo mencionados na doc existem no projeto
1. Blocos de código têm linguagem especificada (`php`, `bash`, `text`)
1. Tabelas têm formatação consistente com espaços
1. Listas têm linha em branco antes
1. A doc é navegável: seções têm headers claros, fluxo lógico de cima para baixo

## Referência de estrutura do projeto

Camadas padrão do orbit-back (para referenciar nas docs):

```text
Route (routes/api.php)
  → Middleware (Http/Middleware/)
  → Controller (Http/Controllers/{Domain}/)
  → Service (Services/{Domain}/)
  → Repository (Repositories/{Domain}/)
  → Model (Models/)
  → DB
```

Patterns comuns a documentar:
- **Factory pattern**: `TicketProviderFactory::make($module)` → Provider específico
- **Interface + binding**: Interface em `Interfaces/`, binding em `AppServiceProvider`
- **DTO**: Classe em `DTO/{Domain}/` com `fromRequest()` estático
- **Enum**: PHP 8.1+ enum em `Enums/{Domain}/`
- **Consumer RabbitMQ**: Artisan command em `Console/Commands/`

## Decisão: atualizar vs criar

Use este guia para decidir:

**Atualizar doc existente quando:**
- A mudança adiciona um novo exemplo/padrão a uma camada já documentada
- O escopo da mudança se encaixa no título da doc existente
- Adicionar uma seção nova na doc existente faz sentido no fluxo de leitura

**Criar doc nova quando:**
- O sistema tem múltiplas camadas próprias (DTO + Interface + Repository + Service + Controller)
- O tema é autossuficiente e merece navegação própria
- A doc existente ficaria muito longa (>500 linhas) com a adição
