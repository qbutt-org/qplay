using System;
using System.Collections.Generic;
using System.Diagnostics;
using System.Globalization;
using System.IO;
using System.Linq;
using System.Threading;

namespace QPlay
{
    internal static class Player
    {
        internal static int Run(PlayerPaths paths, LaunchOptions options)
        {
            var preferences = paths.Preferences();
            var arguments = new List<string> {
                "--config-dir=" + Path.Combine(Runtime.Directory, "profile"),
                "--watch-later-directory=" + Path.Combine(paths.Data, "watch-later"),
                "--screenshot-directory=" + Environment.GetFolderPath(Environment.SpecialFolder.DesktopDirectory),
                "--title=${media-title} - qplay",
                "--terminal=no",
                "--resume-playback=" + (preferences.resume ? "yes" : "no"),
                "--save-position-on-quit=" + (preferences.resume ? "yes" : "no"),
                "--script-opts-append=modernx-language=" + preferences.language,
                "--script-opts-append=modernx-download_button=" + (options.ExternalDownloads ? "yes" : "no")
            };
            var userConfiguration = Path.Combine(paths.Configuration, "mpv.conf");
            if (File.Exists(userConfiguration))
                arguments.Add("--include=" + userConfiguration);
            if (preferences.subtitlePosition.HasValue)
                arguments.Add("--sub-pos=" + preferences.subtitlePosition.Value.ToString(CultureInfo.InvariantCulture));
            arguments.AddRange(options.Arguments);
            var ipc = options.Arguments.LastOrDefault(argument => argument.StartsWith("--input-ipc-server=", StringComparison.Ordinal));
            if (ipc == null)
            {
                ipc = "--input-ipc-server=\\\\.\\pipe\\qplay-" + Guid.NewGuid().ToString("N");
                arguments.Insert(0, ipc);
            }
            ipc = ipc.Substring("--input-ipc-server=".Length);

            var start = new ProcessStartInfo(Runtime.Tools["mpv"], String.Join(" ", arguments.Select(Native.Quote))) {
                WorkingDirectory = Runtime.Directory
            };
            start.EnvironmentVariables["QPLAY_EXECUTABLE"] = Program.Executable;
            start.EnvironmentVariables["QPLAY_MEDIA_RUNTIME"] = Path.Combine(paths.Data, "framegen");
            start.EnvironmentVariables["QPLAY_FFPROBE"] = Runtime.Tools["ffprobe"];
            start.EnvironmentVariables["QPLAY_IPC"] = ipc;
            start.EnvironmentVariables["QPLAY_CONFIG"] = paths.Configuration;
            start.EnvironmentVariables["QPLAY_DATA"] = paths.Data;
            start.EnvironmentVariables["QPLAY_FRAMEGEN"] = preferences.frameGeneration ? "yes" : "no";
            start.EnvironmentVariables["QPLAY_LANGUAGE"] = preferences.language;
            start.EnvironmentVariables["QPLAY_SUBTITLE_POSITION"] = preferences.subtitlePosition.HasValue ? "saved" : "auto";
            start.EnvironmentVariables["PATH"] = Runtime.Directory + ";" + Environment.GetEnvironmentVariable("PATH");
            start.EnvironmentVariables.Remove("VSSCRIPT_PATH");
            using (var job = new EngineJob())
            using (var player = Native.StartEngine(start, job))
            {
                var smallIcon = IntPtr.Zero;
                var largeIcon = IntPtr.Zero;
                try
                {
                    while (!player.HasExited)
                    {
                        player.Refresh();
                        var window = player.MainWindowHandle;
                        if (window != IntPtr.Zero)
                        {
                            smallIcon = Native.LoadPlayerIcon(window, true);
                            largeIcon = Native.LoadPlayerIcon(window, false);
                            Native.SetIcon(window, smallIcon, largeIcon);
                            break;
                        }
                        Thread.Sleep(50);
                    }

                    player.WaitForExit();
                    return player.ExitCode;
                }
                finally
                {
                    Native.DestroyIcon(smallIcon);
                    Native.DestroyIcon(largeIcon);
                }
            }
        }
    }
}
