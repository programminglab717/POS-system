import { useEffect, useState } from "react";
import { type Api, ApiError, type Catalog, type CatalogEdit, type Group, type Item, type Page } from "./api";
import { format, newId, parse } from "./money";

const PREFIXES = ["plain", "no", "extra", "light", "side", "sub"];

type Status = { kind: "info" | "error" | "ok"; text: string } | null;

/** An amount typed as a decimal, kept as minor units once it is one. */
function PriceInput({
  value,
  digits,
  label,
  onChange,
}: {
  value: number;
  digits: number;
  label: string;
  onChange: (minor: number) => void;
}) {
  const [text, setText] = useState(format(value, digits));
  useEffect(() => setText(format(value, digits)), [value, digits]);
  const valid = parse(text, digits) !== null;
  return (
    <input
      className={valid ? "price" : "price invalid"}
      inputMode="decimal"
      aria-label={label}
      aria-invalid={!valid}
      value={text}
      onChange={(event) => {
        setText(event.target.value);
        const minor = parse(event.target.value, digits);
        if (minor !== null) onChange(minor);
      }}
    />
  );
}

/** A count from 0 to 255. */
function CountInput({ value, label, onChange }: { value: number; label: string; onChange: (n: number) => void }) {
  return (
    <input
      className="count"
      type="number"
      min={0}
      max={255}
      aria-label={label}
      value={value}
      onChange={(event) => {
        const n = Number(event.target.value);
        if (Number.isInteger(n) && n >= 0 && n <= 255) onChange(n);
      }}
    />
  );
}

/** The catalog of `location`: its items, modifier groups and menu, checked and published whole. */
export function CatalogEditor({ api, location }: { api: Api; location: string }) {
  const [catalog, setCatalog] = useState<Catalog | null>(null);
  const [items, setItems] = useState<Item[]>([]);
  const [groups, setGroups] = useState<Group[]>([]);
  const [pages, setPages] = useState<Page[]>([]);
  const [changed, setChanged] = useState(false);
  const [busy, setBusy] = useState(false);
  const [status, setStatus] = useState<Status>(null);

  const load = () =>
    api
      .catalog(location)
      .then((found) => {
        setCatalog(found);
        setItems(found.items);
        setGroups(found.groups);
        setPages(found.pages);
        setChanged(false);
      })
      .catch((failed: unknown) => setStatus({ kind: "error", text: message(failed) }));
  useEffect(() => {
    void load();
  }, [location]);

  if (!catalog) return <p className="empty">{status?.text ?? "Loading the catalog…"}</p>;
  const digits = catalog.minorUnits;
  const edit = (): CatalogEdit => ({ base: catalog.version, items, groups, pages });
  const touch = () => {
    setChanged(true);
    setStatus(null);
  };
  const updateItem = (index: number, change: (item: Item) => Item) => {
    setItems((all) => all.map((item, at) => (at === index ? change(item) : item)));
    touch();
  };
  const updateGroup = (index: number, change: (group: Group) => Group) => {
    setGroups((all) => all.map((group, at) => (at === index ? change(group) : group)));
    touch();
  };
  const updatePage = (index: number, change: (page: Page) => Page) => {
    setPages((all) => all.map((page, at) => (at === index ? change(page) : page)));
    touch();
  };
  const variants = items.flatMap((item) =>
    item.variants.map((variant) => ({
      id: variant.id,
      label: variant.name ? `${item.name}, ${variant.name}` : item.name,
    })),
  );
  const label = (id: string) => variants.find((variant) => variant.id === id)?.label ?? "(removed)";

  const check = async () => {
    setBusy(true);
    try {
      await api.check(location, edit());
      setStatus({ kind: "ok", text: "The catalog passes every check." });
    } catch (failed) {
      setStatus({ kind: "error", text: message(failed) });
    } finally {
      setBusy(false);
    }
  };
  const publish = async () => {
    setBusy(true);
    try {
      const { version } = await api.publish(location, edit());
      await load();
      setStatus({ kind: "ok", text: `Published version ${version.slice(0, 12)}. Registers take it within a minute.` });
    } catch (failed) {
      const conflict = failed instanceof ApiError && failed.status === 409;
      setStatus({
        kind: "error",
        text: conflict ? "Another catalog was published since this one was read: reload it first." : message(failed),
      });
    } finally {
      setBusy(false);
    }
  };

  return (
    <div className="editor">
      <div className="toolbar card">
        <div>
          <strong>Catalog</strong>{" "}
          <span className="muted" title={catalog.version}>
            version {catalog.version.slice(0, 12)} · {catalog.currency}
          </span>
          {changed && <span className="badge">Unpublished changes</span>}
        </div>
        <div className="actions">
          <button disabled={busy || !changed} onClick={() => void load().then(() => setStatus(null))}>
            Discard
          </button>
          <button disabled={busy} onClick={() => void check()}>
            Check
          </button>
          <button className="primary" disabled={busy || !changed} onClick={() => void publish()}>
            Publish
          </button>
        </div>
      </div>
      {status && (
        <p role="status" className={status.kind === "error" ? "error banner" : "ok banner"}>
          {status.text}
        </p>
      )}

      <section className="card">
        <div className="heading">
          <h2>Items</h2>
          <button
            onClick={() => {
              setItems((all) => [
                ...all,
                {
                  id: newId(),
                  name: "New item",
                  taxCategory: catalog.taxCategories[0]?.id ?? "",
                  variants: [{ id: newId(), name: null, price: 0 }],
                  groups: [],
                },
              ]);
              touch();
            }}
          >
            Add item
          </button>
        </div>
        {items.map((item, index) => (
          <article className="item" key={item.id} aria-label={`Item ${item.name}`}>
            <div className="row">
              <input
                aria-label="Item name"
                className="name"
                value={item.name}
                onChange={(event) => updateItem(index, (it) => ({ ...it, name: event.target.value }))}
              />
              <label className="inline">
                Tax
                <select
                  aria-label="Tax category"
                  value={item.taxCategory}
                  onChange={(event) => updateItem(index, (it) => ({ ...it, taxCategory: event.target.value }))}
                >
                  {catalog.taxCategories.map((category) => (
                    <option key={category.id} value={category.id}>
                      {category.taxes.length ? category.taxes.join(", ") : "Untaxed"}
                    </option>
                  ))}
                </select>
              </label>
              <button
                className="quiet"
                onClick={() => {
                  const gone = new Set(item.variants.map((variant) => variant.id));
                  setItems((all) => all.filter((_, at) => at !== index));
                  setPages((all) => all.map((page) => ({ ...page, buttons: page.buttons.filter((id) => !gone.has(id)) })));
                  touch();
                }}
              >
                Remove
              </button>
            </div>
            <table className="variants">
              <tbody>
                {item.variants.map((variant, at) => (
                  <tr key={variant.id}>
                    <td>
                      <input
                        aria-label="Variant name"
                        placeholder={item.variants.length > 1 ? "Size or kind" : "(the item itself)"}
                        value={variant.name ?? ""}
                        onChange={(event) =>
                          updateItem(index, (it) => ({
                            ...it,
                            variants: it.variants.map((v, n) => (n === at ? { ...v, name: event.target.value || null } : v)),
                          }))
                        }
                      />
                    </td>
                    <td>
                      <PriceInput
                        label={`Price of ${variant.name ? `${item.name}, ${variant.name}` : item.name}`}
                        value={variant.price}
                        digits={digits}
                        onChange={(price) =>
                          updateItem(index, (it) => ({
                            ...it,
                            variants: it.variants.map((v, n) => (n === at ? { ...v, price } : v)),
                          }))
                        }
                      />
                    </td>
                    <td>
                      {item.variants.length > 1 && (
                        <button
                          className="quiet"
                          aria-label="Remove variant"
                          onClick={() =>
                            updateItem(index, (it) => ({ ...it, variants: it.variants.filter((_, n) => n !== at) }))
                          }
                        >
                          ×
                        </button>
                      )}
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
            <div className="row">
              <button
                className="quiet"
                onClick={() =>
                  updateItem(index, (it) => ({ ...it, variants: [...it.variants, { id: newId(), name: "New", price: 0 }] }))
                }
              >
                Add variant
              </button>
              {groups.length > 0 && <span className="muted">Modifiers:</span>}
              {groups.map((group) => (
                <label className="inline check" key={group.id}>
                  <input
                    type="checkbox"
                    checked={item.groups.includes(group.id)}
                    onChange={(event) =>
                      updateItem(index, (it) => ({
                        ...it,
                        groups: event.target.checked ? [...it.groups, group.id] : it.groups.filter((id) => id !== group.id),
                      }))
                    }
                  />
                  {group.name}
                </label>
              ))}
            </div>
          </article>
        ))}
      </section>

      <section className="card">
        <div className="heading">
          <h2>Modifier groups</h2>
          <button
            onClick={() => {
              setGroups((all) => [
                ...all,
                { id: newId(), name: "New group", min: 0, max: 1, free: 0, repeat: false, modifiers: [{ id: newId(), name: "New modifier", prefix: "plain", price: 0, groups: [] }] },
              ]);
              touch();
            }}
          >
            Add group
          </button>
        </div>
        {groups.map((group, index) => (
          <article className="item" key={group.id} aria-label={`Group ${group.name}`}>
            <div className="row">
              <input
                aria-label="Group name"
                className="name"
                value={group.name}
                onChange={(event) => updateGroup(index, (g) => ({ ...g, name: event.target.value }))}
              />
              <label className="inline">
                Choose at least <CountInput label="Fewest" value={group.min} onChange={(min) => updateGroup(index, (g) => ({ ...g, min }))} />
              </label>
              <label className="inline">
                at most <CountInput label="Most" value={group.max} onChange={(max) => updateGroup(index, (g) => ({ ...g, max }))} />
              </label>
              <label className="inline">
                free <CountInput label="Free" value={group.free} onChange={(free) => updateGroup(index, (g) => ({ ...g, free }))} />
              </label>
              <label className="inline check">
                <input
                  type="checkbox"
                  checked={group.repeat}
                  onChange={(event) => updateGroup(index, (g) => ({ ...g, repeat: event.target.checked }))}
                />
                Repeatable
              </label>
              <button
                className="quiet"
                onClick={() => {
                  setGroups((all) => all.filter((_, at) => at !== index));
                  setItems((all) => all.map((item) => ({ ...item, groups: item.groups.filter((id) => id !== group.id) })));
                  touch();
                }}
              >
                Remove
              </button>
            </div>
            <table className="variants">
              <tbody>
                {group.modifiers.map((modifier, at) => (
                  <tr key={modifier.id}>
                    <td>
                      <input
                        aria-label="Modifier name"
                        value={modifier.name}
                        onChange={(event) =>
                          updateGroup(index, (g) => ({
                            ...g,
                            modifiers: g.modifiers.map((m, n) => (n === at ? { ...m, name: event.target.value } : m)),
                          }))
                        }
                      />
                    </td>
                    <td>
                      <select
                        aria-label="Prefix"
                        value={modifier.prefix}
                        onChange={(event) =>
                          updateGroup(index, (g) => ({
                            ...g,
                            modifiers: g.modifiers.map((m, n) => (n === at ? { ...m, prefix: event.target.value } : m)),
                          }))
                        }
                      >
                        {PREFIXES.map((prefix) => (
                          <option key={prefix}>{prefix}</option>
                        ))}
                      </select>
                    </td>
                    <td>
                      <PriceInput
                        label={`Price of ${modifier.name}`}
                        value={modifier.price}
                        digits={digits}
                        onChange={(price) =>
                          updateGroup(index, (g) => ({
                            ...g,
                            modifiers: g.modifiers.map((m, n) => (n === at ? { ...m, price } : m)),
                          }))
                        }
                      />
                    </td>
                    <td>
                      <button
                        className="quiet"
                        aria-label="Remove modifier"
                        onClick={() => updateGroup(index, (g) => ({ ...g, modifiers: g.modifiers.filter((_, n) => n !== at) }))}
                      >
                        ×
                      </button>
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
            <button
              className="quiet"
              onClick={() =>
                updateGroup(index, (g) => ({
                  ...g,
                  modifiers: [...g.modifiers, { id: newId(), name: "New modifier", prefix: "plain", price: 0, groups: [] }],
                }))
              }
            >
              Add modifier
            </button>
          </article>
        ))}
      </section>

      <section className="card">
        <div className="heading">
          <h2>Menu pages</h2>
          <button
            onClick={() => {
              setPages((all) => [...all, { name: "New page", buttons: [] }]);
              touch();
            }}
          >
            Add page
          </button>
        </div>
        {pages.map((page, index) => (
          <article className="item" key={index} aria-label={`Page ${page.name}`}>
            <div className="row">
              <input
                aria-label="Page name"
                className="name"
                value={page.name}
                onChange={(event) => updatePage(index, (p) => ({ ...p, name: event.target.value }))}
              />
              <select
                aria-label="Add a button"
                value=""
                onChange={(event) => {
                  const id = event.target.value;
                  if (id) updatePage(index, (p) => ({ ...p, buttons: [...p.buttons, id] }));
                }}
              >
                <option value="">Add a button…</option>
                {variants
                  .filter((variant) => !page.buttons.includes(variant.id))
                  .map((variant) => (
                    <option key={variant.id} value={variant.id}>
                      {variant.label}
                    </option>
                  ))}
              </select>
              <button
                className="quiet"
                onClick={() => {
                  setPages((all) => all.filter((_, at) => at !== index));
                  touch();
                }}
              >
                Remove
              </button>
            </div>
            <ul className="buttons">
              {page.buttons.map((id) => (
                <li key={id}>
                  {label(id)}
                  <button
                    className="quiet"
                    aria-label={`Remove ${label(id)} from ${page.name}`}
                    onClick={() => updatePage(index, (p) => ({ ...p, buttons: p.buttons.filter((b) => b !== id) }))}
                  >
                    ×
                  </button>
                </li>
              ))}
            </ul>
          </article>
        ))}
      </section>
    </div>
  );
}

function message(failed: unknown): string {
  return failed instanceof Error ? failed.message : String(failed);
}
