import { useEffect, useState } from "react";
import { type Api, ApiError, type Permission, type Role, type Staff, type StaffEdit } from "./api";
import { format, newId, parse } from "./money";

type Status = { kind: "error" | "ok"; text: string } | null;

/** What each action is, in words for the operator. */
const WORDS: Record<string, string> = {
  "order.void": "Drop an order with items on it",
  "line.remove": "Take an item off an order",
  "discount.apply": "Discount",
  "price.override": "Change a price",
  "refund.issue": "Refund",
  "drawer.no_sale": "Open the drawer with no sale",
  "drawer.variance": "Close the drawer over or short",
  "drawer.paid_out": "Pay out of the drawer",
  "receipt.reprint": "Print a receipt again",
  "item.availability": "Mark an item out (86)",
};

interface MemberRow {
  id: string;
  name: string;
  role?: string;
  hasPin: boolean;
  /** A new PIN typed, if one is. */
  pin: string;
}

/**
 * The location's staff (ADR-0029): its roles, what each may do and up to what limit, and its
 * team, each member's role and PIN. A PIN is never read back: one typed replaces the member's.
 */
export function StaffEditor({ api, location }: { api: Api; location: string }) {
  const [staff, setStaff] = useState<Staff | null>(null);
  const [roles, setRoles] = useState<Role[]>([]);
  const [team, setTeam] = useState<MemberRow[]>([]);
  const [changed, setChanged] = useState(false);
  const [busy, setBusy] = useState(false);
  const [status, setStatus] = useState<Status>(null);
  // Amounts are shown in the catalog's currency: two digits, as every currency the profile
  // takes but a few. The API checks every limit whole.
  const [digits, setDigits] = useState(2);
  // The limits whose text isn't one: while any is, nothing is checked or published.
  const [badLimits, setBadLimits] = useState<ReadonlySet<string>>(new Set());
  const limitValidity = (key: string) => (valid: boolean) =>
    setBadLimits((all) => {
      if (valid === !all.has(key)) return all;
      const next = new Set(all);
      if (valid) next.delete(key);
      else next.add(key);
      return next;
    });

  const load = () =>
    Promise.all([api.staff(location), api.catalog(location)])
      .then(([found, catalog]) => {
        setStaff(found);
        setDigits(catalog.minorUnits);
        setRoles(found.roles);
        setTeam(found.team.map((member) => ({ ...member, pin: "" })));
        setChanged(false);
      })
      .catch((failed: unknown) => setStatus({ kind: "error", text: message(failed) }));
  useEffect(() => {
    void load();
  }, [location]);

  if (!staff) return <p className="empty">{status?.text ?? "Loading the staff…"}</p>;
  const touch = () => {
    setChanged(true);
    setStatus(null);
  };
  const edit = (): StaffEdit => ({
    base: staff.version,
    roles,
    team: team.map((member) => {
      const row: StaffEdit["team"][number] = { id: member.id, name: member.name };
      if (member.role) row.role = member.role;
      if (member.pin) row.pin = member.pin;
      return row;
    }),
  });
  const badPin = team.some((member) => member.pin !== "" && !/^\d{4,8}$/.test(member.pin));
  const blocked = badPin || badLimits.size > 0;
  const run = async (publish: boolean) => {
    setBusy(true);
    try {
      if (publish) {
        const { version } = await api.publishStaff(location, edit());
        await load();
        setStatus({ kind: "ok", text: `Published version ${version.slice(0, 12)}. Registers take it within a minute.` });
      } else {
        await api.checkStaff(location, edit());
        setStatus({ kind: "ok", text: "The staff pass every check." });
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
  const updateRole = (index: number, change: (role: Role) => Role) => {
    setRoles((all) => all.map((role, at) => (at === index ? change(role) : role)));
    touch();
  };
  const updateMember = (index: number, change: (member: MemberRow) => MemberRow) => {
    setTeam((all) => all.map((member, at) => (at === index ? change(member) : member)));
    touch();
  };

  return (
    <div className="editor">
      <div className="toolbar card">
        <div>
          <strong>Staff</strong>{" "}
          <span className="muted" title={staff.version}>
            version {staff.version.slice(0, 12)}
          </span>
          {changed && <span className="badge">Unpublished changes</span>}
        </div>
        <div className="actions">
          <button disabled={busy || !changed} onClick={() => void load().then(() => setStatus(null))}>
            Discard
          </button>
          <button disabled={busy || blocked} onClick={() => void run(false)}>
            Check
          </button>
          <button className="primary" disabled={busy || !changed || blocked} onClick={() => void run(true)}>
            Publish
          </button>
        </div>
      </div>
      {badPin && <p className="error banner">A PIN is 4 to 8 digits.</p>}
      {badLimits.size > 0 && <p className="error banner">A limit isn't an amount: correct it, or leave it blank for none.</p>}
      {status && (
        <p role="status" className={status.kind === "error" ? "error banner" : "ok banner"}>
          {status.text}
        </p>
      )}

      <section className="card">
        <div className="heading">
          <h2>Team</h2>
          <button
            onClick={() => {
              const fresh: MemberRow = { id: newId(), name: "New member", hasPin: false, pin: "" };
              const first = roles[0]?.id;
              if (first) fresh.role = first;
              setTeam((all) => [...all, fresh]);
              touch();
            }}
          >
            Add member
          </button>
        </div>
        <table aria-label="Team">
          <thead>
            <tr>
              <th>Name</th>
              <th>Role</th>
              <th>PIN</th>
              <th />
            </tr>
          </thead>
          <tbody>
            {team.map((member, index) => (
              <tr key={member.id}>
                <td>
                  <input
                    aria-label="Member name"
                    value={member.name}
                    onChange={(event) => updateMember(index, (m) => ({ ...m, name: event.target.value }))}
                  />
                </td>
                <td>
                  {roles.length > 0 ? (
                    <select
                      aria-label={`Role of ${member.name}`}
                      value={member.role ?? ""}
                      onChange={(event) => updateMember(index, (m) => ({ ...m, role: event.target.value }))}
                    >
                      {/* A member with no role shows it: found in review, they seemed to have the first. */}
                      <option value="" disabled>
                        Choose a role
                      </option>
                      {roles.map((role) => (
                        <option key={role.id} value={role.id}>
                          {role.name}
                        </option>
                      ))}
                    </select>
                  ) : (
                    <span className="muted">Everything</span>
                  )}
                </td>
                <td>
                  {roles.length > 0 && (
                    <input
                      className="count"
                      type="password"
                      inputMode="numeric"
                      autoComplete="new-password"
                      aria-label={`New PIN for ${member.name}`}
                      placeholder={member.hasPin ? "unchanged" : "set one"}
                      value={member.pin}
                      onChange={(event) =>
                        updateMember(index, (m) => ({ ...m, pin: event.target.value.replace(/\D/g, "").slice(0, 8) }))
                      }
                    />
                  )}
                </td>
                <td>
                  <button
                    className="quiet"
                    aria-label={`Remove ${member.name}`}
                    onClick={() => {
                      setTeam((all) => all.filter((_, at) => at !== index));
                      touch();
                    }}
                  >
                    ×
                  </button>
                </td>
              </tr>
            ))}
          </tbody>
        </table>
      </section>

      <section className="card">
        <div className="heading">
          <h2>Roles</h2>
          <button
            onClick={() => {
              const role = { id: newId(), name: "New role", permissions: [] };
              // The first role is everyone's, to begin with: with roles, each member has one.
              if (roles.length === 0) setTeam((all) => all.map((member) => ({ ...member, role: role.id })));
              setRoles((all) => [...all, role]);
              touch();
            }}
          >
            Add role
          </button>
        </div>
        {roles.length === 0 && (
          <p className="muted">No roles: every member may do everything, and signs in with no PIN.</p>
        )}
        {roles.map((role, index) => (
          <article className="item" key={role.id} aria-label={`Role ${role.name}`}>
            <div className="row">
              <input
                aria-label="Role name"
                className="name"
                value={role.name}
                onChange={(event) => updateRole(index, (r) => ({ ...r, name: event.target.value }))}
              />
              <button
                className="quiet"
                disabled={team.some((member) => member.role === role.id)}
                title="A role with members in it can't be removed"
                onClick={() => {
                  setRoles((all) => all.filter((_, at) => at !== index));
                  touch();
                }}
              >
                Remove
              </button>
            </div>
            <table className="permissions" aria-label={`What ${role.name} may do`}>
              <tbody>
                {staff.actions.map((action) => {
                  const permission = role.permissions.find((p) => p.action === action.name);
                  const set = (next: Permission | null) =>
                    updateRole(index, (r) => ({
                      ...r,
                      permissions: next
                        ? [...r.permissions.filter((p) => p.action !== action.name), next]
                        : r.permissions.filter((p) => p.action !== action.name),
                    }));
                  return (
                    <tr key={action.name}>
                      <td>
                        <label className="inline check">
                          <input
                            type="checkbox"
                            checked={permission !== undefined}
                            onChange={(event) => set(event.target.checked ? { action: action.name } : null)}
                          />
                          {WORDS[action.name] ?? action.name}
                        </label>
                      </td>
                      <td>
                        {permission && action.hasAmount && (
                          <LimitInput
                            onValidity={limitValidity(`${role.id}:${action.name}:amount`)}
                            label={`Most amount for ${action.name} in ${role.name}`}
                            value={permission.mostAmount}
                            toText={(minor) => format(minor, digits)}
                            fromText={(text) => parse(text, digits)}
                            onChange={(mostAmount) => set(withLimit(permission, "mostAmount", mostAmount))}
                          />
                        )}
                      </td>
                      <td>
                        {permission && action.hasPercent && (
                          <LimitInput
                            onValidity={limitValidity(`${role.id}:${action.name}:percent`)}
                            label={`Most percent for ${action.name} in ${role.name}`}
                            value={permission.mostPercent}
                            toText={(bp) => format(bp, 2)}
                            fromText={(text) => parse(text, 2)}
                            suffix="%"
                            onChange={(mostPercent) => set(withLimit(permission, "mostPercent", mostPercent))}
                          />
                        )}
                      </td>
                    </tr>
                  );
                })}
              </tbody>
            </table>
            <p className="muted">Beyond what a role permits, a member asks a manager whose role permits it.</p>
          </article>
        ))}
      </section>
    </div>
  );
}

/** `permission` with `key` set to `value`, or with no such limit when `value` is undefined. */
function withLimit(permission: Permission, key: "mostAmount" | "mostPercent", value: number | undefined): Permission {
  const next: Permission = { ...permission };
  if (value === undefined) delete next[key];
  else next[key] = value;
  return next;
}

/**
 * A limit: blank for none, else a decimal amount or percentage. The text follows the value when it
 * changes otherwise, as when the staff are loaded again. Found in review: after Discard it showed
 * what had been typed.
 */
function LimitInput({
  label,
  value,
  toText,
  fromText,
  suffix,
  onChange,
  onValidity,
}: {
  label: string;
  value: number | undefined;
  toText: (n: number) => string;
  fromText: (text: string) => number | null;
  suffix?: string;
  onChange: (n: number | undefined) => void;
  onValidity: (valid: boolean) => void;
}) {
  const [text, setText] = useState(value === undefined ? "" : toText(value));
  const read = (typed: string) => (typed.trim() === "" ? undefined : fromText(typed));
  useEffect(() => {
    setText((typed) => (read(typed) === value ? typed : value === undefined ? "" : toText(value)));
  }, [value]);
  const parsed = read(text);
  const valid = parsed !== null && (parsed === undefined || parsed >= 0);
  useEffect(() => {
    onValidity(valid);
    return () => onValidity(true);
  }, [valid]);
  return (
    <span className="inline">
      up to{" "}
      <input
        className={valid ? "price" : "price invalid"}
        aria-label={label}
        aria-invalid={!valid}
        placeholder="no limit"
        value={text}
        onChange={(event) => {
          setText(event.target.value);
          const next = event.target.value.trim() === "" ? undefined : fromText(event.target.value);
          if (next !== null && (next === undefined || next >= 0)) onChange(next);
        }}
      />
      {suffix}
    </span>
  );
}

function message(failed: unknown): string {
  return failed instanceof Error ? failed.message : String(failed);
}
