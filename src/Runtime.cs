using System;
using System.Collections.Generic;
using System.IO;
using System.Security.Cryptography;

namespace QPlay
{
    internal static class Runtime
    {
        internal static readonly string Directory = Path.Combine(Program.InstallDirectory, "engine");
        internal static readonly Dictionary<string, string> Tools = new Dictionary<string, string> {
            { "mpv", Path.Combine(Directory, "mpv.exe") },
            { "ffmpeg", Path.Combine(Directory, "ffmpeg.exe") },
            { "ffprobe", Path.Combine(Directory, "ffprobe.exe") },
            { "ytDlp", Path.Combine(Directory, "yt-dlp.exe") }
        };

        internal static void Verify()
        {
            var manifest = Json.Read<BuildManifest>(Path.Combine(Program.InstallDirectory, "build-manifest.json"));
            if (manifest.product != "qplay" || manifest.version != Program.Version || manifest.protocolVersion != 1
                || manifest.files == null || !manifest.files.ContainsKey("engine/mpv.exe")
                || !manifest.files.ContainsKey("engine/profile/scripts/modernx.lua"))
                throw new InvalidDataException(Ui.Text("Установка qplay неполная. Запустите установщик qplay ещё раз.",
                    "This qplay installation is incomplete. Run the qplay installer again."));
            foreach (var item in manifest.files)
            {
                if (item.Key.Contains("\\") || item.Key.Contains(":") || item.Key.StartsWith("/")
                    || Array.Exists(item.Key.Split('/'), part => part.Length == 0 || part == "." || part == ".."))
                    throw new InvalidDataException(Ui.Text("Повреждён список компонентов qplay.", "Invalid qplay runtime manifest."));
                var path = Path.Combine(Program.InstallDirectory, item.Key.Replace('/', Path.DirectorySeparatorChar));
                if (!File.Exists(path) || (File.GetAttributes(path) & FileAttributes.ReparsePoint) != 0)
                    throw new InvalidDataException(Ui.Text("Отсутствует компонент qplay. Запустите установщик qplay ещё раз.",
                        "A qplay component is missing. Run the qplay installer again."));
                using (var input = File.OpenRead(path))
                using (var algorithm = SHA256.Create())
                {
                    var hash = BitConverter.ToString(algorithm.ComputeHash(input)).Replace("-", "").ToLowerInvariant();
                    if (!String.Equals(hash, item.Value, StringComparison.Ordinal))
                        throw new InvalidDataException(Ui.Text("Проверка целостности компонента qplay не пройдена. Запустите установщик qplay ещё раз.",
                            "A qplay component failed its integrity check. Run the qplay installer again."));
                }
            }
        }
    }

    internal sealed class BuildManifest
    {
        public string product { get; set; }
        public string version { get; set; }
        public int protocolVersion { get; set; }
        public Dictionary<string, string> files { get; set; }
    }
}
