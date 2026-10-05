// End-to-end tests of the explorer against `ontosys serve` on the test fixtures.
// Build first: cargo build --release   ·   run: cd tests/ui && npx playwright test
import { defineConfig } from '@playwright/test';

export default defineConfig({
  testDir: '.',
  testMatch: /.*\.spec\.mjs/,
  timeout: 45_000,
  retries: 0,
  workers: 1,
  use: { baseURL: 'http://localhost:3901', viewport: { width: 1400, height: 900 } },
  webServer: [{
    command: '../../target/release/ontosys serve --path ../fixtures/current.nt --compare ../fixtures/baseline.nt --port 3901',
    url: 'http://localhost:3901/api/info',
    reuseExistingServer: !process.env.CI,
    timeout: 60_000,
  }, {
    // generic graph (generic.spec.mjs): forced onto the class map, as any large ungrounded graph is
    command: '../../target/release/ontosys serve --path ../fixtures/generic.nt --overview-above 0 --port 3902',
    url: 'http://localhost:3902/api/info',
    reuseExistingServer: !process.env.CI,
    timeout: 60_000,
  }],
});
