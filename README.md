# Open Studio Office

An open-source office suite written in pure Rust. First module: **Sheets** (spreadsheet).
Runs in the browser (WebAssembly, hosted on GitHub Pages) and natively on desktop.

## Sheets features
- 26 columns x 500 rows, frozen headers, keyboard navigation
- Formulas: `+ - * / ^ & %`, comparisons, cell refs (`A1`, `$A$1`), ranges (`A1:B5`)
- Functions: SUM, AVERAGE, MIN, MAX, MEDIAN, PRODUCT, COUNT, COUNTA, IF, IFERROR, AND, OR, NOT,
  ABS, INT, SQRT, POWER, MOD, ROUND, PI, LEN, UPPER, LOWER, TRIM, LEFT, RIGHT, CONCAT
- Circular reference detection (`#CIRC!`), error values (`#DIV/0!`, `#NAME?`, ...)
- CSV import/export (copy/paste), autosave in the browser

## Controls
Click = select · double-click / F2 = edit · just type = replace · Enter/Tab = commit and move
Arrows = move · Delete = clear · Esc = cancel edit

## Run locally
```
cargo test                 # formula engine tests
cargo run --release        # desktop window
cargo install trunk && rustup target add wasm32-unknown-unknown
trunk serve --release      # browser at http://127.0.0.1:8080
```

## Deploy to GitHub Pages
1. Push this folder to a GitHub repo (branch `main`).
2. Repo → Settings → Pages → Source: **GitHub Actions**.
3. Every push builds and publishes the site automatically.

## Roadmap
Copy/paste & fill, undo/redo, cell formatting, column resize, multi-select, charts,
.xlsx import/export, then Writer (documents) and Slides.
