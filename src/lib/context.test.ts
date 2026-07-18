import { describe, expect, it } from "vitest";
import { provider, runRecord, usage } from "../test/fixtures";
import { resolveContextLimit } from "./context";

describe("resolveContextLimit", () => {
  it("供应商模型配置优先于历史 Run Usage", () => {
    const configured = provider();
    configured.models[0].contextWindowTokens = 1_000_000;
    const run = runRecord();
    run.usage.contextLimit = 128_000;
    expect(resolveContextLimit([configured], configured.id, configured.models[0].modelId, run)).toBe(1_000_000);
  });

  it("模型未配置上限时使用 Run Usage", () => {
    const configured = provider();
    configured.models[0].contextWindowTokens = undefined;
    const run = runRecord("run", "completed", undefined, { ...usage(), contextLimit: 256_000 });
    expect(resolveContextLimit([configured], configured.id, configured.models[0].modelId, run)).toBe(256_000);
  });

  it("模型和 Run 都无有效上限时回退 128000", () => {
    const configured = provider();
    configured.models[0].contextWindowTokens = undefined;
    expect(resolveContextLimit([configured], configured.id, configured.models[0].modelId)).toBe(128_000);
  });
});
