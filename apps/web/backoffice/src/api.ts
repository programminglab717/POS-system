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

export interface ActionInfo {
  name: string;
  hasAmount: boolean;
  hasPercent: boolean;
}

export interface Permission {
  action: string;
  /** In minor units of the location's currency; absent for no limit. */
  mostAmount?: number;
  /** In basis points; absent for no limit. */
  mostPercent?: number;
}

export interface Role {
  id: string;
  name: string;
  permissions: Permission[];
}

export interface StaffMember {
  id: string;
  name: string;
  role?: string;
  hasPin: boolean;
}

export interface Staff {
  version: string;
  actions: ActionInfo[];
  roles: Role[];
  team: StaffMember[];
}

export interface StaffEdit {
  base?: string;
  roles: Role[];
  /** A member's new PIN, its digits; absent to keep the one they have. */
  team: { id: string; name: string; role?: string; pin?: string }[];
}

export interface PrinterSetting {
  host: string;
  port: number;
}

export interface TaxSetting {
  id: string;
  name: string;
  /** A percentage: "8.875". */
  rate: string;
  categories: string[];
  dining?: "on_premises" | "to_go";
}

export interface StationSetting {
  id: string;
  name: string;
  expo: boolean;
  screen: boolean;
  printer?: PrinterSetting;
}

export interface RouteSetting {
  station: string;
  items: string[];
  modifiers: string[];
  modes: string[];
}

export interface KitchenSetting {
  stations: StationSetting[];
  routes: RouteSetting[];
  defaultStation?: string;
  lateIshSeconds: number;
  lateSeconds: number;
  fallbackPrinter?: PrinterSetting;
}

/** What a location's settings are (ADR-0037, decision 1). */
export interface SettingsBody {
  name: string;
  address: string[];
  timeZone: string;
  /** Local time business days begin, HH:MM. */
  cutoff: string;
  cashRounding?: { mode: string; increment: number };
  rounding: { extension: string; discounts: string; taxScope: "line" | "document"; tax: string };
  taxes: TaxSetting[];
  tipping?: { presets: number[]; prompt: "screen" | "terminal" };
  kitchen?: KitchenSetting;
}

export interface Settings extends SettingsBody {
  version: string;
  location: string;
  currency: string;
}

export interface SettingsEdit extends SettingsBody {
  base?: string;
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

export interface DayFigures {
  orders: number;
  net: number;
  total: number;
  discounts: number;
  voided: number;
  refunded: number;
  tips: number;
}

/** A business day's dashboard (ADR-0037, decision 2). */
export interface DashboardData {
  date: string;
  currency: string;
  minorUnits: number;
  day: DayFigures;
  weekBefore: DayFigures;
  hours: { hour: number; orders: number; total: number }[];
  topItems: { name: string; quantity: string; unit: string; amount: number }[];
  tenders: { tender: string; payments: number; amount: number }[];
  openOrders: number;
  oldestOpenSeconds: number | null;
  kitchen: { waiting: number; bumped: number; medianSeconds: number | null; longestSeconds: number | null };
  soldOut: string[];
  belowZero: { name: string; onHand: string }[];
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

  staff(location: string) {
    return this.call<Staff>("GET", `/api/locations/${location}/staff`);
  }

  checkStaff(location: string, edit: StaffEdit) {
    return this.call<{ version: string }>("POST", `/api/locations/${location}/staff/check`, edit);
  }

  publishStaff(location: string, edit: StaffEdit) {
    return this.call<{ version: string }>("PUT", `/api/locations/${location}/staff`, edit);
  }

  settings(location: string) {
    return this.call<Settings>("GET", `/api/locations/${location}/settings`);
  }

  checkSettings(location: string, edit: SettingsEdit) {
    return this.call<{ version: string }>("POST", `/api/locations/${location}/settings/check`, edit);
  }

  publishSettings(location: string, edit: SettingsEdit) {
    return this.call<{ version: string }>("PUT", `/api/locations/${location}/settings`, edit);
  }

  dashboard(location: string, date?: string) {
    const suffix = date ? `?date=${encodeURIComponent(date)}` : "";
    return this.call<DashboardData>("GET", `/api/locations/${location}/dashboard${suffix}`);
  }

  sales(location: string, from?: string, to?: string) {
    const query = new URLSearchParams();
    if (from) query.set("from", from);
    if (to) query.set("to", to);
    const suffix = query.size ? `?${query}` : "";
    return this.call<SalesReport>("GET", `/api/locations/${location}/reports/sales${suffix}`);
  }
}
