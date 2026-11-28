// Cards, rows and controls of the Settings window: macOS
// `Views/Settings/SettingsTheme.swift` (classic), drawn here rather than with
// native controls so sizes and colors match the design in every state.

import { useEffect, useId, useRef, useState, type CSSProperties, type ReactNode, type Ref } from "react";
import { Check, ChevronsUpDown, X } from "lucide-react";
import appIcon from "../../src-tauri/icons/128x128@2x.png";
import { T, TYPE } from "./theme";
import type { QuotaStatusKind } from "../lib/quotaProducts";

// MARK: - Page

/** The scrolling column a page sits in: 20 px from the top, 22 px from the
 *  sides, 22 px after the last card (a page may ask for less). The page
 *  scrolls on its own; the sidebar never moves. */
export function PageScroll({
  bottomInset = T.pageBottomInset,
  scrollRef,
  children,
}: {
  bottomInset?: number;
  scrollRef?: Ref<HTMLDivElement>;
  children: ReactNode;
}) {
  return (
    <div ref={scrollRef} className="settings-scroll h-full min-w-0 flex-1 overflow-y-auto">
      <div
        className="flex flex-col"
        style={{
          padding: `${T.pageTopInset}px ${T.pageHorizontalInset}px ${bottomInset}px`,
          maxWidth: T.pageMaxWidth + T.pageHorizontalInset * 2,
        }}
      >
        {children}
      </div>
    </div>
  );
}

/** Title, an optional dim detail on the same baseline, an optional trailing
 *  control. */
export function PageHeader({
  title,
  advanced = false,
  detail,
  trailing,
}: {
  title: string;
  advanced?: boolean;
  detail?: ReactNode;
  trailing?: ReactNode;
}) {
  return (
    <div className="flex items-start gap-2.5" style={{ padding: "0 2px 10px" }}>
      <div className="flex min-w-0 items-baseline gap-2.5" style={{ paddingTop: 1 }}>
        <span className="flex shrink-0 items-center gap-1.5">
          <h1 className="m-0" style={{ ...TYPE.pageTitle, color: T.text, lineHeight: "20px" }}>
            {title}
          </h1>
          {advanced && <Tag kind="advanced" />}
        </span>
        {detail}
      </div>
      <span className="grow" />
      {trailing}
    </div>
  );
}

/** A flat card: no border, radius 10; rows carry their own inset. */
export function Card({ children, style }: { children: ReactNode; style?: CSSProperties }) {
  return (
    <div className="flex flex-col" style={{ background: T.card, borderRadius: 10, ...style }}>
      {children}
    </div>
  );
}

/** A group title above its card (更新, 诊断, 链接, 隐私保护, 隔离运行时目录). */
export function SectionTitle({ title }: { title: string }) {
  return (
    <h2 className="m-0" style={{ ...TYPE.sectionTitle, color: T.secondaryText, padding: "18px 2px 7px" }}>
      {title}
    </h2>
  );
}

export function Section({ title, children }: { title: string; children: ReactNode }) {
  return (
    <section className="flex flex-col">
      <SectionTitle title={title} />
      <Card>{children}</Card>
    </section>
  );
}

/** One card row: 44 px (52 with a caption), 14 px in from the left, 12 from the right. */
export function Row({
  tall = false,
  gap = 10,
  children,
  style,
}: {
  tall?: boolean;
  gap?: number;
  children: ReactNode;
  style?: CSSProperties;
}) {
  return (
    <div
      className="flex items-center"
      style={{
        gap,
        minHeight: tall ? T.tallRowHeight : T.rowHeight,
        paddingLeft: T.rowLeading,
        paddingRight: T.rowTrailing,
        ...style,
      }}
    >
      {children}
    </div>
  );
}

/** The hairline between rows, from the rows' text inset to the card's edge.
 *  Takes no height: drawn over the top pixel of the row below. */
export function Separator({ leading = T.rowLeading }: { leading?: number }) {
  return (
    <div aria-hidden="true" className="relative z-[1] h-0">
      <div className="absolute right-0 top-0 h-px" style={{ left: leading, background: T.line }} />
    </div>
  );
}

export function RowLabel({
  title,
  caption,
  captionColor,
}: {
  title: ReactNode;
  caption?: string | null;
  captionColor?: string;
}) {
  return (
    <span className="flex min-w-0 flex-col" style={{ gap: 1 }}>
      <span style={{ ...TYPE.label, color: T.text }}>{title}</span>
      {caption && (
        <span className="break-words" style={{ ...TYPE.caption, color: captionColor ?? T.secondaryText }}>
          {caption}
        </span>
      )}
    </span>
  );
}

export function Spacer({ min = 8 }: { min?: number }) {
  return <span className="grow" style={{ minWidth: min }} />;
}

// MARK: - Small marks

/** 「高级」 after 数据目录 (grey), 「测试版」 after 导出诊断日志 (amber). */
export function Tag({ kind }: { kind: "advanced" | "testBuild" }) {
  const advanced = kind === "advanced";
  return (
    <span
      className="inline-flex shrink-0 items-center"
      style={{
        ...TYPE.tag,
        height: advanced ? 15 : 16,
        padding: `0 ${advanced ? 4 : 5}px`,
        borderRadius: 4,
        color: advanced ? T.secondaryText : T.amber,
        background: advanced ? T.selection : "rgba(229,164,59,0.14)",
      }}
    >
      {advanced ? "高级" : "测试版"}
    </span>
  );
}

/** 6 px dot: ok / amber / red, or a hollow ring for "nothing found". */
export function StatusDot({ kind, dimmed = false }: { kind: QuotaStatusKind; dimmed?: boolean }) {
  const base: CSSProperties = { display: "inline-block", width: 6, height: 6, borderRadius: 3, flexShrink: 0 };
  if (kind === "none") {
    return <span aria-hidden="true" style={{ ...base, boxShadow: `inset 0 0 0 1.2px ${T.tertiaryText}` }} />;
  }
  const color = kind === "ok" ? T.ok : kind === "warning" ? T.amber : T.red;
  return <span aria-hidden="true" style={{ ...base, background: color, opacity: kind === "ok" && dimmed ? 0.6 : 1 }} />;
}

export function Spinner() {
  return <span aria-hidden="true" className="spinner inline-block h-3 w-3 shrink-0" />;
}

/** A check mark before a done state (「Key 已保存」, 「已复制」, the signed-in email). */
export function CheckMark({ size = 11 }: { size?: number }) {
  return <Check aria-hidden="true" size={size + 2} strokeWidth={3} color={T.ok} className="shrink-0" />;
}

/** The app icon: the Windows mark (black strokes, transparent) on a white
 *  rounded tile, the way the macOS app icon looks on the dark window. */
export function AppIcon({ size }: { size: number }) {
  return (
    <span
      aria-hidden="true"
      className="inline-flex shrink-0 items-center justify-center"
      style={{ width: size, height: size, borderRadius: size * 0.225, background: "#F7F7F7" }}
    >
      <img src={appIcon} alt="" width={size * 0.56} height={size * 0.56} draggable={false} />
    </span>
  );
}

// MARK: - Switch

/** 30 × 18 switch in the panel green. */
export function Switch({
  checked,
  disabled,
  label,
  onChange,
}: {
  checked: boolean;
  disabled?: boolean;
  label: string;
  onChange: (value: boolean) => void;
}) {
  return (
    <button
      type="button"
      role="switch"
      aria-checked={checked}
      aria-label={label}
      disabled={disabled}
      onClick={() => onChange(!checked)}
      onPointerDown={(event) => event.stopPropagation()}
      className="settings-focus relative shrink-0 rounded-full transition-colors duration-100"
      style={{
        width: 30,
        height: 18,
        background: checked ? T.ok : "rgba(255,255,255,0.17)",
        opacity: disabled ? 0.45 : 1,
      }}
    >
      <span
        className="absolute top-[2px] rounded-full transition-[left] duration-100"
        style={{
          width: 14,
          height: 14,
          left: checked ? 14 : 2,
          background: "#F4F4F4",
          boxShadow: "0 0.5px 1.5px rgba(0,0,0,0.3)",
        }}
      />
    </button>
  );
}

// MARK: - Buttons

/** 24 px button: faint fill and hairline, radius 6, 13 px text; destructive
 *  ones in red; the one primary button (「链接」 while unlinked) light on dark. */
export function Button({
  kind = "normal",
  square = false,
  disabled,
  title,
  ariaLabel,
  onClick,
  children,
}: {
  kind?: "normal" | "danger" | "primary";
  square?: boolean;
  disabled?: boolean;
  title?: string;
  ariaLabel?: string;
  onClick: () => void;
  children: ReactNode;
}) {
  const primary = kind === "primary";
  return (
    <button
      type="button"
      disabled={disabled}
      title={title}
      aria-label={ariaLabel}
      onClick={onClick}
      onPointerDown={(event) => event.stopPropagation()}
      className="settings-focus settings-button inline-flex shrink-0 items-center justify-center whitespace-nowrap"
      style={{
        ...TYPE.body,
        fontWeight: primary ? 500 : 400,
        height: 24,
        width: square ? 24 : undefined,
        padding: square ? 0 : "0 10px",
        borderRadius: 6,
        color: kind === "danger" ? T.red : primary ? T.background : T.text,
        background: primary ? T.text : "rgba(255,255,255,0.10)",
        boxShadow: primary ? undefined : "inset 0 0 0 1px rgba(255,255,255,0.10)",
        opacity: disabled ? 0.45 : 1,
      }}
    >
      {children}
    </button>
  );
}

/** A button label with a leading symbol. */
export function IconLabel({ icon, title }: { icon: ReactNode; title: string }) {
  return (
    <span className="inline-flex items-center" style={{ gap: 5 }}>
      {icon}
      {title}
    </span>
  );
}

/** ＋ / － on the 数据目录 page: a borderless 22 px icon in the secondary color. */
export function IconButton({
  label,
  disabled,
  onClick,
  children,
}: {
  label: string;
  disabled?: boolean;
  onClick: () => void;
  children: ReactNode;
}) {
  return (
    <button
      type="button"
      aria-label={label}
      title={label}
      disabled={disabled}
      onClick={onClick}
      className="settings-focus settings-icon-button inline-flex shrink-0 items-center justify-center rounded"
      style={{ width: 22, height: 22, color: T.secondaryText }}
    >
      {children}
    </button>
  );
}

// MARK: - Segmented control

/** A two-way choice (the ZCode region): equal pill segments on a faint track,
 *  the selected one raised. */
export function Segmented<V extends string>({
  label,
  options,
  value,
  disabled,
  onChange,
}: {
  label: string;
  options: { value: V; title: string }[];
  value: V;
  disabled?: boolean;
  onChange: (value: V) => void;
}) {
  return (
    <div
      role="radiogroup"
      aria-label={label}
      className="inline-flex shrink-0"
      style={{ padding: 2, borderRadius: 7, background: "rgba(255,255,255,0.07)", opacity: disabled ? 0.6 : 1 }}
    >
      {options.map((option) => {
        const on = option.value === value;
        return (
          <button
            key={option.value}
            type="button"
            role="radio"
            aria-checked={on}
            disabled={disabled}
            onClick={() => onChange(option.value)}
            className="settings-focus whitespace-nowrap"
            style={{
              ...TYPE.body,
              color: T.text,
              height: 22,
              padding: "0 11px",
              borderRadius: 5,
              background: on ? "#505050" : "transparent",
              boxShadow: on ? `0 1px 1px rgba(0,0,0,0.14), inset 0 0 0 0.5px ${T.strongLine}` : undefined,
            }}
          >
            {option.title}
          </button>
        );
      })}
    </div>
  );
}

// MARK: - Text field

/** An inset text field: 26 px, dark fill, hairline, 12 px mono text, a 13 px
 *  prompt while empty and an optional clear button. */
export function TextField({
  prompt,
  value,
  secure = false,
  clearable = false,
  disabled,
  title,
  inputRef,
  onChange,
  onSubmit,
  onEscape,
}: {
  prompt: string;
  value: string;
  secure?: boolean;
  clearable?: boolean;
  disabled?: boolean;
  title?: string;
  inputRef?: Ref<HTMLInputElement>;
  onChange: (value: string) => void;
  onSubmit?: () => void;
  onEscape?: () => void;
}) {
  const showClear = clearable && value.length > 0;
  return (
    <span
      className="settings-field relative flex min-w-0 flex-1 items-center"
      title={title}
      style={{
        height: 26,
        borderRadius: 6,
        background: T.inset,
        boxShadow: `inset 0 0 0 1px ${T.strongLine}`,
        paddingLeft: 9,
        paddingRight: showClear ? 6 : 9,
        gap: 6,
        opacity: disabled ? 0.6 : 1,
      }}
    >
      {value.length === 0 && (
        <span
          aria-hidden="true"
          className="pointer-events-none absolute truncate"
          style={{ ...TYPE.body, color: T.tertiaryText, left: 9, right: 9 }}
        >
          {prompt}
        </span>
      )}
      <input
        ref={inputRef}
        type={secure ? "password" : "text"}
        value={value}
        aria-label={prompt}
        disabled={disabled}
        autoComplete="off"
        spellCheck={false}
        onChange={(event) => onChange(event.target.value)}
        onKeyDown={(event) => {
          if (event.key === "Enter") onSubmit?.();
          if (event.key === "Escape") onEscape?.();
        }}
        className="min-w-0 flex-1 bg-transparent outline-none"
        style={{ ...TYPE.mono, color: T.text, userSelect: "text" }}
      />
      {showClear && (
        <button
          type="button"
          aria-label="清空"
          title="清空"
          disabled={disabled}
          onClick={() => onChange("")}
          className="inline-flex shrink-0 items-center justify-center rounded-full"
          style={{ width: 13, height: 13, background: "rgba(255,255,255,0.17)", color: T.inset }}
        >
          <X size={9} strokeWidth={3} />
        </button>
      )}
    </span>
  );
}

// MARK: - Disclosure

/** The row-start chevron: right when closed, down when open. */
export function DisclosureTriangle({ open }: { open: boolean }) {
  return (
    <svg
      aria-hidden="true"
      width={12}
      height={12}
      viewBox="0 0 12 12"
      style={{ transform: open ? "rotate(90deg)" : undefined, transition: "transform 150ms ease-in-out" }}
    >
      <path
        d="M4.3 2.3 L8 6 L4.3 9.7"
        fill="none"
        stroke={T.secondaryText}
        strokeWidth={1.9}
        strokeLinecap="round"
        strokeLinejoin="round"
      />
    </svg>
  );
}

// MARK: - Pop-up menu

/**
 * A pop-up menu like the macOS one (System Settings' 「显示滚动条」): the
 * button shows the chosen option; the list opens under it with a check on
 * that option. Arrow keys move, Enter or Space picks, Escape or a click
 * outside closes; so does the window being hidden.
 */
export function MenuSelect<V extends string>({
  label,
  value,
  options,
  disabled,
  onChange,
}: {
  label: string;
  value: V;
  options: { value: V; label: string }[];
  disabled?: boolean;
  onChange: (value: V) => void;
}) {
  const [open, setOpen] = useState(false);
  const [active, setActive] = useState(0);
  const id = useId();
  const root = useRef<HTMLDivElement>(null);
  const button = useRef<HTMLButtonElement>(null);
  const list = useRef<HTMLUListElement>(null);
  const selected = Math.max(0, options.findIndex((option) => option.value === value));

  useEffect(() => {
    if (!open) return;
    setActive(selected);
    list.current?.focus();
    const close = (event: MouseEvent) => {
      if (!root.current?.contains(event.target as Node)) setOpen(false);
    };
    // The settings window is hidden, not destroyed, when closed: a menu left
    // open must not greet the user next time.
    const dismiss = () => setOpen(false);
    document.addEventListener("mousedown", close);
    window.addEventListener("blur", dismiss);
    document.addEventListener("visibilitychange", dismiss);
    return () => {
      document.removeEventListener("mousedown", close);
      window.removeEventListener("blur", dismiss);
      document.removeEventListener("visibilitychange", dismiss);
    };
  }, [open, selected]);

  const closeToButton = () => {
    setOpen(false);
    button.current?.focus();
  };

  const pick = (index: number) => {
    closeToButton();
    if (options[index].value !== value) onChange(options[index].value);
  };

  return (
    <div ref={root} className="relative shrink-0">
      <button
        ref={button}
        type="button"
        aria-label={label}
        aria-haspopup="listbox"
        aria-expanded={open}
        disabled={disabled}
        onClick={() => setOpen((value) => !value)}
        onKeyDown={(event) => {
          if (event.key === "ArrowDown" || event.key === "ArrowUp") {
            event.preventDefault();
            setOpen(true);
          }
        }}
        className="settings-focus settings-button flex items-center"
        style={{
          ...TYPE.body,
          color: T.text,
          height: 24,
          gap: 8,
          padding: "0 6px 0 12px",
          borderRadius: 6,
          background: "rgba(255,255,255,0.10)",
          opacity: disabled ? 0.45 : 1,
        }}
      >
        <span>{options[selected]?.label}</span>
        <ChevronsUpDown size={13} strokeWidth={2.2} color={T.text} />
      </button>
      {open && (
        <ul
          ref={list}
          role="listbox"
          aria-label={label}
          tabIndex={-1}
          aria-activedescendant={`${id}-${active}`}
          onKeyDown={(event) => {
            if (event.key === "ArrowDown") {
              event.preventDefault();
              setActive((index) => Math.min(options.length - 1, index + 1));
            } else if (event.key === "ArrowUp") {
              event.preventDefault();
              setActive((index) => Math.max(0, index - 1));
            } else if (event.key === "Enter" || event.key === " ") {
              event.preventDefault();
              pick(active);
            } else if (event.key === "Escape") {
              event.preventDefault();
              closeToButton();
            } else if (event.key === "Tab") {
              setOpen(false);
            }
          }}
          className="absolute right-0 top-full z-30 m-0 mt-1 min-w-full list-none outline-none"
          style={{
            padding: 4,
            borderRadius: 8,
            background: T.raised,
            boxShadow: `inset 0 0 0 1px ${T.strongLine}, 0 10px 24px rgba(0,0,0,0.45)`,
          }}
        >
          {options.map((option, index) => (
            <li
              key={option.value}
              id={`${id}-${index}`}
              role="option"
              aria-selected={index === selected}
              onMouseEnter={() => setActive(index)}
              onMouseDown={(event) => event.preventDefault()}
              onClick={() => pick(index)}
              className="flex cursor-default items-center whitespace-nowrap"
              style={{
                ...TYPE.body,
                color: T.text,
                gap: 6,
                height: 26,
                padding: "0 14px 0 6px",
                borderRadius: 5,
                background: index === active ? T.selection : "transparent",
              }}
            >
              <Check size={13} strokeWidth={2.4} style={{ opacity: index === selected ? 1 : 0 }} />
              {option.label}
            </li>
          ))}
        </ul>
      )}
    </div>
  );
}

// MARK: - Confirmation

/**
 * An in-window confirmation over the dimmed window (WinUI's ContentDialog;
 * macOS asks with a system alert): the question, one sentence, 取消 and the
 * red action. Escape cancels; focus starts on 取消.
 */
export function ConfirmDialog({
  title,
  message,
  confirmLabel,
  onConfirm,
  onCancel,
}: {
  title: string;
  message: string;
  confirmLabel: string;
  onConfirm: () => void;
  onCancel: () => void;
}) {
  const cancel = useRef<HTMLButtonElement>(null);
  const titleId = useId();
  const cancelHandler = useRef(onCancel);
  cancelHandler.current = onCancel;
  useEffect(() => {
    cancel.current?.focus();
    const onKey = (event: KeyboardEvent) => {
      if (event.key === "Escape") cancelHandler.current();
    };
    document.addEventListener("keydown", onKey);
    return () => document.removeEventListener("keydown", onKey);
  }, []);
  return (
    <div
      className="fixed inset-0 z-50 flex items-center justify-center"
      style={{ background: "rgba(0,0,0,0.45)" }}
      onMouseDown={(event) => {
        if (event.target === event.currentTarget) onCancel();
      }}
    >
      <div
        role="alertdialog"
        aria-modal="true"
        aria-labelledby={titleId}
        className="flex flex-col"
        style={{
          width: 300,
          padding: "18px 18px 14px",
          gap: 6,
          borderRadius: 12,
          background: T.raised,
          boxShadow: `inset 0 0 0 1px ${T.strongLine}, 0 18px 40px rgba(0,0,0,0.5)`,
        }}
      >
        <span id={titleId} style={{ ...TYPE.label, color: T.text, fontWeight: 600 }}>
          {title}
        </span>
        <span style={{ ...TYPE.body, color: T.secondaryText, lineHeight: 1.45 }}>{message}</span>
        <div className="flex justify-end" style={{ gap: 8, marginTop: 10 }}>
          <button
            ref={cancel}
            type="button"
            onClick={onCancel}
            className="settings-focus settings-button"
            style={{
              ...TYPE.body,
              color: T.text,
              height: 26,
              minWidth: 72,
              padding: "0 12px",
              borderRadius: 6,
              background: "rgba(255,255,255,0.10)",
              boxShadow: "inset 0 0 0 1px rgba(255,255,255,0.10)",
            }}
          >
            取消
          </button>
          <button
            type="button"
            onClick={onConfirm}
            className="settings-focus settings-button"
            style={{
              ...TYPE.body,
              fontWeight: 500,
              color: "#FFFFFF",
              height: 26,
              minWidth: 72,
              padding: "0 12px",
              borderRadius: 6,
              background: T.red,
            }}
          >
            {confirmLabel}
          </button>
        </div>
      </div>
    </div>
  );
}
