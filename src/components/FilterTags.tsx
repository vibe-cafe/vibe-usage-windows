// Time range pills + 4 filter dropdowns — port of Views/FilterTagsView.swift.

import { useEffect, useMemo, useRef, useState } from "react";
import { Check, ChevronDown, Cpu, Folder, Monitor, SquareTerminal } from "lucide-react";
import { useAppState } from "../state/AppStateContext";
import { FilterState, filtersAreEmpty, TIME_RANGE_ORDER, timeRangeLabel } from "../lib/types";
import { groupModelsByFamily } from "../lib/modelFamilies";
import { localDayKey } from "../lib/formatters";

type Dimension = "hostname" | "source" | "model" | "project";

const DIMENSIONS: { key: Dimension; label: string; Icon: typeof Monitor }[] = [
  { key: "hostname", label: "终端", Icon: Monitor },
  { key: "source", label: "工具", Icon: SquareTerminal },
  { key: "model", label: "模型", Icon: Cpu },
  { key: "project", label: "项目", Icon: Folder },
];

const FILTER_ROW_HEIGHT = 28;
const DROPDOWN_WIDTH = 240;
const DROPDOWN_MAX_HEIGHT = 260;

export function FilterTags() {
  const state = useAppState();
  const [openFilter, setOpenFilter] = useState<Dimension | null>(null);
  const containerRef = useRef<HTMLDivElement>(null);

  const unique = useMemo(() => {
    const s = new Set<string>();
    const m = new Set<string>();
    const p = new Set<string>();
    const h = new Set<string>();
    for (const b of state.buckets) {
      s.add(b.source);
      m.add(b.model);
      p.add(b.project);
      h.add(b.hostname);
    }
    const sort = (x: Set<string>) => [...x].sort();
    return { sources: sort(s), models: sort(m), projects: sort(p), hostnames: sort(h) };
  }, [state.buckets]);

  // Close dropdown when clicking outside the filter grid.
  useEffect(() => {
    if (!openFilter) return;
    const onDown = (e: MouseEvent) => {
      if (containerRef.current && !containerRef.current.contains(e.target as Node)) {
        setOpenFilter(null);
      }
    };
    window.addEventListener("mousedown", onDown);
    return () => window.removeEventListener("mousedown", onDown);
  }, [openFilter]);

  const valuesFor = (d: Dimension): string[] => {
    switch (d) {
      case "hostname":
        return unique.hostnames;
      case "source":
        return unique.sources;
      case "model":
        return unique.models;
      case "project":
        return unique.projects;
    }
  };

  const selectedFor = (d: Dimension): Set<string> => {
    switch (d) {
      case "hostname":
        return state.filters.hostnames;
      case "source":
        return state.filters.sources;
      case "model":
        return state.filters.models;
      case "project":
        return state.filters.projects;
    }
  };

  const setSelected = (d: Dimension, next: Set<string>) => {
    const f: FilterState = {
      sources: new Set(state.filters.sources),
      models: new Set(state.filters.models),
      projects: new Set(state.filters.projects),
      hostnames: new Set(state.filters.hostnames),
    };
    if (d === "hostname") f.hostnames = next;
    if (d === "source") f.sources = next;
    if (d === "model") f.models = next;
    if (d === "project") f.projects = next;
    state.setFilters(f);
  };

  const toggleValue = (d: Dimension, value: string) => {
    const next = new Set(selectedFor(d));
    if (next.has(value)) next.delete(value);
    else next.add(value);
    setSelected(d, next);
  };

  return (
    <div className="flex flex-col gap-2">
      <div className="flex items-center gap-2">
        <TimeRangeSelector />
        <div className="grow" />
        {!filtersAreEmpty(state.filters) && (
          <button
            className="h-7 px-2 text-xs font-medium text-danger"
            title="清除筛选"
            onClick={() =>
              state.setFilters({
                sources: new Set(),
                models: new Set(),
                projects: new Set(),
                hostnames: new Set(),
              })
            }
          >
            清除
          </button>
        )}
      </div>

      {state.timeRange === "custom" && <CustomRangeControls />}

      {/* Filter grid + anchored dropdown */}
      <div ref={containerRef} className="relative" style={{ height: FILTER_ROW_HEIGHT }}>
        <div className="flex gap-2">
          {DIMENSIONS.map((dim) => {
            const values = valuesFor(dim.key);
            const selected = selectedFor(dim.key);
            const enabled = values.length > 0;
            const isOpen = openFilter === dim.key;
            const isActive = selected.size > 0;
            const highlight = isActive || isOpen;
            return (
              <button
                key={dim.key}
                disabled={!enabled}
                onClick={() => setOpenFilter(isOpen ? null : dim.key)}
                className="flex min-w-0 flex-1 items-center gap-1.5 rounded-full border px-[9px]"
                style={{
                  height: FILTER_ROW_HEIGHT,
                  background: highlight ? "#292929" : "#171717",
                  borderColor: highlight ? "#424242" : "#262626",
                  opacity: enabled ? 1 : 0.45,
                }}
              >
                <dim.Icon
                  size={11}
                  strokeWidth={2.25}
                  className="shrink-0"
                  color={highlight ? "#FFFFFF" : "#949494"}
                />
                <span
                  className="shrink-0 text-xs font-medium"
                  style={{ color: highlight ? "#FFFFFF" : "#A8A8A8" }}
                >
                  {dim.label}
                </span>
                <span
                  className="min-w-0 truncate text-xs"
                  style={{ color: isActive ? "#DBDBDB" : "#6B6B6B" }}
                >
                  {selected.size === 0 ? "全部" : `${selected.size} 项`}
                </span>
                <div className="grow" />
                <ChevronDown
                  size={8}
                  strokeWidth={3}
                  color="#616161"
                  className="shrink-0 transition-transform duration-150"
                  style={{ transform: isOpen ? "rotate(180deg)" : undefined }}
                />
              </button>
            );
          })}
        </div>

        {openFilter && (
          <DropdownPanel
            dimension={openFilter}
            values={valuesFor(openFilter)}
            selected={selectedFor(openFilter)}
            onToggle={(v) => toggleValue(openFilter, v)}
            onSetSelected={(s) => setSelected(openFilter, s)}
          />
        )}
      </div>
    </div>
  );
}

function TimeRangeSelector() {
  const state = useAppState();
  return (
    <div className="flex rounded-full bg-filter-row p-0.5" style={{ gap: 1 }}>
      {TIME_RANGE_ORDER.map((range) => {
        const isActive = state.timeRange === range;
        return (
          <button
            key={range}
            disabled={state.isLoadingData}
            onClick={() => {
              if (state.isLoadingData || state.timeRange === range) return;
              state.setTimeRange(range);
            }}
            className="flex h-6 items-center rounded-full px-[9px] text-xs"
            style={{
              background: isActive ? "rgba(255,255,255,0.20)" : "transparent",
              color: isActive ? "#FFFFFF" : "#8A8A8A",
              fontWeight: isActive ? 600 : 400,
            }}
          >
            {timeRangeLabel(range)}
          </button>
        );
      })}
    </div>
  );
}

function CustomRangeControls() {
  const state = useAppState();

  const parse = (v: string): Date | null => {
    const m = /^(\d{4})-(\d{2})-(\d{2})$/.exec(v);
    if (!m) return null;
    return new Date(parseInt(m[1], 10), parseInt(m[2], 10) - 1, parseInt(m[3], 10));
  };

  const inputStyle: React.CSSProperties = {
    background: "#232323",
    border: "1px solid #333333",
    borderRadius: 4,
    color: "#E0E0E0",
    fontSize: 12,
    padding: "2px 6px",
    colorScheme: "dark",
  };

  return (
    <div
      className="flex items-center gap-2 rounded-card border px-2.5 py-[7px]"
      style={{ background: "#141414", borderColor: "#242424" }}
    >
      <input
        type="date"
        style={inputStyle}
        value={localDayKey(state.customRangeFrom)}
        onChange={(e) => {
          const d = parse(e.target.value);
          if (d) state.setCustomRangeFrom(d);
        }}
      />
      <span className="text-xs text-t-tertiary">–</span>
      <input
        type="date"
        style={inputStyle}
        value={localDayKey(state.customRangeTo)}
        onChange={(e) => {
          const d = parse(e.target.value);
          if (d) state.setCustomRangeTo(d);
        }}
      />
      <div className="grow" />
      <button
        className="flex h-6 items-center rounded-full bg-white px-2.5 text-xs font-semibold text-black"
        disabled={state.isLoadingData}
        style={{ opacity: state.isLoadingData ? 0.55 : 1 }}
        onClick={() => void state.fetchUsageData()}
      >
        应用
      </button>
    </div>
  );
}

function DropdownPanel({
  dimension,
  values,
  selected,
  onToggle,
  onSetSelected,
}: {
  dimension: Dimension;
  values: string[];
  selected: Set<string>;
  onToggle: (value: string) => void;
  onSetSelected: (next: Set<string>) => void;
}) {
  // Center the 240px panel under the dimension's button, clamped to bounds.
  const index = DIMENSIONS.findIndex((d) => d.key === dimension);
  const count = DIMENSIONS.length;
  // Content width = 520 - 2*16 padding = 488; gap 8.
  const available = 488;
  const gap = 8;
  const buttonWidth = (available - gap * (count - 1)) / count;
  const buttonCenter = index * (buttonWidth + gap) + buttonWidth / 2;
  const left = Math.min(Math.max(0, buttonCenter - DROPDOWN_WIDTH / 2), available - DROPDOWN_WIDTH);

  return (
    <div
      className="no-scrollbar absolute z-30 overflow-y-auto rounded-card border py-1"
      style={{
        top: FILTER_ROW_HEIGHT + 6,
        left,
        width: DROPDOWN_WIDTH,
        maxHeight: DROPDOWN_MAX_HEIGHT,
        background: "#111214",
        borderColor: "#2E2E2E",
        boxShadow: "0 8px 12px rgba(0,0,0,0.35)",
      }}
    >
      {dimension === "model" ? (
        <ModelOptions models={values} selected={selected} onToggle={onToggle} onSetSelected={onSetSelected} />
      ) : (
        <div className="flex flex-col gap-0.5">
          {values.map((value) => (
            <OptionRow
              key={value}
              title={value === "" ? "未知" : value}
              isSelected={selected.has(value)}
              onClick={() => onToggle(value)}
            />
          ))}
        </div>
      )}
    </div>
  );
}

function ModelOptions({
  models,
  selected,
  onToggle,
  onSetSelected,
}: {
  models: string[];
  selected: Set<string>;
  onToggle: (value: string) => void;
  onSetSelected: (next: Set<string>) => void;
}) {
  const [expanded, setExpanded] = useState<Set<string>>(new Set());
  const groups = groupModelsByFamily(models);

  return (
    <div className="flex flex-col">
      {groups.map((group) => {
        const familyKey = group.family?.key ?? "other";
        const familyLabel = group.family?.label ?? "其他";
        const familyModels = new Set(group.models);
        const selectedInFamily = [...familyModels].filter((m) => selected.has(m));
        const allSelected = familyModels.size > 0 && selectedInFamily.length === familyModels.size;
        const someSelected = selectedInFamily.length > 0 && !allSelected;
        const isExpanded = expanded.has(familyKey);

        return (
          <div key={familyKey} className="flex flex-col">
            <div className="flex items-center">
              <button
                className="min-w-0 grow"
                onClick={() => {
                  const next = new Set(selected);
                  if (allSelected) {
                    for (const m of familyModels) next.delete(m);
                  } else {
                    for (const m of familyModels) next.add(m);
                  }
                  onSetSelected(next);
                }}
              >
                <CheckRowContent title={familyLabel} isSelected={allSelected} isMixed={someSelected} />
              </button>
              <button
                className="flex h-7 w-7 shrink-0 items-center justify-center"
                onClick={() => {
                  const next = new Set(expanded);
                  if (isExpanded) next.delete(familyKey);
                  else next.add(familyKey);
                  setExpanded(next);
                }}
              >
                <ChevronDown
                  size={9}
                  strokeWidth={3}
                  color="#616161"
                  className="transition-transform duration-150"
                  style={{ transform: isExpanded ? "rotate(180deg)" : undefined }}
                />
              </button>
            </div>
            {isExpanded &&
              group.models.map((value) => (
                <OptionRow
                  key={value}
                  title={value === "" ? "未知" : value}
                  isSelected={selected.has(value)}
                  indent={19}
                  onClick={() => onToggle(value)}
                />
              ))}
          </div>
        );
      })}
    </div>
  );
}

function OptionRow({
  title,
  isSelected,
  indent = 0,
  onClick,
}: {
  title: string;
  isSelected: boolean;
  indent?: number;
  onClick: () => void;
}) {
  return (
    <button className="w-full" onClick={onClick}>
      <CheckRowContent title={title} isSelected={isSelected} indent={indent} />
    </button>
  );
}

function CheckRowContent({
  title,
  isSelected,
  isMixed = false,
  indent = 0,
}: {
  title: string;
  isSelected: boolean;
  isMixed?: boolean;
  indent?: number;
}) {
  const on = isSelected || isMixed;
  return (
    <div
      className="flex h-7 w-full items-center gap-[7px] px-2.5"
      style={{ background: on ? "#1A1A1A" : "transparent", paddingLeft: 10 + indent }}
    >
      <span
        className="flex h-[13px] w-[13px] shrink-0 items-center justify-center rounded-[3px]"
        style={{
          background: on ? "#FFFFFF" : "transparent",
          border: on ? "none" : "1px solid #616161",
        }}
      >
        {isSelected && <Check size={9} strokeWidth={3.5} color="#000000" />}
        {!isSelected && isMixed && <span className="h-[1.5px] w-[7px] bg-black" />}
      </span>
      <span
        className="min-w-0 truncate text-xs"
        style={{ color: on ? "#FFFFFF" : "#9E9E9E" }}
        title={title}
      >
        {title}
      </span>
    </div>
  );
}
