import { expect, test } from "@playwright/test";

test("home shows app heading and queue workspace", async ({ page }) => {
  await page.goto("/");
  await expect(
    page.getByRole("heading", { level: 1, name: "Video to Text" }),
  ).toBeVisible();
  await expect(
    page.getByRole("heading", { name: "Transcribe media" }),
  ).toBeVisible();
  await page.getByRole("tab", { name: "Files & folders" }).click();
  await expect(page.getByTestId("drop-zone")).toBeVisible();
  await expect(page.getByTestId("stop-queue")).toHaveCount(0);
});

test("queue: add URL and start (browser shows error without Tauri)", async ({
  page,
}) => {
  await page.goto("/");
  await page.getByTestId("url-input").fill("https://youtube.com/watch?v=demo");
  await page.getByTestId("batch-language-select").selectOption("uk");
  await page.getByTestId("add-urls").click();
  await expect(page.getByTestId("queue-row")).toHaveCount(1);
  await expect(page.locator(".queue-language-select")).toHaveValue("uk");
  await expect(page.getByTestId("start-queue")).toBeDisabled();
});

test("preferences sheet opens panel", async ({ page }) => {
  await page.goto("/");
  await page.getByTestId("open-preferences-header").click();
  await expect(page.getByTestId("preferences-sheet")).toBeVisible();
  await expect(
    page.getByRole("heading", { name: "Preferences" }),
  ).toBeVisible();
  await expect(
    page.getByRole("button", { name: "Save changes" }),
  ).toBeVisible();
});

test("preferences engine depth shows API key storage hint for cloud", async ({
  page,
}) => {
  await page.goto("/");
  await page.getByTestId("open-preferences-header").click();
  await page.getByTestId("prefs-depth-engine").click();
  await expect(
    page.getByText(/API key is saved in the OS credential store/i),
  ).toBeVisible();
});

test("preferences keep save state visible and protect unsaved changes", async ({
  page,
}) => {
  await page.goto("/");
  await page.getByTestId("open-preferences-header").click();
  await page.getByLabel("Filename template").fill("{title}-review");

  await expect(page.getByText("You have unsaved changes")).toBeVisible();
  await expect(
    page.getByRole("button", { name: "Save changes" }),
  ).toBeEnabled();

  await page.getByTestId("preferences-close").click();
  await expect(
    page.getByRole("alertdialog").getByText("Save your changes?"),
  ).toBeVisible();
});
