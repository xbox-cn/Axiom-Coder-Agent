import "@testing-library/jest-dom/vitest";
import { afterEach } from "vitest";
import { cleanup } from "@testing-library/react";

afterEach(() => cleanup());

if (!globalThis.crypto?.randomUUID) {
  Object.defineProperty(globalThis, "crypto", {
    value: { randomUUID: () => `test-${Math.random().toString(16).slice(2)}` },
    configurable: true,
  });
}

Object.defineProperty(HTMLElement.prototype, "scrollHeight", {
  configurable: true,
  get: () => 1000,
});
