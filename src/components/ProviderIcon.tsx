// Official provider artwork, shared by the quota cards' headers and the
// Settings product rows so both surfaces resolve the same asset the same way
// (mirrors RateLimitCardView.ProviderIcon). Assets are the app's own copies of
// each vendor's standard container mark, taken at @2x (56 px) so a 14 px CSS
// box stays crisp on HiDPI displays; no recolouring, no added border.

import { useState } from "react";
import { Code, Sparkles, SquareTerminal } from "lucide-react";
import { RateLimitProvider } from "../lib/types";
import claudeIcon from "../assets/claude-icon.png";
import codexIcon from "../assets/codex-icon.png";
import cursorIcon from "../assets/cursor-icon.png";
import grokIcon from "../assets/grok-icon.png";
import kimiIcon from "../assets/kimi-icon.png";
import openCodeIcon from "../assets/opencode-icon@2x.png";
import zcodeIcon from "../assets/zcode-icon.png";

const ICONS: Partial<Record<RateLimitProvider, string>> = {
  codex: codexIcon,
  claudeCode: claudeIcon,
  "kimi-code": kimiIcon,
  zcode: zcodeIcon,
  grok: grokIcon,
  cursor: cursorIcon,
  "opencode-go": openCodeIcon,
};

const FALLBACKS: Record<RateLimitProvider, typeof Sparkles> = {
  codex: SquareTerminal,
  claudeCode: Sparkles,
  "kimi-code": Sparkles,
  zcode: Sparkles,
  grok: Sparkles,
  cursor: Sparkles,
  antigravity: Sparkles,
  "opencode-go": Code,
};

export function ProviderIcon({
  provider,
  size = 14,
}: {
  provider: RateLimitProvider;
  size?: number;
}) {
  const [failed, setFailed] = useState(false);
  if (!failed && ICONS[provider]) {
    return (
      <img
        src={ICONS[provider]}
        width={size}
        height={size}
        className="shrink-0"
        onError={() => setFailed(true)}
        alt=""
      />
    );
  }
  const Fallback = FALLBACKS[provider];
  return <Fallback size={size - 1} color="#999999" className="shrink-0" />;
}
