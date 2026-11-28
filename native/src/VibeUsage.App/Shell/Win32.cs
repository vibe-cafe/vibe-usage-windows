using System;
using System.Runtime.InteropServices;
using System.Text;

namespace VibeUsage.App.Shell;

/// <summary>
/// The Win32 calls the shell needs. Nothing here reads keyboard or mouse
/// input state or injects input: no input hooks, no hotkeys, no
/// GetAsyncKeyState, no SendInput. The only watching API is SetWinEventHook
/// for accessibility events (foreground, focus, capture), which does not see
/// keystrokes or pointer movement.
/// </summary>
internal static class Win32
{
    // -- messages ---------------------------------------------------------------------
    public const uint WM_NULL = 0x0000;
    public const uint WM_QUERYENDSESSION = 0x0011;
    public const uint WM_ENDSESSION = 0x0016;
    public const uint WM_SETTINGCHANGE = 0x001A;
    public const uint WM_DISPLAYCHANGE = 0x007E;
    public const uint WM_CONTEXTMENU = 0x007B;
    public const uint WM_TIMER = 0x0113;
    public const uint WM_LBUTTONDOWN = 0x0201;
    public const uint WM_RBUTTONDOWN = 0x0204;
    public const uint WM_RBUTTONUP = 0x0205;
    public const uint WM_DPICHANGED = 0x02E0;
    public const uint WM_USER = 0x0400;
    public const uint WM_APP = 0x8000;

    // -- notify icon ------------------------------------------------------------------
    public const uint NIM_ADD = 0x0;
    public const uint NIM_MODIFY = 0x1;
    public const uint NIM_DELETE = 0x2;
    public const uint NIM_SETVERSION = 0x4;
    public const uint NIF_MESSAGE = 0x1;
    public const uint NIF_ICON = 0x2;
    public const uint NIF_TIP = 0x4;
    public const uint NIF_SHOWTIP = 0x80;
    public const uint NOTIFYICON_VERSION_4 = 4;
    public const uint NIN_SELECT = WM_USER + 0;
    public const uint NIN_KEYSELECT = WM_USER + 1;

    // -- windows ----------------------------------------------------------------------
    public const uint WS_POPUP = 0x80000000;
    public const uint WS_EX_TOOLWINDOW = 0x00000080;
    public const long WS_EX_NOACTIVATE = 0x08000000;
    public const int GWL_EXSTYLE = -20;
    public const uint GA_ROOT = 2;
    public const uint MONITOR_DEFAULTTONEAREST = 2;
    public const uint MONITOR_DEFAULTTOPRIMARY = 1;
    public const int MDT_EFFECTIVE_DPI = 0;
    public const int SM_CXSMICON = 49;
    public const uint IMAGE_ICON = 1;
    public const uint LR_LOADFROMFILE = 0x10;

    // -- menus ------------------------------------------------------------------------
    public const uint MF_STRING = 0x0;
    public const uint MF_SEPARATOR = 0x800;
    public const uint TPM_LEFTALIGN = 0x0;
    public const uint TPM_RIGHTALIGN = 0x8;
    public const uint TPM_TOPALIGN = 0x0;
    public const uint TPM_BOTTOMALIGN = 0x20;
    public const uint TPM_RIGHTBUTTON = 0x2;
    public const uint TPM_NONOTIFY = 0x80;
    public const uint TPM_RETURNCMD = 0x100;
    public const uint TPM_VERTICAL = 0x40;
    public const uint TPM_HORIZONTAL = 0x0;
    public const int SM_MENUDROPALIGNMENT = 40;

    // -- win events -------------------------------------------------------------------
    public const uint EVENT_SYSTEM_FOREGROUND = 0x0003;
    public const uint EVENT_SYSTEM_CAPTURESTART = 0x0008;
    public const uint EVENT_OBJECT_FOCUS = 0x8005;
    public const uint WINEVENT_OUTOFCONTEXT = 0x0000;
    public const uint WINEVENT_SKIPOWNPROCESS = 0x0002;

    // -- dwm --------------------------------------------------------------------------
    public const int DWMWA_USE_IMMERSIVE_DARK_MODE = 20;
    public const int DWMWA_WINDOW_CORNER_PREFERENCE = 33;
    public const int DWMWA_CLOAK = 13;
    public const int DWMWA_EXTENDED_FRAME_BOUNDS = 9;
    public const int DWMWA_BORDER_COLOR = 34;
    public const int DWMWCP_ROUND = 2;

    [StructLayout(LayoutKind.Sequential)]
    public struct POINT
    {
        public int X;
        public int Y;
        public POINT(int x, int y) { X = x; Y = y; }
        public override string ToString() => $"({X},{Y})";
    }

    [StructLayout(LayoutKind.Sequential)]
    public struct RECT
    {
        public int Left, Top, Right, Bottom;
        public int Width => Right - Left;
        public int Height => Bottom - Top;
        public bool Contains(POINT p) => p.X >= Left && p.X < Right && p.Y >= Top && p.Y < Bottom;
        public override string ToString() => $"({Left},{Top} {Width}x{Height})";
    }

    [StructLayout(LayoutKind.Sequential, CharSet = CharSet.Unicode)]
    public struct MONITORINFO
    {
        public uint cbSize;
        public RECT rcMonitor;
        public RECT rcWork;
        public uint dwFlags;
    }

    [UnmanagedFunctionPointer(CallingConvention.Winapi)]
    public delegate IntPtr WndProc(IntPtr hWnd, uint msg, IntPtr wParam, IntPtr lParam);

    [UnmanagedFunctionPointer(CallingConvention.Winapi)]
    public delegate void WinEventProc(IntPtr hWinEventHook, uint eventType, IntPtr hwnd,
        int idObject, int idChild, uint idEventThread, uint dwmsEventTime);

    [StructLayout(LayoutKind.Sequential, CharSet = CharSet.Unicode)]
    public struct WNDCLASSEXW
    {
        public uint cbSize;
        public uint style;
        public IntPtr lpfnWndProc;
        public int cbClsExtra;
        public int cbWndExtra;
        public IntPtr hInstance;
        public IntPtr hIcon;
        public IntPtr hCursor;
        public IntPtr hbrBackground;
        [MarshalAs(UnmanagedType.LPWStr)] public string? lpszMenuName;
        [MarshalAs(UnmanagedType.LPWStr)] public string lpszClassName;
        public IntPtr hIconSm;
    }

    [StructLayout(LayoutKind.Sequential, CharSet = CharSet.Unicode)]
    public struct NOTIFYICONDATAW
    {
        public uint cbSize;
        public IntPtr hWnd;
        public uint uID;
        public uint uFlags;
        public uint uCallbackMessage;
        public IntPtr hIcon;
        [MarshalAs(UnmanagedType.ByValTStr, SizeConst = 128)] public string szTip;
        public uint dwState;
        public uint dwStateMask;
        [MarshalAs(UnmanagedType.ByValTStr, SizeConst = 256)] public string szInfo;
        public uint uVersion; // union with uTimeout
        [MarshalAs(UnmanagedType.ByValTStr, SizeConst = 64)] public string szInfoTitle;
        public uint dwInfoFlags;
        public Guid guidItem;
        public IntPtr hBalloonIcon;
    }

    [StructLayout(LayoutKind.Sequential)]
    public struct TPMPARAMS
    {
        public uint cbSize;
        public RECT rcExclude;
    }

    [StructLayout(LayoutKind.Sequential)]
    public struct NOTIFYICONIDENTIFIER
    {
        public uint cbSize;
        public IntPtr hWnd;
        public uint uID;
        public Guid guidItem;
    }

    // -- user32 -----------------------------------------------------------------------
    [DllImport("user32.dll", CharSet = CharSet.Unicode, SetLastError = true)]
    public static extern ushort RegisterClassExW(ref WNDCLASSEXW wc);

    [DllImport("user32.dll", CharSet = CharSet.Unicode, SetLastError = true)]
    public static extern IntPtr CreateWindowExW(uint exStyle, string className, string windowName,
        uint style, int x, int y, int width, int height, IntPtr parent, IntPtr menu, IntPtr instance, IntPtr param);

    [DllImport("user32.dll")]
    public static extern IntPtr DefWindowProcW(IntPtr hWnd, uint msg, IntPtr wParam, IntPtr lParam);

    [DllImport("user32.dll")]
    public static extern bool DestroyWindow(IntPtr hWnd);

    [DllImport("user32.dll", CharSet = CharSet.Unicode)]
    public static extern uint RegisterWindowMessageW(string name);

    [DllImport("user32.dll", CharSet = CharSet.Unicode)]
    public static extern IntPtr FindWindowW(string className, string? windowName);

    [DllImport("user32.dll")]
    public static extern bool PostMessageW(IntPtr hWnd, uint msg, IntPtr wParam, IntPtr lParam);

    [DllImport("user32.dll")]
    public static extern bool SetForegroundWindow(IntPtr hWnd);

    [DllImport("user32.dll")]
    public static extern IntPtr GetForegroundWindow();

    [DllImport("user32.dll")]
    public static extern bool AllowSetForegroundWindow(uint processId);

    [DllImport("user32.dll")]
    public static extern bool IsWindowVisible(IntPtr hWnd);

    [DllImport("user32.dll")]
    public static extern IntPtr GetAncestor(IntPtr hWnd, uint flags);

    [DllImport("user32.dll")]
    public static extern uint GetWindowThreadProcessId(IntPtr hWnd, out uint processId);

    [DllImport("user32.dll", CharSet = CharSet.Unicode)]
    public static extern int GetClassNameW(IntPtr hWnd, StringBuilder buffer, int max);

    [DllImport("user32.dll")]
    public static extern bool GetCursorPos(out POINT p);

    [DllImport("user32.dll")]
    public static extern bool GetWindowRect(IntPtr hWnd, out RECT rect);

    [DllImport("user32.dll")]
    public static extern bool GetClientRect(IntPtr hWnd, out RECT rect);

    [DllImport("user32.dll")]
    public static extern IntPtr MonitorFromPoint(POINT pt, uint flags);

    [DllImport("user32.dll")]
    public static extern IntPtr MonitorFromWindow(IntPtr hWnd, uint flags);

    [DllImport("user32.dll", CharSet = CharSet.Unicode)]
    public static extern bool GetMonitorInfoW(IntPtr monitor, ref MONITORINFO info);

    [DllImport("user32.dll")]
    public static extern uint GetDpiForWindow(IntPtr hWnd);

    [DllImport("user32.dll")]
    public static extern int GetSystemMetricsForDpi(int index, uint dpi);

    [DllImport("user32.dll", CharSet = CharSet.Unicode, SetLastError = true)]
    public static extern IntPtr LoadImageW(IntPtr instance, string name, uint type, int cx, int cy, uint load);

    [DllImport("user32.dll")]
    public static extern bool DestroyIcon(IntPtr icon);

    [DllImport("user32.dll")]
    public static extern IntPtr CreatePopupMenu();

    [DllImport("user32.dll", CharSet = CharSet.Unicode)]
    public static extern bool AppendMenuW(IntPtr menu, uint flags, UIntPtr id, string? text);

    [DllImport("user32.dll")]
    public static extern bool SetMenuDefaultItem(IntPtr menu, uint item, uint byPosition);

    [DllImport("user32.dll", EntryPoint = "TrackPopupMenuEx")]
    public static extern int TrackPopupMenuExWithParams(IntPtr menu, uint flags, int x, int y, IntPtr hWnd, ref TPMPARAMS tpm);

    [DllImport("user32.dll")]
    public static extern bool DestroyMenu(IntPtr menu);

    [DllImport("user32.dll")]
    public static extern int GetSystemMetrics(int index);

    [DllImport("user32.dll")]
    public static extern UIntPtr SetTimer(IntPtr hWnd, UIntPtr id, uint elapse, IntPtr timerProc);

    [DllImport("user32.dll")]
    public static extern bool KillTimer(IntPtr hWnd, UIntPtr id);

    [DllImport("user32.dll")]
    public static extern IntPtr SetWinEventHook(uint eventMin, uint eventMax, IntPtr module,
        WinEventProc proc, uint processId, uint threadId, uint flags);

    [DllImport("user32.dll")]
    public static extern bool UnhookWinEvent(IntPtr hook);

    [DllImport("user32.dll", EntryPoint = "GetWindowLongPtrW")]
    public static extern IntPtr GetWindowLongPtr(IntPtr hWnd, int index);

    [DllImport("user32.dll", EntryPoint = "SetWindowLongPtrW")]
    public static extern IntPtr SetWindowLongPtr(IntPtr hWnd, int index, IntPtr value);

    // -- shell32 / shcore / dwmapi / kernel32 ----------------------------------------
    [DllImport("shell32.dll", CharSet = CharSet.Unicode)]
    public static extern bool Shell_NotifyIconW(uint message, ref NOTIFYICONDATAW data);

    [DllImport("shell32.dll")]
    public static extern int Shell_NotifyIconGetRect(ref NOTIFYICONIDENTIFIER identifier, out RECT location);

    [DllImport("shcore.dll")]
    public static extern int GetDpiForMonitor(IntPtr monitor, int type, out uint dpiX, out uint dpiY);

    [DllImport("dwmapi.dll")]
    public static extern int DwmSetWindowAttribute(IntPtr hWnd, int attribute, ref int value, int size);

    [DllImport("dwmapi.dll")]
    public static extern int DwmGetWindowAttribute(IntPtr hWnd, int attribute, out RECT value, int size);

    [DllImport("kernel32.dll", CharSet = CharSet.Unicode)]
    public static extern IntPtr GetModuleHandleW(string? name);

    [DllImport("kernel32.dll", CharSet = CharSet.Unicode)]
    public static extern IntPtr LoadLibraryW(string name);

    [DllImport("kernel32.dll", EntryPoint = "GetProcAddress")]
    public static extern IntPtr GetProcAddressOrdinal(IntPtr module, IntPtr ordinal);

    // -- helpers ----------------------------------------------------------------------
    public static string ClassName(IntPtr hWnd)
    {
        if (hWnd == IntPtr.Zero) return "(none)";
        var sb = new StringBuilder(256);
        return GetClassNameW(hWnd, sb, sb.Capacity) > 0 ? sb.ToString() : "(unknown)";
    }

    public static uint ProcessOf(IntPtr hWnd)
    {
        GetWindowThreadProcessId(hWnd, out var pid);
        return pid;
    }

    public static bool TryMonitorInfo(IntPtr monitor, out MONITORINFO info)
    {
        info = new MONITORINFO { cbSize = (uint)Marshal.SizeOf<MONITORINFO>() };
        return monitor != IntPtr.Zero && GetMonitorInfoW(monitor, ref info);
    }

    public static uint DpiOfMonitor(IntPtr monitor) =>
        GetDpiForMonitor(monitor, MDT_EFFECTIVE_DPI, out var x, out _) == 0 && x > 0 ? x : 96;

    public static void SetDwmInt(IntPtr hWnd, int attribute, int value) =>
        DwmSetWindowAttribute(hWnd, attribute, ref value, sizeof(int));

    /// <summary>The visible window on screen (DWM extended frame bounds), or the window rect.</summary>
    public static RECT VisibleRect(IntPtr hWnd)
    {
        if (DwmGetWindowAttribute(hWnd, DWMWA_EXTENDED_FRAME_BOUNDS, out var e, Marshal.SizeOf<RECT>()) == 0 && e.Width > 0)
            return e;
        GetWindowRect(hWnd, out var w);
        return w;
    }

    public static short LoWord(IntPtr value) => unchecked((short)((long)value & 0xFFFF));

    public static short HiWord(IntPtr value) => unchecked((short)(((long)value >> 16) & 0xFFFF));
}
