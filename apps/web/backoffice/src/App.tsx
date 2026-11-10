import { useEffect, useState } from "react";
import { Api, ApiError, type LocationInfo } from "./api";
import { CatalogEditor } from "./CatalogEditor";
import { Devices } from "./Devices";
import { Reports } from "./Reports";

type Tab = "catalog" | "reports" | "devices";

const TOKEN = "keel.token";

function stored(): string | null {
  try {
    return sessionStorage.getItem(TOKEN);
  } catch {
    return null;
  }
}

function store(token: string | null) {
  try {
    if (token) sessionStorage.setItem(TOKEN, token);
    else sessionStorage.removeItem(TOKEN);
  } catch {
    // A browser that keeps nothing asks for the token again on reload.
  }
}

/** Asks for the operator's token, and checks it against the API. */
function SignIn({ onSignedIn, refused }: { onSignedIn: (token: string) => void; refused: boolean }) {
  const [token, setToken] = useState("");
  return (
    <main className="sign-in">
      <form
        className="card"
        onSubmit={(event) => {
          event.preventDefault();
          if (token.trim()) onSignedIn(token.trim());
        }}
      >
        <h1>Keel back office</h1>
        <label>
          Operator token
          <input
            type="password"
            autoComplete="current-password"
            value={token}
            onChange={(event) => setToken(event.target.value)}
          />
        </label>
        {refused && <p className="error">That token isn't the cell's.</p>}
        <button type="submit" className="primary">
          Sign in
        </button>
      </form>
    </main>
  );
}

export function App() {
  const [token, setToken] = useState<string | null>(stored);
  const [refused, setRefused] = useState(false);
  const [locations, setLocations] = useState<LocationInfo[] | null>(null);
  const [location, setLocation] = useState<string | null>(null);
  const [tab, setTab] = useState<Tab>("catalog");
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    if (!token) return;
    let live = true;
    new Api(token)
      .locations()
      .then((found) => {
        if (!live) return;
        store(token);
        setRefused(false);
        setLocations(found);
        setLocation((current) => current ?? found[0]?.id ?? null);
      })
      .catch((failed: unknown) => {
        if (!live) return;
        if (failed instanceof ApiError && failed.status === 401) {
          store(null);
          setToken(null);
          setRefused(true);
        } else {
          setError(failed instanceof Error ? failed.message : String(failed));
        }
      });
    return () => {
      live = false;
    };
  }, [token]);

  if (!token) return <SignIn onSignedIn={setToken} refused={refused} />;
  const api = new Api(token);
  const current = locations?.find((found) => found.id === location);
  return (
    <div className="shell">
      <header className="bar">
        <strong className="brand">Keel</strong>
        {locations && locations.length > 0 && (
          <label className="inline">
            <span className="hidden">Location</span>
            <select value={location ?? ""} onChange={(event) => setLocation(event.target.value)}>
              {locations.map((found) => (
                <option key={found.id} value={found.id}>
                  {found.name}
                </option>
              ))}
            </select>
          </label>
        )}
        <nav className="tabs" role="tablist">
          {(["catalog", "reports", "devices"] as const).map((name) => (
            <button
              key={name}
              role="tab"
              aria-selected={tab === name}
              className={tab === name ? "tab active" : "tab"}
              onClick={() => setTab(name)}
            >
              {name[0]?.toUpperCase() + name.slice(1)}
            </button>
          ))}
        </nav>
        <button
          className="quiet"
          onClick={() => {
            store(null);
            setToken(null);
            setLocations(null);
          }}
        >
          Sign out
        </button>
      </header>
      {error && <p className="error banner">{error}</p>}
      {locations && locations.length === 0 && <p className="empty">The cell serves no location.</p>}
      {current && (
        <main key={current.id}>
          {tab === "catalog" && <CatalogEditor api={api} location={current.id} />}
          {tab === "reports" && <Reports api={api} location={current.id} />}
          {tab === "devices" && <Devices api={api} location={current.id} />}
        </main>
      )}
    </div>
  );
}
