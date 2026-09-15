import { defineConfig, devices } from '@playwright/test';

const PORT = 8099;

export default defineConfig({
  testDir: './tests',
  // A room is shared server state, so specs that stage talks would tread on
  // each other. One worker keeps them honest and the suite is seconds either
  // way.
  workers: 1,
  fullyParallel: false,
  forbidOnly: Boolean(process.env.CI),
  retries: process.env.CI ? 1 : 0,
  reporter: process.env.CI ? [['list'], ['html', { open: 'never' }]] : [['list']],
  timeout: 30_000,
  use: {
    baseURL: `http://127.0.0.1:${PORT}`,
    trace: 'retain-on-failure',
  },
  projects: [{ name: 'chromium', use: { ...devices['Desktop Chrome'] } }],
  webServer: {
    // The suite starts a room per spec, which is what the per address hourly
    // cap is there to stop. Raised here rather than worked around.
    command: `cargo run --release -- --port ${PORT} --create-per-hour 1000`,
    cwd: '..',
    url: `http://127.0.0.1:${PORT}/healthz`,
    reuseExistingServer: !process.env.CI,
    // A cold release build is minutes, and CI pays it every run.
    timeout: 600_000,
    stdout: 'ignore',
    stderr: 'pipe',
  },
});
