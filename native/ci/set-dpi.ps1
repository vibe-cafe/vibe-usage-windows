# Runner only: changes the primary display's scaling live (what Settings →
# Display → Scale does), through DisplayConfigSetDeviceInfo's DPI-scale
# request (undocumented device info types -3/-4, as used by the open-source
# SetDPI tool). Prints what happened; never fails the job.
param([Parameter(Mandatory = $true)][int]$Percent)

Add-Type -TypeDefinition @"
using System;
using System.Runtime.InteropServices;
public static class VuDpi {
    [StructLayout(LayoutKind.Sequential)] public struct LUID { public uint LowPart; public int HighPart; }
    [StructLayout(LayoutKind.Sequential)] public struct SOURCE { public LUID adapterId; public uint id; public uint modeInfoIdx; public uint statusFlags; }
    [StructLayout(LayoutKind.Sequential)] public struct RATIONAL { public uint n; public uint d; }
    [StructLayout(LayoutKind.Sequential)] public struct TARGET { public LUID adapterId; public uint id; public uint modeInfoIdx; public uint outputTechnology; public uint rotation; public uint scaling; public RATIONAL refreshRate; public uint scanLineOrdering; public int targetAvailable; public uint statusFlags; }
    [StructLayout(LayoutKind.Sequential)] public struct PATH { public SOURCE source; public TARGET target; public uint flags; }
    [StructLayout(LayoutKind.Sequential)] public struct MODE { public uint infoType; public uint id; public LUID adapterId; public ulong u0, u1, u2, u3, u4, u5; }
    [StructLayout(LayoutKind.Sequential)] public struct HEADER { public int type; public uint size; public LUID adapterId; public uint id; }
    [StructLayout(LayoutKind.Sequential)] public struct DPI_GET { public HEADER header; public int minRel; public int curRel; public int maxRel; }
    [StructLayout(LayoutKind.Sequential)] public struct DPI_SET { public HEADER header; public int scaleRel; }
    [DllImport("user32.dll")] public static extern int GetDisplayConfigBufferSizes(uint flags, out uint paths, out uint modes);
    [DllImport("user32.dll")] public static extern int QueryDisplayConfig(uint flags, ref uint numPaths, [Out] PATH[] paths, ref uint numModes, [Out] MODE[] modes, IntPtr topology);
    [DllImport("user32.dll")] public static extern int DisplayConfigGetDeviceInfo(ref DPI_GET packet);
    [DllImport("user32.dll")] public static extern int DisplayConfigSetDeviceInfo(ref DPI_SET packet);
    static readonly int[] Steps = { 100, 125, 150, 175, 200, 225, 250, 300, 350, 400, 450, 500 };

    public static string Set(int percent) {
        uint np, nm;
        if (GetDisplayConfigBufferSizes(2, out np, out nm) != 0) return "GetDisplayConfigBufferSizes failed";
        var paths = new PATH[np]; var modes = new MODE[nm];
        if (QueryDisplayConfig(2, ref np, paths, ref nm, modes, IntPtr.Zero) != 0 || np == 0) return "QueryDisplayConfig failed";
        var src = paths[0].source;
        var get = new DPI_GET();
        get.header.type = -3; get.header.size = (uint)Marshal.SizeOf(typeof(DPI_GET));
        get.header.adapterId = src.adapterId; get.header.id = src.id;
        int hr = DisplayConfigGetDeviceInfo(ref get);
        if (hr != 0) return "get failed " + hr;
        int recommended = Math.Abs(get.minRel);
        int current = recommended + get.curRel;
        int target = Array.IndexOf(Steps, percent);
        string range = "allowed " + Steps[recommended + get.minRel] + "%.." + Steps[Math.Min(Steps.Length - 1, recommended + get.maxRel)] + "%, current " + Steps[current] + "%";
        if (target < 0) return "unknown step; " + range;
        int rel = target - recommended;
        if (rel < get.minRel || rel > get.maxRel) return "out of range; " + range;
        var set = new DPI_SET();
        set.header.type = -4; set.header.size = (uint)Marshal.SizeOf(typeof(DPI_SET));
        set.header.adapterId = src.adapterId; set.header.id = src.id; set.scaleRel = rel;
        hr = DisplayConfigSetDeviceInfo(ref set);
        return (hr == 0 ? "ok " : "set failed " + hr + " ") + range;
    }
}
"@
$result = [VuDpi]::Set($Percent)
Write-Host "set-dpi ${Percent}%: $result"
Start-Sleep -Seconds 4
if ($result.StartsWith('ok')) { exit 0 } else { exit 3 }
