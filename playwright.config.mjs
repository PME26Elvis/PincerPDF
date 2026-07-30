import { defineConfig, devices } from "@playwright/test";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const chromiumExecutable =
  process.env.PLAYWRIGHT_CHROMIUM_EXECUTABLE_PATH ?? "/usr/bin/chromium";
const repositoryRoot = dirname(fileURLToPath(import.meta.url));
const artifactRoot = resolve(
  repositoryRoot,
  process.env.PINCERPDF_ARTIFACT_DIR ?? ".artifacts/application-shell",
);
const uiRoot = resolve(repositoryRoot, "apps/pincerpdf-ui");
const serveDist = resolve(artifactRoot, "serve-dist");

export default defineConfig({
  testDir: "./tests/e2e",
  outputDir: resolve(artifactRoot, "test-results"),
  fullyParallel: false,
  forbidOnly: Boolean(process.env.CI),
  retries: process.env.CI ? 1 : 0,
  workers: 1,
  reporter: [
    ["line"],
    ["html", { outputFolder: resolve(artifactRoot, "playwright-report"), open: "never" }],
  ],
  use: {
    baseURL: "http://127.0.0.1:1420",
    trace: "retain-on-failure",
    screenshot: "only-on-failure",
    video: "off",
  },
  webServer: {
    command: `trunk serve --address 127.0.0.1 --port 1420 --dist "${serveDist}"`,
    cwd: uiRoot,
    url: "http://127.0.0.1:1420",
    reuseExistingServer: !process.env.CI,
    timeout: 180_000,
  },
  projects: [
    {
      name: "chromium",
      use: {
        ...devices["Desktop Chrome"],
        launchOptions: {
          executablePath: chromiumExecutable,
          args: ["--no-sandbox", "--disable-dev-shm-usage"],
        },
      },
    },
  ],
});
