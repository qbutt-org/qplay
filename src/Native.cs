using System;
using System.Collections;
using System.ComponentModel;
using System.Diagnostics;
using System.Linq;
using System.Runtime.InteropServices;
using System.Text;
using Microsoft.Win32.SafeHandles;

namespace QPlay
{
    internal static class Native
    {
        internal static bool OutputRedirected()
        {
            return GetFileType(GetStdHandle(-11)) == 3 || GetFileType(GetStdHandle(-12)) == 3;
        }

        internal static void SetDarkTitleBar(IntPtr window)
        {
            var enabled = 1;
            DwmSetWindowAttribute(window, 20, ref enabled, sizeof(int));
        }

        internal static string Quote(string value)
        {
            var result = new StringBuilder("\"");
            var slashes = 0;
            foreach (var character in value)
            {
                if (character == '\\')
                {
                    slashes++;
                    continue;
                }
                result.Append('\\', character == '"' ? slashes * 2 + 1 : slashes);
                result.Append(character);
                slashes = 0;
            }
            return result.Append('\\', slashes * 2).Append('"').ToString();
        }

        internal static IntPtr LoadPlayerIcon()
        {
            return LoadImage(IntPtr.Zero, System.IO.Path.Combine(Program.InstallDirectory, "qplay.ico"), 1, 256, 256, 0x10);
        }

        internal static void SetIcon(IntPtr window, IntPtr icon)
        {
            SendMessage(window, 0x80, new IntPtr(0), icon);
            SendMessage(window, 0x80, new IntPtr(1), icon);
        }

        internal static void StartInstaller(string executable, string arguments)
        {
            bool inJob;
            if (!IsProcessInJob(GetCurrentProcess(), IntPtr.Zero, out inJob))
                throw new Win32Exception();
            var startup = new StartupInfo { Size = Marshal.SizeOf(typeof(StartupInfo)) };
            ProcessInformation process;
            if (!CreateProcess(executable, new StringBuilder(Quote(executable) + " " + arguments),
                IntPtr.Zero, IntPtr.Zero, false, inJob ? 0x01000000U : 0U,
                IntPtr.Zero, Program.InstallDirectory, ref startup, out process))
                throw new Win32Exception();
            CloseHandle(process.Thread);
            CloseHandle(process.Process);
        }

        internal static Process StartEngine(ProcessStartInfo start, EngineJob job)
        {
            var entries = start.EnvironmentVariables.Cast<DictionaryEntry>()
                .OrderBy(entry => (string)entry.Key, StringComparer.OrdinalIgnoreCase)
                .Select(entry => entry.Key + "=" + entry.Value);
            var environment = Marshal.StringToHGlobalUni(String.Join("\0", entries) + "\0\0");
            var startup = new StartupInfo { Size = Marshal.SizeOf(typeof(StartupInfo)) };
            ProcessInformation native;
            try
            {
                // Assign the suspended process before it can create probes, scripts or
                // runtime installers, so every descendant belongs to the same job.
                if (!CreateProcess(start.FileName, new StringBuilder(Quote(start.FileName) + " " + start.Arguments),
                    IntPtr.Zero, IntPtr.Zero, false, 0x08000000 | 0x00000400 | 0x00000004,
                    environment, start.WorkingDirectory, ref startup, out native))
                    throw new Win32Exception();
            }
            finally { Marshal.FreeHGlobal(environment); }
            var process = Process.GetProcessById((int)native.ProcessId);
            try
            {
                job.Add(process);
                if (ResumeThread(native.Thread) == UInt32.MaxValue)
                    throw new Win32Exception();
                return process;
            }
            catch
            {
                if (!process.HasExited)
                    process.Kill();
                process.Dispose();
                throw;
            }
            finally
            {
                CloseHandle(native.Thread);
                CloseHandle(native.Process);
            }
        }

        [StructLayout(LayoutKind.Sequential, CharSet = CharSet.Unicode)]
        private struct StartupInfo
        {
            internal int Size;
            internal string Reserved;
            internal string Desktop;
            internal string Title;
            internal uint X, Y, Width, Height, Columns, Rows, Fill, Flags;
            internal ushort ShowWindow, ReservedSize;
            internal IntPtr ReservedData, Input, Output, Error;
        }

        [StructLayout(LayoutKind.Sequential)]
        private struct ProcessInformation
        {
            internal IntPtr Process, Thread;
            internal uint ProcessId, ThreadId;
        }

        [DllImport("kernel32.dll", SetLastError = true)]
        internal static extern bool CloseHandle(IntPtr handle);
        [DllImport("kernel32.dll", SetLastError = true)]
        private static extern uint ResumeThread(IntPtr thread);
        [DllImport("kernel32.dll")]
        private static extern IntPtr GetStdHandle(int type);
        [DllImport("kernel32.dll")]
        private static extern uint GetFileType(IntPtr handle);
        [DllImport("kernel32.dll")]
        private static extern IntPtr GetCurrentProcess();
        [DllImport("kernel32.dll", SetLastError = true)]
        private static extern bool IsProcessInJob(IntPtr process, IntPtr job, out bool result);
        [DllImport("kernel32.dll", EntryPoint = "CreateProcessW", CharSet = CharSet.Unicode, SetLastError = true)]
        private static extern bool CreateProcess(string application, StringBuilder commandLine,
            IntPtr processAttributes, IntPtr threadAttributes, bool inherit, uint flags,
            IntPtr environment, string directory, ref StartupInfo startup, out ProcessInformation process);
        [DllImport("user32.dll", CharSet = CharSet.Unicode)]
        private static extern IntPtr LoadImage(IntPtr instance, string name, uint type, int width, int height, uint flags);
        [DllImport("user32.dll")]
        private static extern IntPtr SendMessage(IntPtr window, uint message, IntPtr wParam, IntPtr lParam);
        [DllImport("user32.dll")]
        internal static extern bool DestroyIcon(IntPtr icon);
        [DllImport("dwmapi.dll")]
        private static extern int DwmSetWindowAttribute(IntPtr window, int attribute, ref int value, int size);
    }

    internal sealed class EngineJob : SafeHandleZeroOrMinusOneIsInvalid
    {
        internal EngineJob() : base(true)
        {
            SetHandle(CreateJobObject(IntPtr.Zero, null));
            if (IsInvalid)
                throw new Win32Exception();
            // All engine descendants close with the launcher. An explicitly initiated
            // installer may break away so an update survives that normal shutdown.
            var limits = new ExtendedLimits();
            limits.Basic.LimitFlags = 0x2000 | 0x0800;
            if (!SetInformationJobObject(handle, 9, ref limits, Marshal.SizeOf(typeof(ExtendedLimits))))
                throw new Win32Exception();
        }

        internal void Add(Process process)
        {
            if (!AssignProcessToJobObject(handle, process.Handle))
            {
                var error = new Win32Exception();
                if (!process.HasExited)
                    process.Kill();
                throw error;
            }
        }

        protected override bool ReleaseHandle() { return Native.CloseHandle(handle); }

        [StructLayout(LayoutKind.Sequential)]
        private struct BasicLimits
        {
            internal long ProcessTime, JobTime;
            internal uint LimitFlags;
            internal UIntPtr MinimumWorkingSet, MaximumWorkingSet;
            internal uint ActiveProcessLimit;
            internal UIntPtr Affinity;
            internal uint PriorityClass, SchedulingClass;
        }

        [StructLayout(LayoutKind.Sequential)]
        private struct ExtendedLimits
        {
            internal BasicLimits Basic;
            internal ulong ReadOperations, WriteOperations, OtherOperations, ReadBytes, WriteBytes, OtherBytes;
            internal UIntPtr ProcessMemory, JobMemory, PeakProcessMemory, PeakJobMemory;
        }

        [DllImport("kernel32.dll", CharSet = CharSet.Unicode, SetLastError = true)]
        private static extern IntPtr CreateJobObject(IntPtr attributes, string name);
        [DllImport("kernel32.dll", SetLastError = true)]
        private static extern bool SetInformationJobObject(IntPtr job, int informationClass, ref ExtendedLimits information, int length);
        [DllImport("kernel32.dll", SetLastError = true)]
        private static extern bool AssignProcessToJobObject(IntPtr job, IntPtr process);
    }
}
