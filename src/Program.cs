using System;
using System.Collections.Generic;
using System.Globalization;
using System.IO;
using System.Linq;
using System.Reflection;
using System.Text;
using System.Windows.Forms;

namespace QPlay
{
    internal static class Program
    {
        internal static readonly string Version = Assembly.GetExecutingAssembly().GetName().Version.ToString(3);
        internal static readonly string Executable = Assembly.GetExecutingAssembly().Location;
        internal static readonly string InstallDirectory = Path.GetDirectoryName(Executable);

        [STAThread]
        private static int Main(string[] arguments)
        {
            Application.EnableVisualStyles();
            Application.SetCompatibleTextRenderingDefault(false);
            try
            {
                var options = LaunchOptions.Parse(arguments);
                var paths = new PlayerPaths(options.ProfileDirectory);
                if (options.VersionOnly)
                {
                    Json.Write(Metadata());
                    return 0;
                }
                if (options.RuntimeInfo)
                {
                    Runtime.Verify();
                    var metadata = Metadata();
                    metadata["runtimeDir"] = Runtime.Directory;
                    metadata["tools"] = Runtime.Tools;
                    metadata["configDir"] = paths.Configuration;
                    metadata["dataDir"] = paths.Data;
                    Json.Write(metadata);
                    return 0;
                }

                paths.Create();
                if (options.SaveSubtitlePosition)
                {
                    paths.UpdatePreferences(updated => updated.subtitlePosition = options.SubtitlePosition);
                    return 0;
                }
                var preferences = paths.Preferences();
                Ui.Language = preferences.language;
                if (options.Settings)
                {
                    Application.Run(new SettingsForm(paths, options.PlayerIpc));
                    return 0;
                }
                if (options.Arguments.Count == 0)
                {
                    using (var home = new OpenForm(paths))
                    {
                        if (home.ShowDialog() != DialogResult.OK)
                            return 0;
                        options.Arguments.Add("--");
                        options.Arguments.Add(home.Media);
                    }
                }
                Runtime.Verify();
                return Player.Run(paths, options);
            }
            catch (Exception error)
            {
                var message = error.GetBaseException().Message;
                using (var output = new StreamWriter(Console.OpenStandardError(), new UTF8Encoding(false)))
                    output.WriteLine(message);
                if (!Native.OutputRedirected()
                    && !arguments.Any(argument => argument == "--runtime-info-json" || argument == "--version-json"))
                    MessageBox.Show(message, "qplay", MessageBoxButtons.OK, MessageBoxIcon.Error);
                return 1;
            }
        }

        internal static Dictionary<string, object> Metadata()
        {
            return new Dictionary<string, object> {
                { "product", "qplay" },
                { "version", Version },
                { "protocolVersion", 1 }
            };
        }
    }

    internal sealed class LaunchOptions
    {
        internal bool VersionOnly;
        internal bool RuntimeInfo;
        internal bool Settings;
        internal bool ExternalDownloads;
        internal bool SaveSubtitlePosition;
        internal double? SubtitlePosition;
        internal string ProfileDirectory;
        internal string PlayerIpc;
        internal readonly List<string> Arguments = new List<string>();

        internal static LaunchOptions Parse(string[] arguments)
        {
            var result = new LaunchOptions();
            var media = false;
            foreach (var argument in arguments)
            {
                if (media)
                {
                    result.Arguments.Add(argument);
                    continue;
                }
                if (argument == "--")
                    media = true;
                else if (argument == "--version-json")
                    result.VersionOnly = true;
                else if (argument == "--runtime-info-json")
                    result.RuntimeInfo = true;
                else if (argument == "--settings")
                    result.Settings = true;
                else if (argument == "--external-downloads")
                    result.ExternalDownloads = true;
                else if (argument.StartsWith("--save-subtitle-position=", StringComparison.Ordinal))
                {
                    var value = argument.Substring("--save-subtitle-position=".Length);
                    result.SaveSubtitlePosition = true;
                    result.SubtitlePosition = null;
                    if (value != "auto")
                    {
                        double position;
                        if (!Double.TryParse(value, NumberStyles.Float, CultureInfo.InvariantCulture, out position)
                            || Double.IsNaN(position) || Double.IsInfinity(position) || position < 0 || position > 100)
                            throw new ArgumentException(Ui.Text("Позиция субтитров должна быть от 0 до 100 или auto.",
                                "Subtitle position must be from 0 to 100 or auto."));
                        result.SubtitlePosition = position;
                    }
                }
                else if (argument.StartsWith("--profile-dir=", StringComparison.Ordinal))
                    result.ProfileDirectory = argument.Substring(14);
                else if (argument.StartsWith("--player-ipc=", StringComparison.Ordinal))
                    result.PlayerIpc = argument.Substring(13);
                else
                {
                    var name = argument.Split('=')[0];
                    if (name == "--config-dir" || name == "--no-config" || name == "--config"
                        || name == "--include" || name == "--script" || name == "--scripts"
                        || name.StartsWith("--scripts-", StringComparison.Ordinal)
                        || name == "--load-scripts" || name == "--no-load-scripts")
                        throw new ArgumentException(Ui.Text("qplay самостоятельно управляет конфигурацией и скриптами плеера.",
                            "qplay manages its player configuration and scripts."));
                    result.Arguments.Add(argument);
                }
                if (argument == "--")
                    result.Arguments.Add(argument);
            }
            return result;
        }
    }
}
