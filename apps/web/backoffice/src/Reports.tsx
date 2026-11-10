import { useEffect, useRef, useState } from "react";
import type { Api, SalesReport } from "./api";
import { format } from "./money";

/** Sales over business days: by day, by item, and by tender, each total the kernel's. */
export function Reports({ api, location }: { api: Api; location: string }) {
  const [from, setFrom] = useState("");
  const [to, setTo] = useState("");
  const [report, setReport] = useState<SalesReport | null>(null);
  const [error, setError] = useState<string | null>(null);
  // Only the answer to the latest request is shown: an earlier one that comes late is dropped.
  const latest = useRef(0);
  const run = (first?: string, last?: string) => {
    const asked = ++latest.current;
    api
      .sales(location, first || undefined, last || undefined)
      .then((found) => {
        if (asked !== latest.current) return;
        setReport(found);
        setError(null);
        // The range the API chose, today's, fills dates left empty; it never replaces typed ones.
        setFrom((typed) => typed || found.from);
        setTo((typed) => typed || found.to);
      })
      .catch((failed: unknown) => {
        if (asked !== latest.current) return;
        setError(failed instanceof Error ? failed.message : String(failed));
      });
  };
  // Today's, to begin with.
  useEffect(() => run(), [location]);
  const money = (minor: number) => (report ? `${format(minor, report.minorUnits)} ${report.currency}` : "");
  return (
    <section className="card">
      <h2>Sales</h2>
      <form
        className="row"
        onSubmit={(event) => {
          event.preventDefault();
          run(from, to);
        }}
      >
        <label>
          From
          <input type="date" value={from} onChange={(event) => setFrom(event.target.value)} />
        </label>
        <label>
          To
          <input type="date" value={to} onChange={(event) => setTo(event.target.value)} />
        </label>
        <button type="submit">Show</button>
      </form>
      {error && <p className="error">{error}</p>}
      {report && (
        <>
          <div className="stats">
            <div className="stat">
              <span>Orders</span>
              <strong data-testid="orders">{report.orders}</strong>
            </div>
            <div className="stat">
              <span>Sales</span>
              <strong data-testid="total">{money(report.total)}</strong>
            </div>
          </div>
          {report.otherCurrency > 0 && (
            <p className="muted">
              {report.otherCurrency} orders in another currency are left out of these totals.
            </p>
          )}
          <h3>By business day</h3>
          <table>
            <thead>
              <tr>
                <th>Day</th>
                <th className="number">Orders</th>
                <th className="number">Sales</th>
              </tr>
            </thead>
            <tbody>
              {report.days.map((day) => (
                <tr key={day.date}>
                  <td>{day.date}</td>
                  <td className="number">{day.orders}</td>
                  <td className="number">{money(day.total)}</td>
                </tr>
              ))}
            </tbody>
          </table>
          <h3>By item</h3>
          <table aria-label="Sales by item">
            <thead>
              <tr>
                <th>Item</th>
                <th className="number">Sold</th>
                <th className="number">Amount, before tax</th>
              </tr>
            </thead>
            <tbody>
              {report.items.map((item) => (
                <tr key={`${item.name} ${item.unit}`}>
                  <td>{item.name}</td>
                  <td className="number">
                    {item.quantity}
                    {item.unit === "each" ? "" : ` ${item.unit}`}
                  </td>
                  <td className="number">{money(item.amount)}</td>
                </tr>
              ))}
            </tbody>
          </table>
          <h3>By tender</h3>
          <table aria-label="Payments by tender">
            <thead>
              <tr>
                <th>Tender</th>
                <th className="number">Payments</th>
                <th className="number">Taken</th>
              </tr>
            </thead>
            <tbody>
              {report.tenders.map((tender) => (
                <tr key={tender.tender}>
                  <td>{tender.tender === "cash" ? "Cash" : "Card"}</td>
                  <td className="number">{tender.payments}</td>
                  <td className="number">{money(tender.amount)}</td>
                </tr>
              ))}
            </tbody>
          </table>
        </>
      )}
    </section>
  );
}
