import { useAppState } from "../state/AppStateContext";
import { emptyFilters, filtersAreEmpty } from "../lib/types";

export function UsageCoverage() {
  const state = useAppState();
  const hosts = new Set(state.buckets.map(b => b.hostname));
  const selected = state.filters.hostnames;
  return (
    <div className="text-[11px] text-t-tertiary">
      <div title={[...(selected.size ? selected : hosts)].join("、")}>
        统计范围：{selected.size ? `已选 ${selected.size} 个终端` : `全部终端（${hosts.size} 台有数据）`}
        {!filtersAreEmpty(state.filters) && (
          <button className="ml-2 text-link" onClick={() => state.setFilters(emptyFilters())}>清除全部筛选</button>
        )}
      </div>
      <div>费用为已采集用量的估算值，包含当前范围内各终端；不是订阅账单或剩余额度。</div>
      {state.usageUpdatedAt && <div>云端数据更新于 {new Date(state.usageUpdatedAt).toLocaleString()}</div>}
    </div>
  );
}
