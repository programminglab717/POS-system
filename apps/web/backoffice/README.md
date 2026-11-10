# Keel back office

A location's catalog editor and reports, on the cloud cell's API
([ADR-0028](../../../docs/adr/0028-cloud-cell-v0.md)): React 19 and TypeScript, built with Vite,
served by the cell (`keel-cell --web dist`).

```bash
npm ci
npm run build
```

To try it by hand, run `keel-cloud`'s demo, a location end to end on the host (the cell, its hub
and a register, the register's sales reaching the cell through the hub), and sign in with the
token it writes to `<dir>/token`:

```bash
cargo run -p keel-cloud --example demo -- /tmp/keel-demo 127.0.0.1:8787 apps/web/backoffice/dist
```

In development, `npm run dev` proxies `/api` to the cell at `KEEL_CELL` (`http://127.0.0.1:8080`
unless set).

The end-to-end tests start the demo themselves, from `target/debug/examples/demo` unless
`KEEL_DEMO` names another, and run in Chromium:

```bash
cargo build -p keel-cloud --example demo
npm run build
npx playwright test
```
