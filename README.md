# buoya-news-mcp

A free, single-binary [MCP](https://modelcontextprotocol.io) server that aggregates crypto and AI news from many free sources, deduplicates and scores it by importance, and exposes it as tools for Claude (or any MCP client).

Ask `"what happened in crypto today?"` or `"any exploits this week?"` and get a ranked, deduplicated answer in seconds — a 5-minute daily briefing instead of 45 minutes of feed-scrolling.

> **Status:** v0.1 — in development. See [dev-docs/](dev-docs/) for the PRD and technical specification.

---

## Why

High-signal information about crypto and AI is scattered across crypto news sites, security feeds, AI research announcements, Reddit, Hacker News, and company blogs. Checking them all daily is slow and inconsistent, and important events (a major exploit, a coin crash, a significant model release) can be missed for hours.

`buoya-news-mcp` is a single, queryable, prioritized feed of *what actually matters*.

**Design constraints:**

- **Zero recurring cost** — only free sources (RSS, public/free-tier APIs). No paid API keys required.
- **Information, not advice** — no trading signals or financial advice.
- **No web UI** — your MCP client (Claude Desktop, Claude Code, etc.) is the interface.
- **Simple deployment** — one compiled binary, no Node runtime, near-zero idle memory.

## Features

- **Severity-tiered ranking** — `critical` (exploit, crash, major outage) separated from `notable` and `info`, so a quiet day returns a near-empty alert feed.
- **Cross-source deduplication** — the same story across five sites collapses into one item; the duplicate count itself becomes an importance signal.
- **Deterministic, explainable scoring** — a pure heuristic (no LLM, no cost) weighting cross-source coverage, community signals, severity keywords, quantified impact, and recency decay. Each item carries its score breakdown.
- **Graceful degradation** — every source is optional and config-toggled; one source being down or rate-limited never blocks the others. Per-source circuit breaker and `get_source_status` visibility.
- **Watchlist-aware** — items mentioning your watchlist coins/protocols get a score multiplier.
- **Read-state tracking** — `get_briefing` can default to "since you last asked".

## MCP Tools

| Tool | Purpose |
|---|---|
| `get_briefing(period?, topics?, limit?)` | The flagship "what did I miss?" digest — ranked, deduplicated, severity-tagged. |
| `get_alerts(severity?, since?)` | Only high-severity events. Near-empty on quiet days. |
| `search_news(query, since?, limit?)` | Full-text search over the cached corpus (SQLite FTS5). |
| `get_exploits(since?, min_amount_usd?)` | Structured exploit data (protocol, loss amount, technique) from DeFiLlama + rekt. |
| `get_market_movers(threshold_pct?)` | Coins that moved beyond a threshold in 24h, watchlist always included. |
| `get_ai_releases(since?, limit?)` | New models, papers, and tools, ranked by signal. |
| `get_source_status()` | Health of each upstream source — distinguishes "quiet" from "broken". |

## Data Sources (all free)

| Category | Sources |
|---|---|
| Crypto news | CoinDesk, Cointelegraph, The Block, Decrypt (RSS), CryptoPanic (free API) |
| Exploits / hacks | rekt.news (RSS), DeFiLlama Hacks (public API) |
| Market | CoinGecko (prices, 24h change), alternative.me Fear & Greed Index |
| AI news & research | Hacker News (Algolia API), arXiv (cs.AI/cs.LG/cs.CL), Hugging Face |
| AI releases | Company blogs (OpenAI, Anthropic, Google DeepMind, Meta AI, Mistral) |
| Community signal | Reddit (r/CryptoCurrency, r/MachineLearning, r/LocalLLaMA, r/ethereum) |

> Free tiers and rate limits change over time. Every source is treated as optional and re-verified at implementation time.

## Architecture

- **Language:** Rust (stable 1.96+, edition 2024) with the official [`rmcp`](https://github.com/modelcontextprotocol/rust-sdk) SDK.
- **Process model:** single local process. The MCP server runs over stdio; a fetch scheduler runs in-process as tokio background tasks (default 12h interval per source group, configurable). Lazy staleness refresh runs an ingest before answering when data is stale.
- **Storage:** a single SQLite file with FTS5 for full-text search. 90-day retention. Zero infrastructure.
- **Pipeline:** fetchers parse bytes into a normalized `NewsItem` schema → deduplicate (Jaccard title similarity + URL canonicalization within a 48h window) → score → store. Fetchers never touch the DB; tools never touch the network.

See [dev-docs/buoya-news-mcp-technical-spec.md](dev-docs/buoya-news-mcp-technical-spec.md) for the full design.

## Build

Requires the Rust toolchain (1.96+). Install via [rustup](https://rustup.rs/).

```sh
git clone <repo-url>
cd buoya-news-mcp
cargo build --release
```

The binary is produced at `target/release/buoya-news-mcp`.

## Configuration

Configuration lives in `config.toml` (overriding committed defaults in `config.default.toml`). Every field has a default, so a missing file still runs. Configurable without code changes: enabled sources, fetch and staleness intervals, watchlist coins, score weights, severity thresholds, keyword lists, and HTTP timeout/User-Agent.

See [§9 of the technical spec](dev-docs/buoya-news-mcp-technical-spec.md) for the full config reference.

## Use with Claude Desktop

Add the server to your Claude Desktop MCP config (`claude_desktop_config.json`), pointing at the compiled binary:

```json
{
  "mcpServers": {
    "buoya-news": {
      "command": "/absolute/path/to/target/release/buoya-news-mcp"
    }
  }
}
```

Restart Claude Desktop, then ask: *"Give me a briefing on what happened in crypto and AI today."*

## Testing

```sh
cargo test                                   # pipeline, fetcher-parse, and repo tests (no network)
cargo clippy --all-targets -- -D warnings
cargo fmt --check
```

Fetcher parsing and pipeline logic are pure functions tested against committed fixtures — `cargo test` makes no network calls.

## License

TBD.
