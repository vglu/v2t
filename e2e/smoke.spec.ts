import { expect, test } from "@playwright/test";

test("home shows app heading and queue workspace", async ({ page }) => {
  await page.goto("/");
  await expect(
    page.getByRole("heading", { level: 1, name: "Video to Text" }),
  ).toBeVisible();
  await expect(page.getByRole("heading", { name: "Queue" })).toBeVisible();
  await expect(page.getByTestId("drop-zone")).toBeVisible();
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

test("settings tab opens panel", async ({ page }) => {
  await page.goto("/");
  await page.getByRole("tab", { name: "Settings" }).click();
  await expect(page.getByRole("heading", { name: "Settings" })).toBeVisible();
  await expect(
    page.getByRole("button", { name: "Save settings" }),
  ).toBeVisible();
});

test("settings panel shows API key storage hint", async ({ page }) => {
  await page.goto("/");
  await page.getByRole("tab", { name: "Settings" }).click();
  await expect(
    page.getByText(/API key is saved in the OS credential store/i),
  ).toBeVisible();
});
