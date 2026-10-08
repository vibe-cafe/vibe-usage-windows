// Durable per-request usage can survive even when the UI token_count is missing.
// Convert both indexing and accounting passes identically; keep the source marker
// so the following UI mirror is not counted again when its cumulative total lags.
export function normalizeUsageRecord(obj) {
  if (obj?.type !== 'token_usage_record') return obj;
  const p = obj.payload;
  const usage = p?.usage;
  const total = p?.thread_token_usage;
  if (!usage || !total || !Number.isFinite(total.total_tokens) || total.total_tokens <= 0) return obj;
  for (const value of [usage.input_tokens, usage.output_tokens, usage.cached_input_tokens ?? 0, usage.reasoning_output_tokens ?? 0]) {
    if (!Number.isFinite(value) || value < 0) return obj;
  }
  return { ...obj, type: 'event_msg', payload: {
    type: 'token_count', usage_record: true,
    info: { last_token_usage: pickUsage(usage), total_token_usage: pickUsage(total) },
  }};
}

function pickUsage(value) {
  const keys = ['input_tokens', 'output_tokens', 'cached_input_tokens', 'cache_read_input_tokens',
    'cache_write_input_tokens', 'reasoning_output_tokens', 'total_tokens'];
  return Object.fromEntries(keys.filter(key => Number.isFinite(value[key])).map(key => [key, value[key]]));
}
