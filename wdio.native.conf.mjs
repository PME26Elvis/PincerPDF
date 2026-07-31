import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const repositoryRoot = dirname(fileURLToPath(import.meta.url));
const targetRoot = resolve(
  repositoryRoot,
  process.env.CARGO_TARGET_DIR ?? "target",
);
const binaryName =
  process.platform === "win32" ? "pincerpdf-desktop.exe" : "pincerpdf-desktop";
const appBinaryPath = resolve(targetRoot, "release", binaryName);
const artifactRoot = resolve(
  repositoryRoot,
  process.env.PINCERPDF_NATIVE_E2E_ARTIFACT_DIR ??
    ".artifacts/native-webview",
);

export const config = {
  runner: "local",
  specs: ["./tests/native/**/*.e2e.mjs"],
  maxInstances: 1,
  maxInstancesPerCapability: 1,
  services: [
    [
      "@wdio/tauri-service",
      {
        appBinaryPath,
        driverProvider: "official",
        autoInstallTauriDriver: true,
        autoDownloadEdgeDriver: true,
        captureBackendLogs: false,
        captureFrontendLogs: false,
        startTimeout: 120_000,
        commandTimeout: 30_000,
        logLevel: "warn",
        logDir: resolve(artifactRoot, "service-logs"),
      },
    ],
  ],
  capabilities: [
    {
      browserName: "tauri",
      "tauri:options": {
        application: appBinaryPath,
      },
    },
  ],
  logLevel: "warn",
  outputDir: resolve(artifactRoot, "runner"),
  bail: 0,
  waitforTimeout: 15_000,
  connectionRetryTimeout: 120_000,
  connectionRetryCount: 1,
  framework: "mocha",
  reporters: ["spec"],
  mochaOpts: {
    ui: "bdd",
    timeout: 120_000,
  },
};
