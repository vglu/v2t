import { expect, test } from "@playwright/test";

test("home shows app heading and queue workspace", async ({ page }) => {
  await page.goto("/");
  await expect(
    page.getByRole("heading", { level: 1, name: "Video to Text" }),
  ).toBeVisible();
  await expect(page.getByRole("heading", { name: "Queue" })).toBeVisible();
  await expect(page.getByTestId("drop-zone")).toBeVisible();
  await expect(page.getByTestId("workbench-context")).toBeVisible();
  await expect(page.getByTestId("stop-queue")).toBeDisabled();
});

test("queue: add URL and start (browser shows error without Tauri)", async ({
  page,
}) => {
  await page.goto("/");
  await page.getByTestId("url-input").fill("https://youtube.com/watch?v=demo");
  await page.getByTestId("add-urls").click();
  await expect(page.getByTestId("queue-row")).toHaveCount(1);
  await page.getByTestId("start-queue").click();
  await expect(page.locator('[data-testid^="job-status-"]')).toHaveText(
    "error",
    { timeout: 15_000 },
  );
});

test("preferences sheet opens panel", async ({ page }) => {
  await page.goto("/");
  await page.getByTestId("open-preferences").click();
  await expect(page.getByTestId("preferences-sheet")).toBeVisible();
  await expect(
    page.getByRole("heading", { name: "Preferences" }),
  ).toBeVisible();
  await expect(
    page.getByRole("button", { name: "Save settings" }),
  ).toBeVisible();
});

test("preferences engine depth shows API key storage hint for cloud", async ({
  page,
}) => {
  await page.goto("/");
  await page.getByTestId("open-preferences").click();
  await page.getByTestId("prefs-depth-engine").click();
  await expect(
    page.getByText(/API key is saved in the OS credential store/i),
  ).toBeVisible();
});
