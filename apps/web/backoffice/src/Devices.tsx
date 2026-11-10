import { useCallback, useEffect, useState } from "react";
import type { Api, Device } from "./api";

/** The location's devices: who is connected, and how far the durable copy holds each log. */
export function Devices({ api, location }: { api: Api; location: string }) {
  const [devices, setDevices] = useState<Device[] | null>(null);
  const [error, setError] = useState<string | null>(null);
  const load = useCallback(() => {
    api
      .replication(location)
      .then((found) => {
        setDevices(found.devices);
        setError(null);
      })
      .catch((failed: unknown) => setError(failed instanceof Error ? failed.message : String(failed)));
  }, [api, location]);
  useEffect(() => {
    load();
    const every = setInterval(load, 5000);
    return () => clearInterval(every);
  }, [load]);
  return (
    <section className="card">
      <h2>Devices</h2>
      {error && <p className="error">{error}</p>}
      {devices && (
        <table>
          <thead>
            <tr>
              <th>Device</th>
              <th>Role</th>
              <th>Connected to the cell</th>
              <th className="number">Events held</th>
            </tr>
          </thead>
          <tbody>
            {devices.map((device) => (
              <tr key={device.id}>
                <td>{device.name}</td>
                <td>
                  {device.role === "hub" ? `Hub (priority ${device.hubPriority ?? ""})` : device.role === "cloud" ? "Cloud" : "Register"}
                </td>
                <td>
                  <span className={device.connected ? "dot on" : "dot"} aria-hidden="true" />
                  {device.connected ? "Yes" : "No"}
                </td>
                <td className="number">{device.held}</td>
              </tr>
            ))}
          </tbody>
        </table>
      )}
    </section>
  );
}
