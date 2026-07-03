// Unconfigured / device-link view — port of PopoverView.unconfiguredView.

import { useEffect, useRef, useState } from "react";
import { Info } from "lucide-react";
import { api, onDeviceLink } from "../lib/api";
import { useAppState } from "../state/AppStateContext";

type FlowState = "idle" | "awaitingApproval";

export function OnboardingView() {
  const state = useAppState();
  const [flowState, setFlowState] = useState<FlowState>("idle");
  const [pendingUserCode, setPendingUserCode] = useState<string | null>(null);
  const [setupError, setSetupError] = useState<string | null>(null);
  const flowStateRef = useRef(flowState);
  flowStateRef.current = flowState;

  useEffect(() => {
    const sub = onDeviceLink(async (e) => {
      if (flowStateRef.current !== "awaitingApproval") return;
      switch (e.status) {
        case "success":
          setPendingUserCode(null);
          setFlowState("idle");
          await state.markConfigured();
          break;
        case "denied":
          setSetupError("你拒绝了链接请求。");
          setPendingUserCode(null);
          setFlowState("idle");
          break;
        case "expired":
          setSetupError("验证码已过期，请重新登录。");
          setPendingUserCode(null);
          setFlowState("idle");
          break;
        case "error":
          setSetupError(`服务端返回未知错误：${e.message}`);
          setPendingUserCode(null);
          setFlowState("idle");
          break;
      }
    });
    return () => {
      void sub.then((un) => un());
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  const startFlow = async () => {
    setSetupError(null);
    setFlowState("awaitingApproval");
    setPendingUserCode(null);
    try {
      const { userCode } = await api.startDeviceLink();
      setPendingUserCode(userCode);
    } catch (err) {
      setSetupError(`无法连接服务端：${String(err)}`);
      setFlowState("idle");
    }
  };

  const cancelFlow = () => {
    void api.cancelDeviceLink();
    setPendingUserCode(null);
    setSetupError(null);
    setFlowState("idle");
  };

  return (
    <div className="flex flex-col">
      <div className="flex items-center gap-1.5 px-4 pb-2 pt-3">
        <span className="text-[15px] font-bold text-white">Vibe Usage</span>
        {state.status?.isDev && (
          <span
            className="rounded-[3px] px-1 py-px font-mono text-[10px] font-bold text-orange-400"
            style={{ background: "rgba(251,146,60,0.15)" }}
          >
            DEBUG
          </span>
        )}
      </div>
      <div className="h-px bg-card-border" />

      <div className="flex flex-col gap-4 p-4">
        {pendingUserCode && (
          <>
            <div
              className="flex items-start gap-2 rounded-card border border-card-border px-2.5 py-2"
              style={{ background: "#0F0F0F" }}
            >
              <Info size={12} color="#808080" className="mt-0.5 shrink-0" />
              <span className="text-xs" style={{ color: "#B3B3B3" }}>
                请确认浏览器中显示的验证码与下方一致
              </span>
            </div>

            <div className="flex flex-col gap-1.5">
              <span className="text-[10px] font-medium uppercase text-t-muted">验证码</span>
              <span className="font-mono text-[22px] font-semibold tracking-[3px] text-white">
                {pendingUserCode}
              </span>
            </div>
          </>
        )}

        {setupError && <div className="text-xs text-red-500">{setupError}</div>}

        <button
          className="flex w-full items-center justify-center gap-1.5 rounded-md bg-white py-2 text-[13px] font-medium text-black disabled:opacity-80"
          disabled={flowState === "awaitingApproval"}
          onClick={() => void startFlow()}
        >
          {flowState === "awaitingApproval" && <div className="spinner spinner-dark h-3 w-3" />}
          {flowState === "awaitingApproval" ? "等待浏览器确认…" : "登录并链接数据"}
        </button>

        {flowState === "awaitingApproval" && (
          <button
            className="w-full py-1.5 text-xs font-medium"
            style={{ color: "#999999" }}
            onClick={cancelFlow}
          >
            取消，重新开始
          </button>
        )}
      </div>
    </div>
  );
}
