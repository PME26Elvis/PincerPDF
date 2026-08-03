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

test("exposes Merge and Split while keeping six independent gates", async ({ page }) => {
  const tools = page.locator('[data-testid^="tool-nav-"]');
  await expect(tools).toHaveCount(8);
  await expect(page.locator(".tool-state.is-ready")).toHaveCount(2);
  await expect(page.locator(".tool-state.is-ready")).toHaveText(["Available", "Available"]);
  await expect(page.locator(".tool-state:not(.is-ready)")).toHaveCount(6);
  await expect(page.locator(".tool-state:not(.is-ready)")).toHaveText(
    Array(6).fill("Not implemented"),
  );
  await expect(page.getByTestId("add-merge-sources")).toBeEnabled();
  await expect(page.getByTestId("merge-engine-status")).toContainText(
    "Browser verification mode",
  );
});

test("runs the deterministic Split workspace across its core rule controls", async ({ page }) => {
  await page.getByTestId("tool-nav-split").click();
  await expect(page.getByTestId("split-workspace")).toBeVisible();
  await page.getByTestId("choose-split-source").click();
  await page.getByTestId("choose-split-output").click();
  await expect(page.getByTestId("run-split")).toBeEnabled();

  await page.getByTestId("split-rule-fixed").check();
  await page.getByTestId("split-fixed-count").fill("2");
  await page.getByTestId("run-split").click();
  await expect(page.getByTestId("split-task-status")).toContainText("Verifying and splitting");
  await expect(page.getByTestId("split-result-summary")).toContainText("6 pages");
  await expect(page.getByTestId("split-result-summary")).toContainText("3 parts");

  await page.getByTestId("split-rule-bookmarks").check();
  await expect(page.getByTestId("split-bookmark-depth")).toHaveValue("0");
  await page.getByTestId("split-bookmark-depth").fill("1");
  await page.getByTestId("run-split").click();
  await expect(page.getByTestId("split-result-summary")).toContainText("2 parts");

  await page.getByTestId("split-rule-size").check();
  await page.getByTestId("split-max-bytes").fill("100000");
  await page.getByTestId("run-split").click();
  await expect(page.getByTestId("split-result-summary")).toContainText("100000");
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
  const retainBookmarks = page.getByTestId("bookmark-policy-retain");
  const retainAsOneEntry = page.getByTestId(
    "bookmark-policy-retain-as-one-entry",
  );
  await expect(discardBookmarks).toBeChecked();
  await expect(oneEntryBookmarks).not.toBeChecked();
  await expect(retainBookmarks).not.toBeChecked();
  await expect(retainAsOneEntry).not.toBeChecked();
  const blankPageIfOdd = page.getByTestId("blank-page-if-odd");
  await expect(blankPageIfOdd).not.toBeChecked();
  await blankPageIfOdd.check();
  await expect(blankPageIfOdd).toBeChecked();
  const filenameFooter = page.getByTestId("filename-footer");
  await expect(filenameFooter).not.toBeChecked();
  await filenameFooter.check();
  await expect(filenameFooter).toBeChecked();
  const tocNone = page.getByTestId("toc-policy-none");
  const tocFileNames = page.getByTestId("toc-policy-file-names");
  await expect(tocNone).toBeChecked();
  await expect(tocFileNames).not.toBeChecked();
  await tocFileNames.check();
  await expect(tocFileNames).toBeChecked();
  await oneEntryBookmarks.check();
  await expect(blankPageIfOdd).toBeChecked();
  await expect(filenameFooter).toBeChecked();
  await expect(tocFileNames).toBeChecked();
  await expect(oneEntryBookmarks).toBeChecked();
  await expect(page.getByTestId("merge-advanced-panel")).toContainText(
    "One entry per document",
  );
  await retainBookmarks.check();
  await expect(page.getByTestId("merge-advanced-panel")).toContainText(
    "Retain relevant source hierarchy",
  );
  await retainAsOneEntry.check();
  await expect(page.getByTestId("merge-advanced-panel")).toContainText(
    "Retain under one entry per document",
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

test("reports deterministic retained-bookmark policy states", async ({ page }) => {
  await page.getByTestId("add-merge-sources").click();
  await page.getByTestId("choose-merge-output").click();
  await page.getByTestId("merge-advanced-toggle").click();
  await page.getByTestId("bookmark-policy-retain").check();
  await page.getByTestId("run-merge").click();
  await expect(page.getByTestId("merge-result-summary")).toContainText("1 bookmarks");

  await page.getByTestId("bookmark-policy-retain-as-one-entry").check();
  await page.getByTestId("run-merge").click();
  await expect(page.getByTestId("merge-result-summary")).toContainText("2 bookmarks");
});

test("reports the deterministic filename table-of-contents policy", async ({ page }) => {
  await page.getByTestId("add-merge-sources").click();
  await expect(page.getByTestId("merge-source-row")).toHaveCount(2);
  await page.getByTestId("choose-merge-output").click();
  await expect(page.getByTestId("merge-output-path")).toHaveText("merged-document.pdf");
  await page.getByTestId("merge-advanced-toggle").click();
  await expect(page.getByTestId("merge-advanced-panel")).toBeVisible();
  await page.getByTestId("toc-policy-file-names").check();
  await page.getByTestId("run-merge").click();

  await expect(page.getByTestId("merge-result-summary")).toContainText("10 pages");
});

test("reports the deterministic document-title table-of-contents policy", async ({ page }) => {
  await page.getByTestId("add-merge-sources").click();
  await expect(page.getByTestId("merge-source-row")).toHaveCount(2);
  await page.getByTestId("choose-merge-output").click();
  await page.getByTestId("merge-advanced-toggle").click();
  await expect(page.getByTestId("merge-advanced-panel")).toBeVisible();
  await page.getByTestId("toc-policy-document-titles").check();
  await page.getByTestId("run-merge").click();

  await expect(page.getByTestId("merge-result-summary")).toContainText("10 pages");
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
  await page.evaluate(() => window.scrollTo(0, 0));
  await page.screenshot({
    path: `${screenshotDir}/merge-configured-desktop.png`,
    fullPage: true,
    animations: "disabled",
  });

  await page.getByTestId("bookmark-policy-one-entry").check();
  await page.evaluate(() => window.scrollTo(0, 0));
  await page.screenshot({
    path: `${screenshotDir}/merge-bookmark-policy-desktop.png`,
    fullPage: true,
    animations: "disabled",
  });

  await page.getByTestId("bookmark-policy-retain-as-one-entry").check();
  await page.evaluate(() => window.scrollTo(0, 0));
  await page.screenshot({
    path: `${screenshotDir}/merge-retained-bookmark-policy-desktop.png`,
    fullPage: true,
    animations: "disabled",
  });

  await page.getByTestId("run-merge").click();
  await expect(page.getByTestId("merge-result-summary")).toContainText("9 pages");
  await expect(page.getByTestId("merge-result-summary")).toContainText("2 bookmarks");
  await page.evaluate(() => window.scrollTo(0, 0));
  await page.screenshot({
    path: `${screenshotDir}/merge-completed-desktop.png`,
    fullPage: true,
    animations: "disabled",
  });

  await page.setViewportSize({ width: 390, height: 844 });
  await page.evaluate(() => window.scrollTo(0, 0));
  await page.screenshot({
    path: `${screenshotDir}/merge-completed-compact.png`,
    fullPage: true,
    animations: "disabled",
  });
});

test("captures empty, configured, completed and compact Split checkpoints", async ({
  page,
}) => {
  await mkdir(screenshotDir, { recursive: true });
  await page.getByTestId("tool-nav-split").click();
  await page.setViewportSize({ width: 1440, height: 900 });
  await page.screenshot({
    path: `${screenshotDir}/split-empty-desktop.png`,
    fullPage: true,
    animations: "disabled",
  });

  await page.getByTestId("choose-split-source").click();
  await page.getByTestId("choose-split-output").click();
  await page.getByTestId("split-rule-fixed").check();
  await page.getByTestId("split-fixed-count").fill("2");
  await page.evaluate(() => window.scrollTo(0, 0));
  await page.screenshot({
    path: `${screenshotDir}/split-configured-desktop.png`,
    fullPage: true,
    animations: "disabled",
  });

  await page.getByTestId("run-split").click();
  await expect(page.getByTestId("split-result-summary")).toContainText("3 parts");
  await page.evaluate(() => window.scrollTo(0, 0));
  await page.screenshot({
    path: `${screenshotDir}/split-completed-desktop.png`,
    fullPage: true,
    animations: "disabled",
  });

  await page.setViewportSize({ width: 390, height: 844 });
  await page.evaluate(() => window.scrollTo(0, 0));
  await page.screenshot({
    path: `${screenshotDir}/split-completed-compact.png`,
    fullPage: true,
    animations: "disabled",
  });
});
