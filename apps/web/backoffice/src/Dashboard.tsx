import { useCallback, useEffect, useRef, useState } from "react";
import type { Api, DashboardData, DayFigures, Device } from "./api";
import { format } from "./money";

/** How often the dashboard asks again while it is shown (ADR-0037, decision 2). */
const EVERY_MS = 10_000;

function minutes(seconds: number | null): string {
  if (seconds === null) return "—";
  if (seconds < 60) return `${seconds} s`;
  return `${Math.floor(seconds / 60)} min ${seconds % 60} s`;
}

/** How `now` compares with `then`, as a week before: "+12%", or nothing to compare. */
function change(now: number, then: number): string {
  // Against the whole of that day, so a day not over reads low: said so. Found in review.
  if (then === 0) return now === 0 ? "none a week before either" : "none a week before";
  const percent = Math.round(((now - then) / Math.abs(then)) * 100);
  return `${percent >= 0 ? "+" : ""}${percent}% on all of a week before`;
}

function Stat({ label, value, compare }: { label: string; value: string; compare?: string | undefined }) {
  return (
    <div className="stat">
      <span>{label}</span>
      <strong>{value}</strong>
      {compare && <span className="compare">{compare}</span>}
    </div>
  );
}

function Figures({ day, before, digits }: { day: DayFigures; before: DayFigures; digits: number }) {
  const money = (minor: number) => format(minor, digits);
  const average = (figures: DayFigures) => (figures.orders === 0 ? 0 : Math.round(figures.total / figures.orders));
  return (
    <div className="stats" aria-label="The day's figures">
      <Stat label="Sales, with tax" value={money(day.total)} compare={change(day.total, before.total)} />
      <Stat label="Net sales" value={money(day.net)} compare={change(day.net, before.net)} />
      <Stat label="Orders" value={String(day.orders)} compare={change(day.orders, before.orders)} />
      <Stat label="Average order" value={money(average(day))} compare={change(average(day), average(before))} />
      <Stat label="Discounts and comps" value={money(day.discounts)} />
      <Stat label="Refunded" value={money(day.refunded)} />
      <Stat label="Tips" value={money(day.tips)} />
      <Stat label="Voided" value={String(day.voided)} />
    </div>
  );
}

/**
 * The location's dashboard (ADR-0037, decision 2; O-01): the day's sales against a week before,
 * by hour and by item, what is open, the kitchen's times, what is 86'd or below zero, and the
 * devices. Asked again every 10 s while the page is shown.
 */
export function Dashboard({ api, location }: { api: Api; location: string }) {
  const [data, setData] = useState<DashboardData | null>(null);
  const [devices, setDevices] = useState<Device[]>([]);
  const [date, setDate] = useState("");
  const [error, setError] = useState<string | null>(null);
  // Only the latest request's answer is shown: found in review, one for today answering after
  // one for the day picked showed today's under the other's date.
  const latest = useRef(0);
  const load = useCallback(() => {
    const asked = ++latest.current;
    Promise.all([api.dashboard(location, date || undefined), api.replication(location)])
      .then(([found, replication]) => {
        if (asked !== latest.current) return;
        setData(found);
        setDevices(replication.devices);
        setError(null);
      })
      .catch((failed: unknown) => {
        if (asked === latest.current) setError(failed instanceof Error ? failed.message : String(failed));
      });
  }, [api, location, date]);
  useEffect(() => {
    load();
    const every = setInterval(() => {
      if (document.visibilityState === "visible") load();
    }, EVERY_MS);
    return () => clearInterval(every);
  }, [load]);

  if (!data) return <p className="empty">{error ?? "Loading the dashboard…"}</p>;
  const digits = data.minorUnits;
  const busiest = Math.max(1, ...data.hours.map((hour) => hour.total));
  return (
    <div className="editor">
      <div className="toolbar card">
        <div>
          <strong>Dashboard</strong> <span className="muted">{data.date}</span>
        </div>
        <label className="inline">
          Day
          <input type="date" value={date || data.date} onChange={(event) => setDate(event.target.value)} />
        </label>
      </div>
      {error && <p className="error banner">{error}</p>}
      <section className="card">
        <Figures day={data.day} before={data.weekBefore} digits={digits} />
      </section>
      <div className="dashboard-grid">
        <section className="card" aria-label="Sales by hour">
          <h2>By hour</h2>
          {data.hours.length === 0 && <p className="muted">No order closed yet.</p>}
          <div className="hours">
            {data.hours.map((hour) => (
              <div key={hour.hour} className="hour">
                <span>{String(hour.hour).padStart(2, "0")}:00</span>
                <div className="bar-fill" style={{ width: `${Math.round((hour.total / busiest) * 100)}%` }} />
                <span className="number">{format(hour.total, digits)}</span>
              </div>
            ))}
          </div>
        </section>
        <section className="card" aria-label="Top items">
          <h2>Top items</h2>
          {data.topItems.length === 0 && <p className="muted">Nothing sold yet.</p>}
          <table>
            <tbody>
              {data.topItems.map((item) => (
                <tr key={`${item.name}:${item.unit}`}>
                  <td>{item.name}</td>
                  <td className="number">
                    {item.quantity}
                    {item.unit === "each" ? "" : ` ${item.unit}`}
                  </td>
                  <td className="number">{format(item.amount, digits)}</td>
                </tr>
              ))}
            </tbody>
          </table>
        </section>
        <section className="card" aria-label="Now">
          <h2>Now</h2>
          <div className="stats">
            <Stat label="Open orders" value={String(data.openOrders)} compare={data.openOrders > 0 ? `oldest ${minutes(data.oldestOpenSeconds)}` : undefined} />
            <Stat label="Kitchen tickets waiting" value={String(data.kitchen.waiting)} />
            <Stat label="Ticket time, median" value={minutes(data.kitchen.medianSeconds)} compare={`longest ${minutes(data.kitchen.longestSeconds)}`} />
          </div>
          <table aria-label="Tenders">
            <tbody>
              {data.tenders.map((tender) => (
                <tr key={tender.tender}>
                  <td>{tender.tender === "cash" ? "Cash" : "Card"}</td>
                  <td className="number">{tender.payments} payments</td>
                  <td className="number">{format(tender.amount, digits)}</td>
                </tr>
              ))}
            </tbody>
          </table>
        </section>
        <section className="card" aria-label="Look at">
          <h2>To look at</h2>
          {data.soldOut.length === 0 && data.belowZero.length === 0 && <p className="muted">Nothing 86'd, nothing below zero.</p>}
          {data.soldOut.length > 0 && <p>86'd: {data.soldOut.join(", ")}</p>}
          {data.belowZero.map((item) => (
            <p key={item.name} className="error">
              {item.name}: {item.onHand} on hand
            </p>
          ))}
          <h3>Devices</h3>
          {devices.map((device) => (
            <p key={device.id}>
              <span className={device.connected ? "dot on" : "dot"} aria-hidden="true" />
              {device.name} <span className="muted">{device.connected ? "connected" : "not connected"}</span>
            </p>
          ))}
        </section>
      </div>
    </div>
  );
}
