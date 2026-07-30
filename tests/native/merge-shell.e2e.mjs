import assert from "node:assert/strict";
import { mkdir, writeFile } from "node:fs/promises";
import { resolve } from "node:path";

const artifactRoot = resolve(
  process.env.PINCERPDF_NATIVE_E2E_ARTIFACT_DIR ??
    ".artifacts/native-webview",
);

describe("PincerPDF native WebView2 Merge shell", () => {
  before(async () => {
    await mkdir(artifactRoot, { recursive: true });
    let mounted = false;
    for (let attempt = 0; attempt < 100; attempt += 1) {
      mounted = await browser.execute(
        () => document.querySelector('[data-testid="app-shell"]') !== null,
      );
      if (mounted) {
        break;
      }
      await browser.pause(100);
    }

    assert.equal(mounted, true, "Leptos did not mount within 10 seconds");
    const runtime = await browser.execute(() => ({
      bodyText: document.body.innerText,
      readyState: document.readyState,
      resources: performance
        .getEntriesByType("resource")
        .map(({ initiatorType, name }) => ({ initiatorType, name })),
      tauriGlobal: typeof window.__TAURI__,
      title: document.title,
      url: window.location.href,
      wasmBindings: typeof window.wasmBindings,
    }));
    const startup = {
      title: runtime.title,
      url: runtime.url,
      windowHandles: await browser.getWindowHandles(),
      runtime,
    };
    try {
      startup.browserLogs = await browser.getLogs("browser");
    } catch (error) {
      startup.browserLogError = String(error);
    }
    await Promise.all([
      writeFile(
        resolve(artifactRoot, "native-startup.json"),
        `${JSON.stringify(startup, null, 2)}\n`,
        "utf8",
      ),
      writeFile(
        resolve(artifactRoot, "native-startup.html"),
        await browser.getPageSource(),
        "utf8",
      ),
      browser.saveScreenshot(resolve(artifactRoot, "native-startup.png")),
    ]);
  });

  it("crosses the real Tauri bridge and discovers QPDF", async () => {
    const result = await browser.executeAsync((done) => {
      window.__TAURI__.core
        .invoke("merge_engine_status")
        .then((value) => done({ ok: true, value }))
        .catch((error) => done({ ok: false, error }));
    });

    assert.equal(result.ok, true);
    assert.equal(result.value.ready, true);
    assert.equal(result.value.engineId, "qpdf-process");
    assert.match(result.value.engineVersion, /11\.3\.0/);
    const engineStatus = await browser.execute(
      () =>
        document.querySelector('[data-testid="merge-engine-status"]')
          ?.textContent ?? "",
    );
    assert.match(engineStatus, /11\.3\.0/);
  });

  it("keeps seven tool gates independent from available Merge", async () => {
    const navigation = await browser.executeAsync((done) => {
      const buttons = [
        ...document.querySelectorAll('[data-testid^="tool-nav-"]'),
      ];
      const labels = buttons.map((button) => button.textContent ?? "");
      document
        .querySelector('[data-testid="tool-nav-rotate"]')
        ?.dispatchEvent(new MouseEvent("click", { bubbles: true }));

      requestAnimationFrame(() => {
        const rotated = {
          status:
            document.querySelector('[data-testid="selected-tool-status"]')
              ?.textContent ?? "",
          title:
            document.querySelector('[data-testid="selected-tool-title"]')
              ?.textContent ?? "",
        };
        document
          .querySelector('[data-testid="tool-nav-merge"]')
          ?.dispatchEvent(new MouseEvent("click", { bubbles: true }));
        requestAnimationFrame(() =>
          done({
            labels,
            mergeWorkspace:
              document.querySelector('[data-testid="merge-workspace"]') !==
              null,
            rotated,
          }),
        );
      });
    });

    assert.equal(navigation.labels.length, 8);
    assert.match(navigation.labels[0], /Merge[\s\S]*Available/);
    for (const label of navigation.labels.slice(1)) {
      assert.match(label, /Not implemented/);
    }
    assert.equal(navigation.rotated.title.trim(), "Rotate PDF");
    assert.match(navigation.rotated.status, /Not implemented/i);
    assert.equal(navigation.mergeWorkspace, true);
  });

  it("exposes safe empty-state controls and manual reduced motion", async () => {
    const state = await browser.executeAsync((done) => {
      const runMerge = document.querySelector('[data-testid="run-merge"]');
      window.__TAURI__.core
        .invoke("cancel_merge", { operationId: "native-e2e-missing" })
        .then((value) => {
          document
            .querySelector('[data-testid="motion-toggle"]')
            ?.dispatchEvent(new MouseEvent("click", { bubbles: true }));
          requestAnimationFrame(() =>
            done({
              cancellation: { ok: true, value },
              motionClass:
                document.querySelector('[data-testid="app-shell"]')
                  ?.className ?? "",
              motionMode:
                document.querySelector('[data-testid="motion-mode"]')
                  ?.textContent ?? "",
              runDisabled:
                runMerge instanceof HTMLButtonElement && runMerge.disabled,
            }),
          );
        })
        .catch((error) => done({ ok: false, error }));
    });
    assert.deepEqual(state.cancellation, { ok: true, value: false });
    assert.equal(state.runDisabled, true);
    assert.equal(state.motionMode.trim(), "Manual mode on");
    assert.match(state.motionClass, /motion-reduced/);

    await browser.saveScreenshot(
      resolve(artifactRoot, "native-merge-empty-windows.png"),
    );
  });
});
