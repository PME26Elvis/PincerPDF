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
  await expect(page.getByTestId("merge-workspace")).toBeVisible();
});

test("exposes one available Merge tool and keeps seven independent gates", async ({ page }) => {
  const tools = page.locator('[data-testid^="tool-nav-"]');
  await expect(tools).toHaveCount(8);
  await expect(page.locator(".tool-state.is-ready")).toHaveText("Available");
  await expect(page.locator(".tool-state:not(.is-ready)")).toHaveCount(7);
  await expect(page.locator(".tool-state:not(.is-ready)")).toHaveText(
    Array(7).fill("Not implemented"),
  );
  await expect(page.getByTestId("add-merge-sources")).toBeEnabled();
  await expect(page.getByTestId("merge-engine-status")).toContainText(
    "Browser verification mode",
  );
});

test("changes to a gated workspace without implying parity", async ({ page }) => {
  await page.getByTestId("tool-nav-rotate").click();
  await expect(page.getByTestId("selected-tool-title")).toHaveText("Rotate PDF");
  await expect(page.getByTestId("selected-tool-description")).toContainText(
    "deterministic page rotations",
  );
  await expect(page.getByTestId("selected-tool-status")).toHaveText("Not implemented");
  await expect(page.getByTestId("open-files")).toBeDisabled();
  await expect(page.getByTestId("tool-nav-rotate")).toHaveAttribute("aria-current", "page");
});

test("builds an ordered Merge plan and validates page selections", async ({ page }) => {
  await expect(page.getByTestId("merge-empty-state")).toBeVisible();
  await page.getByTestId("add-merge-sources").click();
  await expect(page.getByTestId("merge-source-row")).toHaveCount(2);
  await expect(page.getByTestId("merge-source-count")).toHaveText("2");
  await expect(page.getByTestId("merge-page-total")).toHaveText("9");
  await expect(page.getByTestId("run-merge")).toBeDisabled();

  const selections = page.getByTestId("merge-page-selection");
  await selections.first().fill("9");
  await expect(page.getByRole("alert")).toContainText("outside");
  await expect(page.getByTestId("merge-page-total")).toHaveText("—");

  await selections.first().fill("3,1-2");
  await expect(page.getByRole("alert")).toHaveCount(0);
  await expect(page.getByTestId("merge-page-total")).toHaveText("6");

  await page.getByTestId("duplicate-merge-source").nth(1).click();
  await expect(page.getByTestId("merge-source-row")).toHaveCount(3);
  await expect(page.getByTestId("merge-page-total")).toHaveText("9");

  await page.getByTestId("choose-merge-output").click();
  await expect(page.getByTestId("merge-output-path")).toHaveText("merged-document.pdf");
  await expect(page.getByTestId("run-merge")).toBeEnabled();
});

test("reports deterministic running and completed Merge states", async ({ page }) => {
  await page.getByTestId("add-merge-sources").click();
  await page.getByTestId("choose-merge-output").click();
  await page.getByTestId("run-merge").click();

  await expect(page.getByTestId("merge-task-status")).toContainText(
    "Verifying and merging",
  );
  await expect(page.getByTestId("run-merge")).toBeDisabled();
  await expect(page.getByTestId("merge-result-summary")).toContainText("9 pages");
  await expect(page.getByTestId("merge-result-summary")).toContainText("2 sources");
  await expect(page.getByTestId("merge-result-summary")).toContainText("0 bookmarks");
  await expect(page.getByTestId("merge-result-summary")).toContainText(
    "merged-document.pdf",
  );
});

test("reveals explicit advanced safety policies", async ({ page }) => {
  const toggle = page.getByTestId("merge-advanced-toggle");
  await expect(toggle).toHaveAttribute("aria-expanded", "false");
  await toggle.click();
  await expect(toggle).toHaveAttribute("aria-expanded", "true");
  await expect(page.getByTestId("merge-advanced-panel")).toContainText(
    "Discard and report",
  );
  const discardBookmarks = page.getByTestId("bookmark-policy-discard");
  const oneEntryBookmarks = page.getByTestId("bookmark-policy-one-entry");
  await expect(discardBookmarks).toBeChecked();
  await expect(oneEntryBookmarks).not.toBeChecked();
  await oneEntryBookmarks.check();
  await expect(oneEntryBookmarks).toBeChecked();
  await expect(page.getByTestId("merge-advanced-panel")).toContainText(
    "One entry per document",
  );
  await expect(page.getByTestId("merge-advanced-panel")).toContainText(
    "Reject before processing",
  );
  await expect(page.getByTestId("merge-advanced-panel")).toContainText("Stop safely");
  const replaceExisting = page.getByTestId("replace-existing-output");
  await expect(replaceExisting).not.toBeChecked();
  await replaceExisting.check();
  await expect(replaceExisting).toBeChecked();
  await expect(page.getByTestId("merge-advanced-panel")).toContainText(
    "Atomic replacement",
  );
  await expect(page.getByTestId("merge-output-safety")).toContainText(
    "only after the temporary PDF passes verification",
  );
});

test("reports the deterministic one-entry-per-document bookmark policy", async ({ page }) => {
  await page.getByTestId("add-merge-sources").click();
  await page.getByTestId("choose-merge-output").click();
  await page.getByTestId("merge-advanced-toggle").click();
  await page.getByTestId("bookmark-policy-one-entry").check();
  await page.getByTestId("run-merge").click();

  await expect(page.getByTestId("merge-result-summary")).toContainText("2 bookmarks");
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

test("captures empty, configured, completed and compact Merge checkpoints", async ({
  page,
}) => {
  await mkdir(screenshotDir, { recursive: true });
  await page.setViewportSize({ width: 1440, height: 900 });
  await page.screenshot({
    path: `${screenshotDir}/merge-empty-desktop.png`,
    fullPage: true,
    animations: "disabled",
  });

  await page.getByTestId("add-merge-sources").click();
  await page.getByTestId("choose-merge-output").click();
  await page.getByTestId("merge-advanced-toggle").click();
  await page.screenshot({
    path: `${screenshotDir}/merge-configured-desktop.png`,
    fullPage: true,
    animations: "disabled",
  });

  await page.getByTestId("bookmark-policy-one-entry").check();
  await page.screenshot({
    path: `${screenshotDir}/merge-bookmark-policy-desktop.png`,
    fullPage: true,
    animations: "disabled",
  });

  await page.getByTestId("run-merge").click();
  await expect(page.getByTestId("merge-result-summary")).toContainText("9 pages");
  await expect(page.getByTestId("merge-result-summary")).toContainText("2 bookmarks");
  await page.screenshot({
    path: `${screenshotDir}/merge-completed-desktop.png`,
    fullPage: true,
    animations: "disabled",
  });

  await page.setViewportSize({ width: 390, height: 844 });
  await page.screenshot({
    path: `${screenshotDir}/merge-completed-compact.png`,
    fullPage: true,
    animations: "disabled",
  });
});
