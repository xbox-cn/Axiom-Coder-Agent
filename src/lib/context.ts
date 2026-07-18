import type { ProviderProfile, RunRecord } from "./types";

const FALLBACK_CONTEXT_LIMIT = 128_000;

export function resolveContextLimit(
  providers: ProviderProfile[],
  providerId: string,
  modelId: string,
  latestRun?: RunRecord,
): number {
  const configured = providers
    .find((provider) => provider.id === providerId)
    ?.models.find((model) => model.modelId === modelId)
    ?.contextWindowTokens;

  if (configured != null && configured > 0) return configured;
  if (latestRun?.usage.contextLimit && latestRun.usage.contextLimit > 0) return latestRun.usage.contextLimit;
  return FALLBACK_CONTEXT_LIMIT;
}
