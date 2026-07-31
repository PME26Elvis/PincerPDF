import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { execFile, spawn } from "node:child_process";
import {
  mkdir,
  readFile,
  rm,
  writeFile,
} from "node:fs/promises";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { promisify } from "node:util";

const execFileAsync = promisify(execFile);
const repositoryRoot = resolve(
  dirname(fileURLToPath(import.meta.url)),
  "..",
  "..",
);
const enabled =
  process.platform === "win32" &&
  process.env.PINCERPDF_SYSTEM_DIALOG_E2E === "1";
const describeSystemDialog = enabled ? describe : describe.skip;
const fixtureRoot = process.env.PINCERPDF_SYSTEM_DIALOG_FIXTURES;
const outputRoot = process.env.PINCERPDF_SYSTEM_DIALOG_OUTPUT_DIR;

function invokeDialog(action, paths = [], confirmOverwrite = false) {
  const helper = resolve(
    repositoryRoot,
    "scripts",
    "invoke_windows_file_dialog.py",
  );
  const argumentsList = [
    helper,
    "--action",
    action,
    "--paths-json",
    JSON.stringify(paths),
  ];
  if (confirmOverwrite) {
    argumentsList.push("--confirm-overwrite");
  }
  const pythonSitePackages = resolve(
    process.env.PINCERPDF_DEV_ROOT ?? "D:\\PincerPDF-dev",
    "python",
    "site-packages",
  );
  const child = spawn("python.exe", argumentsList, {
    env: {
      ...process.env,
      PYTHONPATH: [
        pythonSitePackages,
        process.env.PYTHONPATH,
      ]
        .filter(Boolean)
        .join(";"),
    },
    windowsHide: true,
    stdio: ["ignore", "pipe", "pipe"],
  });
  return new Promise((accept, reject) => {
    let stdout = "";
    let stderr = "";
    child.stdout.on("data", (chunk) => {
      stdout += chunk;
    });
    child.stderr.on("data", (chunk) => {
      stderr += chunk;
    });
    child.once("error", reject);
    child.once("exit", (code) => {
      if (code === 0) {
        const jsonLine = stdout.trim().split(/\r?\n/u).at(-1);
        accept(JSON.parse(jsonLine));
      } else {
        reject(
          new Error(
            `file-dialog helper exited ${code}: ${stderr || stdout}`,
          ),
        );
      }
    });
  });
}

async function click(testId) {
  const clicked = await browser.execute((id) => {
    const target = document.querySelector(`[data-testid="${id}"]`);
    if (!target) {
      return false;
    }
    target.dispatchEvent(new MouseEvent("click", { bubbles: true }));
    return true;
  }, testId);
  assert.equal(clicked, true, `control ${testId} must exist before clicking`);
}

async function waitForState(readState, predicate, description) {
  let latestState;
  for (let attempt = 0; attempt < 200; attempt += 1) {
    latestState = await browser.execute(readState);
    if (predicate(latestState)) {
      return latestState;
    }
    await browser.pause(100);
  }
  throw new Error(
    `Timed out waiting for ${description}; latest=${JSON.stringify(latestState)}`,
  );
}

function sha256(bytes) {
  return createHash("sha256").update(bytes).digest("hex");
}

describeSystemDialog("PincerPDF Windows system-dialog Merge acceptance", () => {
  before(async () => {
    await waitForState(
      () => ({
        engine:
          document.querySelector('[data-testid="merge-engine-status"]')
            ?.textContent ?? "",
        mounted:
          document.querySelector('[data-testid="merge-workspace"]') !== null,
      }),
      (state) => state.mounted && state.engine.includes("11.3.0"),
      "mounted native Merge workspace and QPDF discovery",
    );
  });

  it("cancels safely, selects real files, verifies output, and recovers from conflict", async () => {
    assert.ok(fixtureRoot, "PINCERPDF_SYSTEM_DIALOG_FIXTURES is required");
    assert.ok(outputRoot, "PINCERPDF_SYSTEM_DIALOG_OUTPUT_DIR is required");
    await mkdir(outputRoot, { recursive: true });
    const plain = resolve(fixtureRoot, "plain-three-pages.pdf");
    const bookmarks = resolve(fixtureRoot, "bookmarks.pdf");
    const output = resolve(outputRoot, "merged-from-system-dialog.pdf");
    await rm(output, { force: true });

    const cancelDialog = invokeDialog("Cancel");
    await click("add-merge-sources");
    const cancelEvidence = await cancelDialog;
    console.log("[system-dialog] source picker cancellation invoked");
    assert.equal(cancelEvidence.action, "Cancel");
    const cancelledState = await waitForState(
      () => {
        const addButton = document.querySelector(
          '[data-testid="add-merge-sources"]',
        );
        return {
          addDisabled:
            !(addButton instanceof HTMLButtonElement) || addButton.disabled,
          count: document.querySelectorAll(
            '[data-testid="merge-source-row"]',
          ).length,
        };
      },
      (state) => state.count === 0 && !state.addDisabled,
      "cancelled source picker to preserve and restore the empty state",
    );
    assert.equal(cancelledState.count, 0);

    const openDialog = invokeDialog("Open", [plain, bookmarks]);
    await click("add-merge-sources");
    const openEvidence = await openDialog;
    console.log("[system-dialog] source selection command posted");
    assert.deepEqual(openEvidence.paths, [plain, bookmarks]);
    await waitForState(
      () => ({
        addDisabled:
          document.querySelector('[data-testid="add-merge-sources"]')
            ?.disabled ?? true,
        count: document.querySelectorAll(
          '[data-testid="merge-source-row"]',
        ).length,
        pages:
          document.querySelector('[data-testid="merge-page-total"]')
            ?.textContent ?? "",
        status:
          document.querySelector('[data-testid="merge-task-status"]')
            ?.textContent ?? "",
      }),
      (state) =>
        state.count === 2 &&
        state.pages.trim() === "6" &&
        !state.addDisabled,
      "two inspected source rows",
    );
    console.log("[system-dialog] two source rows inspected and picker restored");

    const saveDialog = invokeDialog("Save", [output]);
    await click("choose-merge-output");
    const saveEvidence = await saveDialog;
    assert.deepEqual(saveEvidence.paths, [output]);
    console.log("[system-dialog] destination selection command posted");
    await waitForState(
      () =>
        document.querySelector('[data-testid="merge-output-path"]')
          ?.textContent ?? "",
      (value) => value.includes("merged-from-system-dialog.pdf"),
      "registered save destination",
    );

    await click("merge-advanced-toggle");
    const bookmarkPolicyEnabled = await browser.execute(() => {
      const oneEntry = document.querySelector(
        '[data-testid="bookmark-policy-one-entry"]',
      );
      if (!(oneEntry instanceof HTMLInputElement)) {
        return false;
      }
      oneEntry.checked = true;
      oneEntry.dispatchEvent(new Event("change", { bubbles: true }));
      return oneEntry.checked;
    });
    assert.equal(bookmarkPolicyEnabled, true);

    await click("run-merge");
    await waitForState(
      () =>
        document.querySelector('[data-testid="merge-task-status"]')
          ?.textContent ?? "",
      (value) => value.includes("Merged PDF created"),
      "verified first merge result",
    );
    console.log("[system-dialog] first six-page merge completed");
    const firstQpdf = await execFileAsync("qpdf.exe", [
      "--show-npages",
      output,
    ]);
    assert.equal(firstQpdf.stdout.trim(), "6");
    const firstOutlineJson = await execFileAsync("qpdf.exe", [
      "--json=2",
      "--json-key=outlines",
      output,
    ]);
    const firstOutlines = JSON.parse(firstOutlineJson.stdout).outlines;
    assert.deepEqual(
      firstOutlines.map(({ title, destpageposfrom1 }) => ({
        title,
        page: destpageposfrom1,
      })),
      [
        { title: "plain-three-pages.pdf", page: 1 },
        { title: "bookmarks.pdf", page: 4 },
      ],
    );
    const firstBytes = await readFile(output);
    assert.equal(firstBytes.subarray(0, 5).toString("ascii"), "%PDF-");

    const sentinel = Buffer.from("pincerpdf-conflict-sentinel", "utf8");
    await writeFile(output, sentinel);
    const conflictDialog = invokeDialog("Save", [output], true);
    await click("choose-merge-output");
    const conflictDialogEvidence = await conflictDialog;
    console.log("[system-dialog] existing destination reselected");
    assert.equal(conflictDialogEvidence.confirmationHandled, true);
    await waitForState(
      () =>
        document.querySelector('[data-testid="merge-output-path"]')
          ?.textContent ?? "",
      (value) => value.includes("merged-from-system-dialog.pdf"),
      "reselected existing destination",
    );

    await click("run-merge");
    const conflictStatus = await waitForState(
      () =>
        document.querySelector('[data-testid="merge-task-status"]')
          ?.textContent ?? "",
      (value) =>
        value.includes("output already exists") ||
        value.includes("replacement is disabled"),
      "safe existing-output conflict",
    );
    assert.match(conflictStatus, /output already exists|replacement is disabled/);
    assert.equal(sha256(await readFile(output)), sha256(sentinel));
    console.log("[system-dialog] default conflict preserved sentinel bytes");

    const replacementEnabled = await browser.execute(() => {
      const checkbox = document.querySelector(
        '[data-testid="replace-existing-output"]',
      );
      if (!(checkbox instanceof HTMLInputElement)) {
        return false;
      }
      checkbox.checked = true;
      checkbox.dispatchEvent(new Event("change", { bubbles: true }));
      return checkbox.checked;
    });
    assert.equal(replacementEnabled, true);

    await click("run-merge");
    await waitForState(
      () =>
        document.querySelector('[data-testid="merge-task-status"]')
          ?.textContent ?? "",
      (value) => value.includes("Merged PDF created"),
      "successful atomic replacement",
    );
    const replacementQpdf = await execFileAsync("qpdf.exe", [
      "--check",
      output,
    ]);
    assert.match(replacementQpdf.stdout, /No syntax or stream encoding errors/);
    const replacementPages = await execFileAsync("qpdf.exe", [
      "--show-npages",
      output,
    ]);
    assert.equal(replacementPages.stdout.trim(), "6");
    const replacementBytes = await readFile(output);
    assert.equal(
      replacementBytes.subarray(0, 5).toString("ascii"),
      "%PDF-",
    );
    assert.notEqual(sha256(replacementBytes), sha256(sentinel));
    console.log("[system-dialog] atomic replacement produced a valid six-page PDF");

    const evidence = {
      cancellation: cancelEvidence,
      conflictDialog: conflictDialogEvidence,
      conflictPreservedSha256: sha256(sentinel),
      firstOutputSha256: sha256(firstBytes),
      firstOutlines: firstOutlines.map(
        ({ title, destpageposfrom1 }) => ({
          title,
          page: destpageposfrom1,
        }),
      ),
      open: openEvidence,
      output,
      replacementQpdfCheck: true,
      replacementOutputSha256: sha256(replacementBytes),
      replacementPages: 6,
      save: saveEvidence,
    };
    await writeFile(
      resolve(outputRoot, "system-dialog-evidence.json"),
      `${JSON.stringify(evidence, null, 2)}\n`,
      "utf8",
    );
    await browser.saveScreenshot(
      resolve(outputRoot, "system-dialog-merge-completed.png"),
    );
  });
});
