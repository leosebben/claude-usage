# claude-usage

TUI (Rust + ratatui) que mostra o uso de tokens e o custo estimado do Claude
Code CLI. É a aba "Uso" do [Claude Desk](../claude-memory-editor) refeita para
o terminal, sem a parte de edição de memória.

```
 claude-usage    1 Hoje  2 7 dias  3 30 dias  4 Tudo   │   Custo  Tokens            6259 respostas
╭ Custo ────────── 2388 respostas ╮╭ Hoje ─────────── 62 respostas ╮╭ Tokens ──── 1,79 M saída ╮╭ Sessões ─── 44 projetos ╮
│ US$ 229,69  7 dias              ││ US$ 8,89  5,21 M tokens       ││ 228,53 M  219,91 M lidos ││ 97  1 modelo             │
╰─────────────────────────────────╯╰───────────────────────────────╯╰──────────────────────────╯╰──────────────────────────╯
╭ Por dia · máx US$ 77,10 ───────────────────────────────────────────────────────────────────────────── ● fable 5.1 ╮
│                                        ███████                                                                    │
│███████ ███████ ▂▂▂▂▂▂▂ ███████ ███████ ███████                                                                    │
│██55███ ██38███ ██11███ ██22███ ██18███ ██77███ ██8,9██                                                            │
│ 03/09   04/09   05/09   06/09   07/09   08/09   09/09                                                             │
╰───────────────────────────────────────────────────────────────────────────────────────────────────────────────────╯
╭ Por modelo ──────────────────── ⏎ filtra modelo ╮╭ Por projeto ─────────────────── ⏎ filtra projeto ╮
│› ● fable 5.1 █████████████████████ US$ 229,69   ││  pretatemplate-univers… ███████████ US$ 81,46    │
```

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

Pelo Homebrew:

```sh
brew tap leosebben/tap
brew install claude-usage
```

Ou direto do fonte, no `~/.cargo/bin`:

```sh
cargo install --path .
```

Para rodar sem instalar: `cargo run --release`.

## Publicando uma versão

1. Subir a versão em `Cargo.toml`, commitar e criar a tag `vX.Y.Z`.
2. `gh release create vX.Y.Z --generate-notes`.
3. Atualizar `url` e `sha256` em `homebrew/claude-usage.rb` e copiar o arquivo
   para `Formula/claude-usage.rb` no repositório `homebrew-tap`.

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
