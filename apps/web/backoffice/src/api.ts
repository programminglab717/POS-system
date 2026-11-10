// The cell's API (ADR-0028, decision 4), as the back office calls it.

export interface LocationInfo {
  id: string;
  name: string;
}

export interface Variant {
  id: string;
  name?: string | null;
  /** In minor units of the location's currency. */
  price: number;
}

export interface Item {
  id: string;
  name: string;
  taxCategory: string;
  variants: Variant[];
  groups: string[];
}

export interface Modifier {
  id: string;
  name: string;
  prefix: string;
  price: number;
  groups: string[];
}

export interface Group {
  id: string;
  name: string;
  min: number;
  max: number;
  free: number;
  repeat: boolean;
  modifiers: Modifier[];
}

export interface Page {
  name: string;
  buttons: string[];
}

export interface TaxCategory {
  id: string;
  taxes: string[];
}

export interface Catalog {
  version: string;
  currency: string;
  minorUnits: number;
  taxCategories: TaxCategory[];
  items: Item[];
  groups: Group[];
  pages: Page[];
}

export interface CatalogEdit {
  base?: string;
  items: Item[];
  groups: Group[];
  pages: Page[];
}

export interface Device {
  id: string;
  name: string;
  role: "hub" | "cloud" | "device";
  hubPriority: number | null;
  connected: boolean;
  held: number;
}

export interface SalesReport {
  from: string;
  to: string;
  currency: string;
  minorUnits: number;
  orders: number;
  total: number;
  days: { date: string; orders: number; total: number }[];
  items: { name: string; quantity: string; unit: string; amount: number }[];
  tenders: { tender: string; payments: number; amount: number }[];
  otherCurrency: number;
}

/** An answer the API refused, with its reason. */
export class ApiError extends Error {
  constructor(
    readonly status: number,
    message: string,
    readonly version?: string,
  ) {
    super(message);
  }
}

export class Api {
  constructor(private readonly token: string) {}

  private async call<T>(method: string, path: string, body?: unknown): Promise<T> {
    const headers: Record<string, string> = { Authorization: `Bearer ${this.token}` };
    const init: RequestInit = { method, headers };
    if (body !== undefined) {
      headers["Content-Type"] = "application/json";
      init.body = JSON.stringify(body);
    }
    const response = await fetch(path, init);
    const text = await response.text();
    const json: unknown = text ? JSON.parse(text) : null;
    if (!response.ok) {
      const reason = json as { error?: string; version?: string } | null;
      throw new ApiError(response.status, reason?.error ?? response.statusText, reason?.version);
    }
    return json as T;
  }

  locations() {
    return this.call<LocationInfo[]>("GET", "/api/locations");
  }

  replication(location: string) {
    return this.call<{ devices: Device[] }>("GET", `/api/locations/${location}/replication`);
  }

  catalog(location: string) {
    return this.call<Catalog>("GET", `/api/locations/${location}/catalog`);
  }

  check(location: string, edit: CatalogEdit) {
    return this.call<{ version: string }>("POST", `/api/locations/${location}/catalog/check`, edit);
  }

  publish(location: string, edit: CatalogEdit) {
    return this.call<{ version: string }>("PUT", `/api/locations/${location}/catalog`, edit);
  }

  sales(location: string, from?: string, to?: string) {
    const query = new URLSearchParams();
    if (from) query.set("from", from);
    if (to) query.set("to", to);
    const suffix = query.size ? `?${query}` : "";
    return this.call<SalesReport>("GET", `/api/locations/${location}/reports/sales${suffix}`);
  }
}
