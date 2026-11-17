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
  // The dashboard opens first; most tests are of the catalog.
  await expect(page.getByLabel("The day's figures")).toBeVisible();
  await page.getByRole("tab", { name: "Catalog" }).click();
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
  // Typed key by key, as an operator types it, it stays as typed. Found in review: each key
  // reformatted it, so "3.4" became "3.40" under the cursor and the next key made "3.400".
  await price.clear();
  await price.pressSequentially("3.4");
  await expect(price).toHaveValue("3.4");
  await price.pressSequentially("0");
  await expect(price).toHaveValue("3.40");
  await expect(page.getByText("Unpublished changes")).toBeVisible();
  // A price that isn't one holds back checking and publishing: what would go isn't what it shows.
  await price.pressSequentially(".5");
  await expect(price).toHaveAttribute("aria-invalid", "true");
  await expect(page.getByRole("button", { name: "Publish" })).toBeDisabled();
  await expect(page.getByRole("button", { name: "Check" })).toBeDisabled();
  await price.press("Backspace");
  await price.press("Backspace");
  await expect(price).toHaveValue("3.40");
  await expect(page.getByRole("button", { name: "Publish" })).toBeEnabled();
  // A group that allows fewer than it requires fails the kernel's check, and says why.
  const group = page.getByRole("article", { name: /^Group / }).first();
  const fewest = group.getByLabel("Fewest");
  const most = group.getByLabel("Most");
  const [original, originalMost] = [await fewest.inputValue(), await most.inputValue()];
  await most.fill(String(Math.max(Number(original) - 1, 0)));
  await fewest.fill(String(Number(original) + 1));
  await page.getByRole("button", { name: "Check" }).click();
  await expect(page.getByRole("status")).toHaveClass(/error/);
  await most.fill(originalMost);
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

test("a variant removed takes its buttons with it", async ({ page }) => {
  await signIn(page);
  const latte = page.getByRole("article", { name: "Item Latte" });
  const pages = page.getByRole("article", { name: /^Page / });
  await expect(pages.getByRole("listitem").filter({ hasText: "Latte, Large" })).not.toHaveCount(0);
  await latte.getByRole("button", { name: "Remove variant" }).last().click();
  await expect(pages.getByRole("listitem").filter({ hasText: "Latte, Large" })).toHaveCount(0);
  await expect(pages.getByText("(removed)")).toHaveCount(0);
  await page.getByRole("button", { name: "Check" }).click();
  await expect(page.getByRole("status")).toHaveText("The catalog passes every check.");
  await page.getByRole("button", { name: "Discard" }).click();
  await expect(latte.getByLabel("Variant name")).toHaveCount(2);
});

test("a catalog read before another was published is refused", async ({ page, request }) => {
  await signIn(page);
  const headers = { Authorization: `Bearer ${token()}` };
  const [location] = await (await request.get("/api/locations", { headers })).json();
  const read = await (await request.get(`/api/locations/${location.id}/catalog`, { headers })).json();
  const edit = { base: "0".repeat(64), items: read.items, groups: read.groups, pages: read.pages };
  // With no base at all, it is refused too: it would overwrite what it never read.
  const blind = await request.put(`/api/locations/${location.id}/catalog`, {
    headers,
    data: { items: read.items, groups: read.groups, pages: read.pages },
  });
  expect(blind.status()).toBe(422);
  const stale = await request.put(`/api/locations/${location.id}/catalog`, { headers, data: edit });
  expect(stale.status()).toBe(409);
  expect((await stale.json()).version).toBe(read.version);
  // An identifier that isn't one is refused before the kernel is asked.
  const broken = { items: [{ ...read.items[0], id: "not-an-id" }], groups: read.groups, pages: read.pages };
  const refused = await request.post(`/api/locations/${location.id}/catalog/check`, { headers, data: broken });
  expect(refused.status()).toBe(422);
  expect((await refused.json()).error).toContain("isn't an identifier");
});

test("a cashier's role and PIN are changed, published, and taken by the register", async ({ page, request }) => {
  await signIn(page);
  await page.getByRole("tab", { name: "Staff" }).click();
  const team = page.getByRole("table", { name: "Team" });
  await expect(team.getByLabel("Member name")).toHaveCount(2);
  await expect(page.getByLabel("Role of Sam")).toHaveValue(/0192f0c1-0000-7000-8000-000000000602/);
  // A PIN is never read back: the API says only that one is set.
  const headers = { Authorization: `Bearer ${token()}` };
  const [location] = await (await request.get("/api/locations", { headers })).json();
  const staff = await (await request.get(`/api/locations/${location.id}/staff`, { headers })).json();
  expect(staff.team.map((member: { hasPin: boolean }) => member.hasPin)).toEqual([true, true]);
  expect(JSON.stringify(staff)).not.toMatch(/salt|hash|"pin"/i);
  // A cashier may now drop an order, and Sam has a new PIN.
  const cashier = page.getByRole("article", { name: "Role Cashier" });
  await cashier.getByLabel("Drop an order with items on it").check();
  const pin = page.getByLabel("New PIN for Sam");
  await pin.fill("12");
  await expect(page.getByText("A PIN is 4 to 8 digits.")).toBeVisible();
  await expect(page.getByRole("button", { name: "Publish" })).toBeDisabled();
  await pin.fill("1357");
  const before = registerVersion();
  await page.getByRole("button", { name: "Check" }).click();
  await expect(page.getByRole("status")).toHaveText("The staff pass every check.");
  await page.getByRole("button", { name: "Publish" }).click();
  await expect(page.getByRole("status")).toHaveText(/^Published version [0-9a-f]{12}\./);
  await expect(cashier.getByLabel("Drop an order with items on it")).toBeChecked();
  await expect(page.getByLabel("New PIN for Sam")).toHaveValue("");
  await expect
    .poll(() => registerVersion(), { timeout: 60_000, intervals: [250] })
    .not.toBe(before);
  // A limit that isn't one holds back publishing; Discard puts back what was published.
  // Found in review: after Discard, the field showed what had been typed.
  const limit = page.getByLabel("Most amount for discount.apply in Cashier");
  await expect(limit).toHaveValue("5.00");
  await limit.fill("abc");
  await expect(page.getByRole("button", { name: "Publish" })).toBeDisabled();
  await limit.fill("7.00");
  await expect(page.getByRole("button", { name: "Publish" })).toBeEnabled();
  await page.getByRole("button", { name: "Discard" }).click();
  await expect(limit).toHaveValue("5.00");
  // A member new to the team needs a PIN: the kernel says so.
  await page.getByRole("button", { name: "Add member" }).click();
  await page.getByRole("button", { name: "Check" }).click();
  await expect(page.getByRole("status")).toContainText("a team member with no PIN");
});

test("the dashboard shows the day the register rang, by hour and by item", async ({ page }) => {
  await signIn(page);
  await page.getByRole("tab", { name: "Dashboard" }).click();
  const figures = page.getByLabel("The day's figures");
  await expect(figures.getByText("Sales, with tax")).toBeVisible();
  await expect(figures.locator(".stat").filter({ hasText: "Orders" }).locator("strong")).not.toHaveText("0");
  await expect(page.getByLabel("Sales by hour").locator(".hour")).not.toHaveCount(0);
  await expect(page.getByLabel("Top items").getByRole("row")).not.toHaveCount(0);
  await expect(page.getByLabel("Look at").getByText("Front counter")).toBeVisible();
});

test("settings are checked by the kernel and refused in its words", async ({ page }) => {
  await signIn(page);
  await page.getByRole("tab", { name: "Settings" }).click();
  const name = page.getByLabel("Name", { exact: true });
  await expect(name).not.toHaveValue("");
  await name.fill("Corner Café");
  await expect(page.getByText("Unpublished changes")).toBeVisible();
  await page.getByRole("button", { name: "Check" }).click();
  await expect(page.getByRole("status")).toHaveText("The settings pass every check.");
  await page.getByLabel("Time zone").fill("Mars/Olympus");
  await page.getByRole("button", { name: "Check" }).click();
  await expect(page.getByRole("status")).toHaveClass(/error/);
  await expect(page.getByRole("status")).toContainText("time zone");
  await page.getByRole("button", { name: "Discard" }).click();
  await expect(name).not.toHaveValue("Corner Café");
  // Tip percentages are typed as they are said, commas and decimals and all: found in review,
  // each key was parsed and undone.
  const tips = page.getByLabel("Tip percentages");
  if ((await tips.count()) === 0) await page.getByLabel("Ask for tips").check();
  await tips.fill("");
  await tips.pressSequentially("12.5, 18, 22");
  await expect(tips).toHaveValue("12.5, 18, 22");
  await page.getByRole("button", { name: "Check" }).click();
  await expect(page.getByRole("status")).toHaveText("The settings pass every check.");
  await tips.fill("18, 12");
  await page.getByRole("button", { name: "Check" }).click();
  await expect(page.getByRole("status")).toContainText("order");
  await tips.fill("15, abc");
  await expect(page.getByRole("button", { name: "Check" })).toBeDisabled();
  await page.getByRole("button", { name: "Discard" }).click();
});

test("at phone width the tabs scroll and the dashboard doesn't overflow the page", async ({ page }) => {
  await page.setViewportSize({ width: 360, height: 740 });
  await signIn(page);
  await page.getByRole("tab", { name: "Dashboard" }).click();
  await expect(page.getByLabel("The day's figures")).toBeVisible();
  const overflow = await page.evaluate(() => document.documentElement.scrollWidth - window.innerWidth);
  expect(overflow).toBeLessThanOrEqual(0);
  await page.getByRole("tab", { name: "Devices" }).click();
  await expect(page.getByRole("tab", { name: "Devices" })).toHaveAttribute("aria-selected", "true");
});

test("the export downloads everything the location wrote, as JSON Lines", async ({ page }) => {
  await signIn(page);
  const download = page.waitForEvent("download");
  await page.getByRole("button", { name: "Export" }).click();
  const file = await download;
  expect(file.suggestedFilename()).toMatch(/^keel-.*\.jsonl$/);
  const text = readFileSync(await file.path(), "utf8");
  const lines = text.trim().split("\n").map((line) => JSON.parse(line) as Record<string, unknown>);
  expect(lines[0]).toMatchObject({ type: "manifest", format: "keel.export", version: 1 });
  expect(lines[1]).toMatchObject({ type: "profile" });
  // No PIN, nor anything of one: a member says only whether they have one.
  const team = ((lines[1]?.["staff"] as { team?: Record<string, unknown>[] } | undefined)?.team ?? []);
  expect(team.length).toBeGreaterThan(0);
  for (const member of team) expect(Object.keys(member).sort()).toEqual(["hasPin", "id", "name", "role"]);
  expect(JSON.stringify(lines[1])).not.toMatch(/"(salt|pinHash|pin)"/);
  expect(lines.some((line) => (line["stream"] as { kind?: string } | undefined)?.kind === "profile")).toBe(false);
  expect(lines.length - 2).toBe(lines[0]?.["events"]);
  // The register's sales are in it, their payments named.
  const captured = lines.find((line) => (line["schema"] as { name?: string } | undefined)?.name === "payment.captured");
  expect((captured?.["payload"] as Record<string, unknown> | undefined)?.["amount"]).toBeTruthy();
});
