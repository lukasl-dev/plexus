# Plexus

Plexus asks a language server for source symbols and relationships, then builds a
deterministic program graph. The graph contains direct facts only:

- `contains` for lexical containment
- `calls` for outgoing calls
- `reads` and `writes` for fields, statics, and globals

Transitive mutation is derived from those facts rather than stored as another
edge type.

## Usage

The language server must be installed and available in `PATH`.

```console
$ plexus analyse \
    --root . \
    --lsp-server rust-analyzer \
    --language rust \
    --extension rs \
    --format compact
```

Build with Cargo or Nix:

```console
$ cargo build --release
$ nix build
```

Graph output is written to stdout and diagnostics to stderr. `--format` accepts:

- `summary`: node and relationship counts
- `json`: canonical `plexus/graph@1` JSON with deterministic IDs and full ranges
- `compact`: a lossy, low-token containment tree and relationship adjacency lists

Positions are zero-based UTF-16 positions with exclusive ends, matching LSP.

## Language-server requirements

Plexus requires hierarchical document symbols. It adds call and state-access
edges only when the server advertises the corresponding standard LSP
capabilities. Consequently, graph completeness depends on the language server;
unsupported relationships are omitted rather than guessed from source text.
