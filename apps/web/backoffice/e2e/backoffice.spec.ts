import { expect, test, type Page } from "@playwright/test";
import { readFileSync } from "node:fs";
import { join } from "node:path";
import { demoDir } from "../playwright.config";

const token = () => readFileSync(join(demoDir, "token"), "utf8").trim();

async function signIn(page: Page) {
  await page.goto("/");
  await page.getByLabel("Operator token").fill(token());
  await page.getByRole("button", { name: "Sign in" }).click();
  await expect(page.getByRole("tab", { name: "Catalog" })).toBeVisible();
}

/** What the register rings from now, as the demo writes it. */
function registerVersion(): string | null {
  try {
    return readFileSync(join(demoDir, "register-version"), "utf8").trim();
  } catch {
    return null;
  }
}

test("a token that isn't the cell's is refused", async ({ page }) => {
  await page.goto("/");
  await page.getByLabel("Operator token").fill("not-the-token");
  await page.getByRole("button", { name: "Sign in" }).click();
  await expect(page.getByText("That token isn't the cell's.")).toBeVisible();
  // The API refuses it too, and a request without one.
  const refused = await page.request.get("/api/locations", { headers: { Authorization: "Bearer nope" } });
  expect(refused.status()).toBe(401);
  expect((await page.request.get("/api/locations")).status()).toBe(401);
});

test("the devices show the hub connected to the cell, and the register's log held through it", async ({ page }) => {
  await signIn(page);
  await page.getByRole("tab", { name: "Devices" }).click();
  const rows = page.locator("tbody tr");
  await expect(rows).toHaveCount(3);
  await expect(page.getByRole("row", { name: /Back office.*Hub \(priority 1\).*Yes/ })).toBeVisible();
  // The register reaches the hub alone: its sales come to the cell through it.
  const register = page.getByRole("row", { name: /Front counter.*Register.*No/ });
  await expect(register).toBeVisible();
  await expect(register.locator("td.number")).not.toHaveText("0");
  await expect(page.getByRole("row", { name: /Cloud.*Cloud.*Yes/ })).toBeVisible();
});

test("the reports total the sales rung on the register", async ({ page }) => {
  await signIn(page);
  await page.getByRole("tab", { name: "Reports" }).click();
  // The demo's register rings three espressos at $3.25, with New York's 8.875% sales tax: $3.54 each.
  await expect(async () => {
    await page.getByRole("button", { name: "Show" }).click();
    await expect(page.getByTestId("orders")).toHaveText(/^[3-9]$/, { timeout: 1000 });
  }).toPass({ timeout: 30_000 });
  const orders = Number(await page.getByTestId("orders").textContent());
  const items = page.getByRole("table", { name: "Sales by item" });
  await expect(items.getByRole("row", { name: new RegExp(`Espresso ${orders} \\d`) })).toBeVisible();
  const tenders = page.getByRole("table", { name: "Payments by tender" });
  await expect(tenders.getByRole("row", { name: new RegExp(`Cash ${orders} `) })).toBeVisible();
  // A range that ends before it begins is refused.
  await page.getByLabel("From").fill("2026-12-02");
  await page.getByLabel("To").fill("2026-12-01");
  await page.getByRole("button", { name: "Show" }).click();
  await expect(page.getByText("the range ends before it begins")).toBeVisible();
});

test("a price changed in the editor is checked, published, and taken by the register", async ({ page }) => {
  await signIn(page);
  const before = registerVersion();
  expect(before).toMatch(/^[0-9a-f]{64}$/);
  const espresso = page.getByRole("article", { name: "Item Espresso" });
  const price = espresso.getByLabel("Price of Espresso");
  await expect(price).toHaveValue("3.25");
  // A price that isn't one isn't taken.
  await price.fill("3.2.5");
  await expect(price).toHaveAttribute("aria-invalid", "true");
  await price.fill("3.40");
  await expect(page.getByText("Unpublished changes")).toBeVisible();
  // A group that allows fewer than it requires fails the kernel's check, and says why.
  const group = page.getByRole("article", { name: /^Group / }).first();
  const fewest = group.getByLabel("Fewest");
  const original = await fewest.inputValue();
  await group.getByLabel("Most").fill("1");
  await fewest.fill("2");
  await page.getByRole("button", { name: "Check" }).click();
  await expect(page.getByRole("status")).toHaveClass(/error/);
  await fewest.fill(original);
  await page.getByRole("button", { name: "Check" }).click();
  await expect(page.getByRole("status")).toHaveText("The catalog passes every check.");
  await page.getByRole("button", { name: "Publish" }).click();
  await expect(page.getByRole("status")).toHaveText(/^Published version [0-9a-f]{12}\./);
  await expect(price).toHaveValue("3.40");
  const published = (await page.getByRole("status").textContent())?.match(/version ([0-9a-f]{12})/)?.[1];
  expect(published).toBeTruthy();
  // The register takes it within the 60 s budget.
  await expect
    .poll(() => registerVersion()?.slice(0, 12), { timeout: 60_000, intervals: [250] })
    .toBe(published);
  expect(registerVersion()).not.toBe(before);
});

test("a catalog read before another was published is refused", async ({ page, request }) => {
  await signIn(page);
  const headers = { Authorization: `Bearer ${token()}` };
  const [location] = await (await request.get("/api/locations", { headers })).json();
  const read = await (await request.get(`/api/locations/${location.id}/catalog`, { headers })).json();
  const edit = { base: "0".repeat(64), items: read.items, groups: read.groups, pages: read.pages };
  const stale = await request.put(`/api/locations/${location.id}/catalog`, { headers, data: edit });
  expect(stale.status()).toBe(409);
  expect((await stale.json()).version).toBe(read.version);
  // An identifier that isn't one is refused before the kernel is asked.
  const broken = { items: [{ ...read.items[0], id: "not-an-id" }], groups: read.groups, pages: read.pages };
  const refused = await request.post(`/api/locations/${location.id}/catalog/check`, { headers, data: broken });
  expect(refused.status()).toBe(422);
  expect((await refused.json()).error).toContain("isn't an identifier");
});
