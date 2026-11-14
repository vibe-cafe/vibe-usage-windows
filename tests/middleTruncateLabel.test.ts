import { createElement } from "react";
import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it } from "vitest";
import { MiddleTruncateLabel } from "../src/components/MiddleTruncateLabel";

// The label renders as two flex items (head + 6-char tail). A flex item drops
// whitespace at its own edges, so without `whitespace-pre` a space at the cut
// disappeared: "Gemini 4 Carbon" showed as "Gemini 4Carbon".

function halves(label: string): { text: string; className: string }[] {
  const html = renderToStaticMarkup(createElement(MiddleTruncateLabel, { label }));
  return [...html.matchAll(/<span class="([^"]*)">([^<]*)<\/span>/g)].map(([, className, text]) => ({
    className,
    text,
  }));
}

describe("MiddleTruncateLabel", () => {
  it.each([
    ["Gemini 4 Carbon", "Gemini 4 ", "Carbon"],
    ["GPT-6.1 Terra", "GPT-6.1", " Terra"],
    ["Gemini 4 Carbon (High)", "Gemini 4 Carbon ", "(High)"],
  ])("keeps the space where %s is cut", (label, head, tail) => {
    const parts = halves(label);
    expect(parts.map((part) => part.text)).toEqual([head, tail]);
    for (const part of parts) {
      expect(part.className.split(" ")).toContain("whitespace-pre");
    }
  });

  it("still truncates the head and pins the tail", () => {
    const [head, tail] = halves("sudoHG/an-extremely-long-monorepo-project-name-repo-a");
    expect(head.className.split(" ")).toContain("truncate");
    expect(tail.className.split(" ")).toContain("shrink-0");
    expect(tail.text).toBe("repo-a");
  });

  it("renders short labels as one span", () => {
    const html = renderToStaticMarkup(createElement(MiddleTruncateLabel, { label: "Codex" }));
    expect(html.match(/<span/g)).toHaveLength(1);
    expect(html).toContain(">Codex</span>");
  });
});
