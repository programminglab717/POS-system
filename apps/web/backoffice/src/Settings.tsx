import { useEffect, useState } from "react";
import {
  type Api,
  ApiError,
  type Catalog,
  type KitchenSetting,
  type PrinterSetting,
  type Settings,
  type SettingsBody,
  type SettingsEdit,
} from "./api";
import { newId } from "./money";

type Status = { kind: "error" | "ok"; text: string } | null;

const MODES: [string, string][] = [
  ["half_away_from_zero", "Half away from zero"],
  ["half_even", "Half to even (banker's)"],
  ["half_toward_zero", "Half toward zero"],
  ["away_from_zero", "Up, away from zero"],
  ["toward_zero", "Down, toward zero"],
  ["ceiling", "Ceiling"],
  ["floor", "Floor"],
];

const ORDER_MODES: [string, string][] = [
  ["dine_in", "Dine in"],
  ["takeout", "Takeout"],
  ["pickup", "Pickup"],
  ["delivery", "Delivery"],
  ["drive_thru", "Drive-thru"],
  ["curbside", "Curbside"],
];

function message(failed: unknown): string {
  return failed instanceof Error ? failed.message : String(failed);
}

function body(settings: Settings): SettingsBody {
  const { version: _version, location: _location, currency: _currency, ...rest } = settings;
  return rest;
}

function ModeSelect({ label, value, onChange }: { label: string; value: string; onChange: (mode: string) => void }) {
  return (
    <label>
      {label}
      <select value={value} onChange={(event) => onChange(event.target.value)}>
        {MODES.map(([mode, words]) => (
          <option key={mode} value={mode}>
            {words}
          </option>
        ))}
      </select>
    </label>
  );
}

function PrinterFields({
  label,
  printer,
  onChange,
}: {
  label: string;
  printer: PrinterSetting | undefined;
  onChange: (printer: PrinterSetting | undefined) => void;
}) {
  return (
    <span className="row">
      <input
        aria-label={`${label}: host`}
        placeholder="no printer"
        value={printer?.host ?? ""}
        onChange={(event) =>
          onChange(event.target.value ? { host: event.target.value, port: printer?.port ?? 9100 } : undefined)
        }
      />
      {printer && (
        <input
          className="count"
          aria-label={`${label}: port`}
          inputMode="numeric"
          value={printer.port}
          onChange={(event) => onChange({ ...printer, port: Number(event.target.value.replace(/\D/g, "")) || 0 })}
        />
      )}
    </span>
  );
}

/**
 * The location's settings (ADR-0037, decision 1): its name, address and business day, taxes and
 * rounding, tipping, and the kitchen. Checked whole by the kernel, published as a new profile.
 */
export function SettingsEditor({ api, location }: { api: Api; location: string }) {
  const [read, setRead] = useState<Settings | null>(null);
  const [settings, setSettings] = useState<SettingsBody | null>(null);
  const [catalog, setCatalog] = useState<Catalog | null>(null);
  const [changed, setChanged] = useState(false);
  const [busy, setBusy] = useState(false);
  const [status, setStatus] = useState<Status>(null);

  const load = () =>
    Promise.all([api.settings(location), api.catalog(location)])
      .then(([found, items]) => {
        setRead(found);
        setSettings(body(found));
        setCatalog(items);
        setChanged(false);
      })
      .catch((failed: unknown) => setStatus({ kind: "error", text: message(failed) }));
  useEffect(() => {
    void load();
  }, [location]);

  if (!read || !settings) return <p className="empty">{status?.text ?? "Loading the settings…"}</p>;
  const update = (change: (settings: SettingsBody) => SettingsBody) => {
    setSettings((current) => (current ? change(current) : current));
    setChanged(true);
    setStatus(null);
  };
  const kitchen = (change: (kitchen: KitchenSetting) => KitchenSetting) =>
    update((s) => (s.kitchen ? { ...s, kitchen: change(s.kitchen) } : s));
  const edit = (): SettingsEdit => ({ base: read.version, ...settings });
  const run = async (publish: boolean) => {
    setBusy(true);
    try {
      if (publish) {
        const { version } = await api.publishSettings(location, edit());
        await load();
        setStatus({ kind: "ok", text: `Published version ${version.slice(0, 12)}. Registers take it within a minute.` });
      } else {
        await api.checkSettings(location, edit());
        setStatus({ kind: "ok", text: "The settings pass every check." });
      }
    } catch (failed) {
      const conflict = failed instanceof ApiError && failed.status === 409;
      setStatus({
        kind: "error",
        text: conflict ? "Another profile was published since this one was read: reload it first." : message(failed),
      });
    } finally {
      setBusy(false);
    }
  };
  const categories = catalog?.taxCategories.map((category) => category.id) ?? [];
  const items = catalog?.items ?? [];

  return (
    <div className="editor">
      <div className="toolbar card">
        <div>
          <strong>Settings</strong>{" "}
          <span className="muted" title={read.version}>
            version {read.version.slice(0, 12)}
          </span>
          {changed && <span className="badge">Unpublished changes</span>}
        </div>
        <div className="actions">
          <button disabled={busy || !changed} onClick={() => void load().then(() => setStatus(null))}>
            Discard
          </button>
          <button disabled={busy} onClick={() => void run(false)}>
            Check
          </button>
          <button className="primary" disabled={busy || !changed} onClick={() => void run(true)}>
            Publish
          </button>
        </div>
      </div>
      {status && (
        <p role="status" className={status.kind === "error" ? "error banner" : "ok banner"}>
          {status.text}
        </p>
      )}

      <section className="card" aria-label="The location">
        <div className="heading">
          <h2>The location</h2>
          <span className="muted">Prices in {read.currency}</span>
        </div>
        <label>
          Name
          <input value={settings.name} onChange={(event) => update((s) => ({ ...s, name: event.target.value }))} />
        </label>
        <label>
          Address, a line each
          <textarea
            rows={3}
            value={settings.address.join("\n")}
            onChange={(event) => update((s) => ({ ...s, address: event.target.value.split("\n") }))}
          />
        </label>
        <label>
          Time zone
          <input
            value={settings.timeZone}
            placeholder="America/New_York"
            onChange={(event) => update((s) => ({ ...s, timeZone: event.target.value.trim() }))}
          />
        </label>
        <label>
          Business days begin at
          <input type="time" value={settings.cutoff} onChange={(event) => update((s) => ({ ...s, cutoff: event.target.value }))} />
        </label>
      </section>

      <section className="card" aria-label="Taxes and rounding">
        <div className="heading">
          <h2>Taxes and rounding</h2>
          <button
            onClick={() =>
              update((s) => ({ ...s, taxes: [...s.taxes, { id: newId(), name: "New tax", rate: "0", categories: [] }] }))
            }
          >
            Add tax
          </button>
        </div>
        <table aria-label="Taxes">
          <thead>
            <tr>
              <th>Name</th>
              <th>Rate, %</th>
              <th>Tax categories</th>
              <th>Applies</th>
              <th />
            </tr>
          </thead>
          <tbody>
            {settings.taxes.map((tax, index) => {
              const change = (next: typeof tax) =>
                update((s) => ({ ...s, taxes: s.taxes.map((t, at) => (at === index ? next : t)) }));
              return (
                <tr key={tax.id}>
                  <td>
                    <input aria-label="Tax name" value={tax.name} onChange={(event) => change({ ...tax, name: event.target.value })} />
                  </td>
                  <td>
                    <input
                      className="count"
                      aria-label={`Rate of ${tax.name}`}
                      inputMode="decimal"
                      value={tax.rate}
                      onChange={(event) => change({ ...tax, rate: event.target.value.trim() })}
                    />
                  </td>
                  <td>
                    {categories.map((category) => (
                      <label key={category} className="check">
                        <input
                          type="checkbox"
                          checked={tax.categories.includes(category)}
                          onChange={(event) =>
                            change({
                              ...tax,
                              categories: event.target.checked
                                ? [...tax.categories, category]
                                : tax.categories.filter((known) => known !== category),
                            })
                          }
                        />
                        {category.slice(-6)}
                      </label>
                    ))}
                  </td>
                  <td>
                    <select
                      aria-label={`Where ${tax.name} applies`}
                      value={tax.dining ?? ""}
                      onChange={(event) => {
                        const { dining: _dining, ...rest } = tax;
                        const value = event.target.value;
                        change(value === "on_premises" || value === "to_go" ? { ...rest, dining: value } : rest);
                      }}
                    >
                      <option value="">Always</option>
                      <option value="on_premises">Eaten here</option>
                      <option value="to_go">To go</option>
                    </select>
                  </td>
                  <td>
                    <button
                      className="quiet"
                      aria-label={`Remove ${tax.name}`}
                      onClick={() => update((s) => ({ ...s, taxes: s.taxes.filter((_, at) => at !== index) }))}
                    >
                      ×
                    </button>
                  </td>
                </tr>
              );
            })}
          </tbody>
        </table>
        <div className="row">
          <label>
            Tax is rounded
            <select
              value={settings.rounding.taxScope}
              onChange={(event) =>
                update((s) => ({ ...s, rounding: { ...s.rounding, taxScope: event.target.value === "line" ? "line" : "document" } }))
              }
            >
              <option value="document">Once per order</option>
              <option value="line">Per line</option>
            </select>
          </label>
          <ModeSelect label="Tax rounds" value={settings.rounding.tax} onChange={(mode) => update((s) => ({ ...s, rounding: { ...s.rounding, tax: mode } }))} />
          <ModeSelect
            label="Discounts round"
            value={settings.rounding.discounts}
            onChange={(mode) => update((s) => ({ ...s, rounding: { ...s.rounding, discounts: mode } }))}
          />
          <ModeSelect
            label="Prices by weight round"
            value={settings.rounding.extension}
            onChange={(mode) => update((s) => ({ ...s, rounding: { ...s.rounding, extension: mode } }))}
          />
        </div>
        <label className="check">
          <input
            type="checkbox"
            checked={settings.cashRounding !== undefined}
            onChange={(event) =>
              update((s) => {
                const { cashRounding: _cash, ...rest } = s;
                return event.target.checked ? { ...rest, cashRounding: { mode: "half_away_from_zero", increment: 5 } } : rest;
              })
            }
          />
          Round cash
        </label>
        {settings.cashRounding && (
          <div className="row">
            <label>
              To multiples of (minor units)
              <input
                className="count"
                inputMode="numeric"
                value={settings.cashRounding.increment}
                onChange={(event) => {
                  const increment = Number(event.target.value.replace(/\D/g, "")) || 0;
                  update((s) => (s.cashRounding ? { ...s, cashRounding: { ...s.cashRounding, increment } } : s));
                }}
              />
            </label>
            <ModeSelect
              label="Cash rounds"
              value={settings.cashRounding.mode}
              onChange={(mode) => update((s) => (s.cashRounding ? { ...s, cashRounding: { ...s.cashRounding, mode } } : s))}
            />
          </div>
        )}
      </section>

      <section className="card" aria-label="Tipping">
        <div className="heading">
          <h2>Tipping</h2>
          <label className="check">
            <input
              type="checkbox"
              checked={settings.tipping !== undefined}
              onChange={(event) =>
                update((s) => {
                  const { tipping: _tipping, ...rest } = s;
                  return event.target.checked ? { ...rest, tipping: { presets: [1500, 1800, 2000], prompt: "screen" } } : rest;
                })
              }
            />
            Ask for tips
          </label>
        </div>
        {settings.tipping && (
          <div className="row">
            <label>
              Percentages offered, lowest first
              <input
                aria-label="Tip percentages"
                value={settings.tipping.presets.map((preset) => preset / 100).join(", ")}
                onChange={(event) => {
                  const presets = event.target.value
                    .split(",")
                    .map((part) => Math.round(Number(part.trim()) * 100))
                    .filter((preset) => Number.isFinite(preset) && preset > 0);
                  update((s) => (s.tipping ? { ...s, tipping: { ...s.tipping, presets } } : s));
                }}
              />
            </label>
            <label>
              Asked on
              <select
                value={settings.tipping.prompt}
                onChange={(event) => {
                  const prompt = event.target.value === "terminal" ? "terminal" : "screen";
                  update((s) => (s.tipping ? { ...s, tipping: { ...s.tipping, prompt } } : s));
                }}
              >
                <option value="screen">The register's screen</option>
                <option value="terminal">The card terminal</option>
              </select>
            </label>
          </div>
        )}
      </section>

      <section className="card" aria-label="The kitchen">
        <div className="heading">
          <h2>The kitchen</h2>
          <label className="check">
            <input
              type="checkbox"
              checked={settings.kitchen !== undefined}
              onChange={(event) =>
                update((s) => {
                  const { kitchen: _kitchen, ...rest } = s;
                  if (!event.target.checked) return rest;
                  const station = { id: newId(), name: "Kitchen", expo: false, screen: true };
                  return { ...rest, kitchen: { stations: [station], routes: [], defaultStation: station.id, lateIshSeconds: 300, lateSeconds: 600 } };
                })
              }
            />
            Send orders to a kitchen
          </label>
        </div>
        {settings.kitchen && (
          <>
            <div className="heading">
              <h3>Stations</h3>
              <button
                onClick={() =>
                  kitchen((k) => ({ ...k, stations: [...k.stations, { id: newId(), name: "New station", expo: false, screen: true }] }))
                }
              >
                Add station
              </button>
            </div>
            <table aria-label="Stations">
              <thead>
                <tr>
                  <th>Name</th>
                  <th>Expo</th>
                  <th>Screen</th>
                  <th>Printer</th>
                  <th />
                </tr>
              </thead>
              <tbody>
                {settings.kitchen.stations.map((station, index) => {
                  const change = (next: typeof station) =>
                    kitchen((k) => ({ ...k, stations: k.stations.map((s, at) => (at === index ? next : s)) }));
                  return (
                    <tr key={station.id}>
                      <td>
                        <input aria-label="Station name" value={station.name} onChange={(event) => change({ ...station, name: event.target.value })} />
                      </td>
                      <td>
                        <input type="checkbox" aria-label={`${station.name} is expo`} checked={station.expo} onChange={(event) => change({ ...station, expo: event.target.checked })} />
                      </td>
                      <td>
                        <input type="checkbox" aria-label={`${station.name} has a screen`} checked={station.screen} onChange={(event) => change({ ...station, screen: event.target.checked })} />
                      </td>
                      <td>
                        <PrinterFields
                          label={`${station.name}'s printer`}
                          printer={station.printer}
                          onChange={(printer) => {
                            const { printer: _printer, ...rest } = station;
                            change(printer ? { ...rest, printer } : rest);
                          }}
                        />
                      </td>
                      <td>
                        <button
                          className="quiet"
                          aria-label={`Remove ${station.name}`}
                          onClick={() =>
                            kitchen((k) => {
                              const { defaultStation, ...rest } = k;
                              const kept = { ...rest, stations: k.stations.filter((_, at) => at !== index), routes: k.routes.filter((route) => route.station !== station.id) };
                              return defaultStation && defaultStation !== station.id ? { ...kept, defaultStation } : kept;
                            })
                          }
                        >
                          ×
                        </button>
                      </td>
                    </tr>
                  );
                })}
              </tbody>
            </table>
            <div className="row">
              <label>
                What no route names goes to
                <select
                  value={settings.kitchen.defaultStation ?? ""}
                  onChange={(event) =>
                    kitchen((k) => {
                      const { defaultStation: _default, ...rest } = k;
                      return event.target.value ? { ...rest, defaultStation: event.target.value } : rest;
                    })
                  }
                >
                  <option value="">No station but expo</option>
                  {settings.kitchen.stations
                    .filter((station) => !station.expo)
                    .map((station) => (
                      <option key={station.id} value={station.id}>
                        {station.name}
                      </option>
                    ))}
                </select>
              </label>
              <label>
                Late-ish after (minutes)
                <input
                  className="count"
                  inputMode="numeric"
                  value={settings.kitchen.lateIshSeconds / 60}
                  onChange={(event) => kitchen((k) => ({ ...k, lateIshSeconds: (Number(event.target.value.replace(/\D/g, "")) || 0) * 60 }))}
                />
              </label>
              <label>
                Late after (minutes)
                <input
                  className="count"
                  inputMode="numeric"
                  value={settings.kitchen.lateSeconds / 60}
                  onChange={(event) => kitchen((k) => ({ ...k, lateSeconds: (Number(event.target.value.replace(/\D/g, "")) || 0) * 60 }))}
                />
              </label>
              <label>
                When no screen shows a ticket, print it at
                <PrinterFields
                  label="The fallback printer"
                  printer={settings.kitchen.fallbackPrinter}
                  onChange={(printer) =>
                    kitchen((k) => {
                      const { fallbackPrinter: _fallback, ...rest } = k;
                      return printer ? { ...rest, fallbackPrinter: printer } : rest;
                    })
                  }
                />
              </label>
            </div>
            <div className="heading">
              <h3>Routes</h3>
              <button
                disabled={settings.kitchen.stations.length === 0}
                onClick={() =>
                  kitchen((k) => ({ ...k, routes: [...k.routes, { station: k.stations[0]?.id ?? "", items: [], modifiers: [], modes: [] }] }))
                }
              >
                Add route
              </button>
            </div>
            {settings.kitchen.routes.map((route, index) => {
              const change = (next: typeof route) => kitchen((k) => ({ ...k, routes: k.routes.map((r, at) => (at === index ? next : r)) }));
              const toggle = (list: string[], value: string, on: boolean) => (on ? [...list, value] : list.filter((known) => known !== value));
              return (
                <div key={index} className="card" aria-label={`Route ${index + 1}`}>
                  <div className="heading">
                    <label className="inline">
                      Station
                      <select value={route.station} onChange={(event) => change({ ...route, station: event.target.value })}>
                        {settings.kitchen?.stations.map((station) => (
                          <option key={station.id} value={station.id}>
                            {station.name}
                          </option>
                        ))}
                      </select>
                    </label>
                    <button
                      className="quiet"
                      aria-label={`Remove route ${index + 1}`}
                      onClick={() => kitchen((k) => ({ ...k, routes: k.routes.filter((_, at) => at !== index) }))}
                    >
                      ×
                    </button>
                  </div>
                  <div className="buttons">
                    {items.map((item) => (
                      <label key={item.id} className="check">
                        <input
                          type="checkbox"
                          checked={route.items.includes(item.id)}
                          onChange={(event) => change({ ...route, items: toggle(route.items, item.id, event.target.checked) })}
                        />
                        {item.name}
                      </label>
                    ))}
                  </div>
                  <div className="buttons">
                    <span className="muted">Only for:</span>
                    {ORDER_MODES.map(([mode, words]) => (
                      <label key={mode} className="check">
                        <input
                          type="checkbox"
                          checked={route.modes.includes(mode)}
                          onChange={(event) => change({ ...route, modes: toggle(route.modes, mode, event.target.checked) })}
                        />
                        {words}
                      </label>
                    ))}
                    <span className="muted">{route.modes.length === 0 ? "(every order)" : ""}</span>
                  </div>
                </div>
              );
            })}
          </>
        )}
      </section>
    </div>
  );
}
