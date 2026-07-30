import { expect, test } from "@playwright/test";
import { mkdir } from "node:fs/promises";
import { resolve } from "node:path";

const screenshotDir = resolve(
  process.env.PINCERPDF_ARTIFACT_DIR ?? ".artifacts/application-shell",
  "screenshots",
);

test.beforeEach(async ({ page }) => {
  await page.goto("/");
  await expect(page.getByTestId("app-shell")).toBeVisible();
});

test("exposes eight gated PDF tools without faking implementation", async ({ page }) => {
  const tools = page.locator('[data-testid^="tool-nav-"]');
  await expect(tools).toHaveCount(8);
  await expect(page.locator(".tool-state")).toHaveCount(8);
  await expect(page.locator(".tool-state")).toHaveText(Array(8).fill("Not implemented"));
  await expect(page.getByTestId("selected-tool-title")).toHaveText("Merge PDF");
  await expect(page.getByTestId("selected-tool-status")).toHaveText("Not implemented");
  await expect(page.getByTestId("open-files")).toBeDisabled();
});

test("changes the selected workspace while keeping its capability gate explicit", async ({
  page,
}) => {
  await page.getByTestId("tool-nav-rotate").click();
  await expect(page.getByTestId("selected-tool-title")).toHaveText("Rotate PDF");
  await expect(page.getByTestId("selected-tool-description")).toContainText(
    "deterministic page rotations",
  );
  await expect(page.getByTestId("selected-tool-status")).toHaveText("Not implemented");
  await expect(page.getByTestId("tool-nav-rotate")).toHaveAttribute("aria-current", "page");
});

test("supports a manual reduced-motion override", async ({ page }) => {
  const shell = page.getByTestId("app-shell");
  const toggle = page.getByTestId("motion-toggle");

  await expect(toggle).toHaveAttribute("aria-pressed", "false");
  await toggle.click();
  await expect(toggle).toHaveAttribute("aria-pressed", "true");
  await expect(shell).toHaveClass(/motion-reduced/);
  await expect(page.getByTestId("motion-mode")).toHaveText("Manual mode on");
});

test("keeps keyboard navigation anchored by a visible skip link", async ({ page }) => {
  await page.keyboard.press("Tab");
  const skipLink = page.getByRole("link", { name: "Skip to main content" });
  await expect(skipLink).toBeFocused();
  await skipLink.press("Enter");
  await expect(page.locator("#main-content")).toBeFocused();
});

test("captures deterministic desktop and compact shell checkpoints", async ({ page }) => {
  await mkdir(screenshotDir, { recursive: true });

  await page.setViewportSize({ width: 1440, height: 900 });
  await page.goto("/");
  await expect(page.getByTestId("hero")).toBeVisible();
  await page.screenshot({
    path: `${screenshotDir}/application-shell-desktop.png`,
    fullPage: true,
    animations: "disabled",
  });

  await page.setViewportSize({ width: 390, height: 844 });
  await page.goto("/");
  await expect(page.getByTestId("sidebar")).toBeVisible();
  await page.screenshot({
    path: `${screenshotDir}/application-shell-compact.png`,
    fullPage: true,
    animations: "disabled",
  });
});
