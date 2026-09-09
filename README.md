# claude-usage

TUI (Rust + ratatui) que mostra o uso de tokens e o custo estimado do Claude
Code CLI. É a aba "Uso" do [Claude Desk](../claude-memory-editor) refeita para
o terminal, sem a parte de edição de memória.

## De onde vêm os dados

Lê os transcritos `~/.claude/projects/<slug>/<sessão>.jsonl` (ou
`$CLAUDE_HOME/projects`). Cada resposta do assistente traz `message.usage`
(entrada, saída, escrita e leitura de cache); as linhas repetidas da mesma
mensagem (uma por bloco de conteúdo) são deduplicadas pelo `message.id`. O
projeto vem do `cwd` gravado na sessão.

O custo é estimado com a tabela de preços da API em `src/usage.rs`
(cache write = 1,25× entrada, cache read = 0,1× entrada, exceto o Fable 5.1).
Modelos fora da tabela aparecem sem custo. Em planos por assinatura o valor é
só uma referência.

Os transcritos são relidos a cada 30 s (ou com `r`), com cache por arquivo:
só arquivos novos ou modificados são interpretados de novo.

## Instalando

Pelo Homebrew (binário pré-compilado, não precisa de Rust):

```sh
brew tap leosebben/tap
brew trust leosebben/tap   # Homebrew 6 exige confiar em taps de terceiros
brew install claude-usage
```

Ou baixando o binário da [página de releases](https://github.com/leosebben/claude-usage/releases)
para macOS (ARM e Intel) ou Linux (x86_64 e ARM).

Ou compilando do fonte, no `~/.cargo/bin`:

```sh
cargo install --path .
```

Para rodar sem instalar: `cargo run --release`.

## Publicando uma versão

1. Subir a versão em `Cargo.toml`, commitar e criar a tag `vX.Y.Z`.
2. Dar push da tag. O workflow `release.yml` compila para os quatro targets,
   cria a release com os tarballs, os checksums e a fórmula `claude-usage.rb`
   gerada por `scripts/formula.sh`.
3. `scripts/update-tap.sh vX.Y.Z` copia a fórmula para o tap e faz o push.

## Teclas

| Tecla              | Ação                                        |
| ------------------ | ------------------------------------------- |
| `1` `2` `3` `4`    | período: hoje, 7 dias, 30 dias, tudo        |
| `t` / `m`          | alterna entre custo e tokens                |
| `Tab` / `←` `→` / `h` `l` | painel anterior / seguinte; no gráfico, `←` `→` andam entre os dias |
| `↑` `↓` / `j` `k`  | move a seleção                              |
| `Enter` / espaço   | liga ou desliga o filtro do dia/modelo/projeto |
| `Esc` / `x`        | limpa os filtros                            |
| `r`                | recarrega os transcritos                    |
| `?`                | ajuda                                       |
| `q`                | sai                                         |

## Testes

```sh
cargo test                                      # regras de negócio e formatação
cargo test real_home -- --ignored --nocapture   # renderiza a tela com os dados reais de ~/.claude
```

## Estrutura

- `src/usage.rs` — leitura dos transcritos, deduplicação, cache e tabela de preços.
- `src/data.rs` — filtros, agregações (resumo, por modelo/projeto/sessão, por dia) e formatação pt-BR.
- `src/app.rs` — estado da TUI, teclas e carregamento em segundo plano.
- `src/ui.rs` — desenho da tela com ratatui.
